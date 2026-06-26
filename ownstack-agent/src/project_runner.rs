//! Project Runner — a bounded, persisted SDLC state machine over the agent.
//!
//! Ports Syllabe's process discipline onto OwnStack's existing primitives:
//! `AgentOrchestrator::{plan, process, critique, execute_tool}` +
//! `MissionManager` (work-unit persistence, checkpoints, event stream).
//!
//! Pipeline per run:
//! ```text
//! PLAN  →  for each work-unit: IMPLEMENT → TEST ⇄ REPAIR(≤max_repair)
//!                              → REVIEW(critique ≤max_review) → CHECKPOINT
//! ```
//! Unlike the freeform Worker loop, every unit must pass real tests (the test
//! command is actually executed via the sandbox) AND a Critic verdict.

use crate::mission::manager::MissionManager;
use crate::mission::models::MissionStatus;
use crate::orchestrator::AgentOrchestrator;
use serde::Serialize;
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Clone, Serialize)]
pub struct UnitOutcome {
    pub description: String,
    pub tests_passed: Option<bool>,
    pub repair_attempts: u32,
    pub review_cycles: u32,
    pub approved: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectOutcome {
    pub mission_id: String,
    pub goal: String,
    pub units: Vec<UnitOutcome>,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectConfig {
    /// Max TEST⇄REPAIR iterations per work-unit (Syllabe: 3).
    pub max_repair: u32,
    /// Max REVIEW MUST_FIX cycles per work-unit (Syllabe: 2).
    pub max_review: u32,
    /// Optional shell command run after IMPLEMENT to gate the unit on real tests.
    pub test_command: Option<String>,
    /// Guard against planner over-decomposition: if PLAN yields more than this
    /// many micro-steps, collapse them into a single coarse work-unit (the goal),
    /// so a tiny task isn't run through TEST/REVIEW once per trivial step.
    pub max_units: u32,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self { max_repair: 3, max_review: 2, test_command: None, max_units: 5 }
    }
}

pub struct ProjectRunner {
    orchestrator: AgentOrchestrator,
    manager: MissionManager,
    workspace: PathBuf,
    config: ProjectConfig,
}

impl ProjectRunner {
    pub fn new(
        orchestrator: AgentOrchestrator,
        workspace: PathBuf,
        config: ProjectConfig,
    ) -> Self {
        let manager = MissionManager::new(&workspace);
        Self { orchestrator, manager, workspace, config }
    }

    /// Borrow the manager (e.g. to subscribe to events for UI streaming).
    pub fn manager(&self) -> &MissionManager {
        &self.manager
    }

    pub async fn run(&mut self, goal: &str) -> ProjectOutcome {
        let mission_id = format!("proj-{}", now_ms());
        self.manager.create_mission(&mission_id, goal, goal);

        // ── PLAN ────────────────────────────────────────────────────────────
        self.manager.update_status(&mission_id, MissionStatus::Planning, "decomposing into work units");
        let mission = match self.orchestrator.plan(goal).await {
            Ok(m) => m,
            Err(e) => {
                self.manager.update_status(&mission_id, MissionStatus::Failed, &format!("plan failed: {e}"));
                return ProjectOutcome { mission_id, goal: goal.to_string(), units: vec![], success: false };
            }
        };
        // Collapse over-decomposed plans into one coarse unit (the goal).
        let units: Vec<String> = if mission.steps.len() as u32 > self.config.max_units {
            self.manager.add_log(&mission_id, &format!("plan had {} micro-steps > max_units {} — collapsing to one coarse unit", mission.steps.len(), self.config.max_units));
            vec![goal.to_string()]
        } else {
            mission.steps.iter().map(|s| s.description.clone()).collect()
        };
        self.persist_work_units(goal, &units);
        self.manager.update_status(&mission_id, MissionStatus::Running, &format!("{} work units", units.len()));

        // ── Per work-unit loop ──────────────────────────────────────────────
        let mut outcomes = Vec::new();
        let n = units.len();
        for (i, desc) in units.iter().enumerate() {
            let desc = desc.clone();
            self.manager.add_log(&mission_id, &format!("[unit {}/{}] {}", i + 1, n, desc));

            // IMPLEMENT
            let mut last_output = self
                .orchestrator
                .process(&format!("Work unit {}/{}: {desc}\nImplement it now using your tools.", i + 1, n))
                .await
                .unwrap_or_default();

            let mut uo = UnitOutcome {
                description: desc.clone(),
                tests_passed: None,
                repair_attempts: 0,
                review_cycles: 0,
                approved: false,
            };

            // TEST ⇄ REPAIR (real test execution, bounded)
            if let Some(cmd) = self.config.test_command.clone() {
                for attempt in 0..self.config.max_repair {
                    let res = self
                        .orchestrator
                        .execute_tool("exec", serde_json::json!({ "command": cmd }))
                        .await;
                    if res.success {
                        uo.tests_passed = Some(true);
                        break;
                    }
                    uo.tests_passed = Some(false);
                    uo.repair_attempts = attempt + 1;
                    self.manager.add_log(&mission_id, &format!("tests failed (repair {}/{})", attempt + 1, self.config.max_repair));
                    let out = clip(&res.stdout, 1200);
                    let err = clip(&res.stderr, 1200);
                    last_output = self
                        .orchestrator
                        .process(&format!("The test command `{cmd}` failed.\nstdout:\n{out}\nstderr:\n{err}\nFix the code so the tests pass."))
                        .await
                        .unwrap_or_default();
                }
            }

            // REVIEW (Critic verdict, bounded MUST_FIX cycles)
            for cycle in 0..self.config.max_review {
                match self.orchestrator.critique(&desc, &last_output).await {
                    Ok(c) if c.approved => {
                        uo.approved = true;
                        break;
                    }
                    Ok(c) => {
                        uo.review_cycles = cycle + 1;
                        self.manager.add_log(&mission_id, &format!("review MUST_FIX: {}", clip(&c.feedback, 120)));
                        let suggestions = c.suggestions.join("; ");
                        last_output = self
                            .orchestrator
                            .process(&format!("A code reviewer requires fixes for: {desc}\nFeedback: {}\nSuggestions: {}\nApply the fixes now.", c.feedback, suggestions))
                            .await
                            .unwrap_or_default();
                    }
                    Err(_) => break,
                }
            }

            self.manager.create_checkpoint(&mission_id, &format!("unit {}/{} done", i + 1, n), None);
            outcomes.push(uo);
        }

        let success = !outcomes.is_empty()
            && outcomes.iter().all(|u| u.approved && u.tests_passed != Some(false));
        let final_status = if success { MissionStatus::Completed } else { MissionStatus::NeedsReview };
        self.manager.update_status(&mission_id, final_status, "project run finished");
        info!("ProjectRunner: '{mission_id}' success={success} units={}", outcomes.len());

        ProjectOutcome { mission_id, goal: goal.to_string(), units: outcomes, success }
    }

    fn persist_work_units(&self, goal: &str, units: &[String]) {
        let units_json = serde_json::json!({
            "goal": goal,
            "units": units.iter().map(|d| serde_json::json!({ "description": d })).collect::<Vec<_>>(),
        });
        let dir = self.workspace.join(".ownstack");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("work-units.json"), serde_json::to_string_pretty(&units_json).unwrap_or_default());
    }
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
