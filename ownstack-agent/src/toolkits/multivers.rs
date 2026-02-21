//! Multivers — A/B Testing at Infrastructure Level
//!
//! Ported from Python multivers.py.
//! Runs parallel sandbox executions with different configurations
//! and compares results using multi-objective scoring.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info};

use ownstack_engine::{ProcessSandbox, Sandbox, SandboxLevel};

use super::{ToolDef, ToolResult, Toolkit, ToolkitError};

// ─── Types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ForkStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkResult {
    pub variant_name: String,
    pub status: ForkStatus,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiversRun {
    pub run_id: String,
    pub command: String,
    pub results: HashMap<String, ForkResult>,
    pub winner: Option<String>,
    pub completed: bool,
}

impl MultiversRun {
    /// Calculate scores and determine winner
    pub fn evaluate(&mut self) {
        let mut best_score = 0.0_f64;
        let mut winner = None;

        let min_duration = self
            .results
            .values()
            .filter(|r| r.exit_code == 0)
            .map(|r| r.duration_ms)
            .min()
            .unwrap_or(1);

        for (name, result) in self.results.iter_mut() {
            let mut score = 0.0;

            if result.exit_code == 0 {
                score += 50.0; // Success weight

                // Performance bonus (20 max, fastest gets full)
                let perf =
                    20.0 * (min_duration as f64 / result.duration_ms.max(1) as f64);
                score += perf;

                // Quality bonus (no warnings/errors in output)
                if !result.stdout.contains("WARNING")
                    && !result.stderr.contains("error")
                {
                    score += 20.0;
                }

                // Clean output bonus
                if result.stderr.is_empty() {
                    score += 10.0;
                }
            }

            result.score = (score * 100.0).round() / 100.0;

            if result.exit_code == 0 && score > best_score {
                best_score = score;
                winner = Some(name.clone());
            }
        }

        self.winner = winner;
        self.completed = true;
    }
}

// ─── Multivers Toolkit ─────────────────────────────────────────────

pub struct MultiversToolkit {
    workspace: PathBuf,
}

impl MultiversToolkit {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    /// Run a command with multiple variant configurations
    pub fn fork_and_run(
        &self,
        command: &str,
        variants: &HashMap<String, VariantConfig>,
    ) -> MultiversRun {
        let run_id = format!(
            "multivers-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let mut run = MultiversRun {
            run_id: run_id.clone(),
            command: command.to_string(),
            results: HashMap::new(),
            winner: None,
            completed: false,
        };

        info!(
            "Multivers: starting {} variants for '{}'",
            variants.len(),
            command
        );

        for (name, config) in variants {
            let result = self.run_variant(name, command, config);
            run.results.insert(name.clone(), result);
        }

        run.evaluate();

        info!("Multivers: winner = {:?}", run.winner);
        run
    }

    fn run_variant(
        &self,
        name: &str,
        command: &str,
        config: &VariantConfig,
    ) -> ForkResult {
        debug!("Multivers: running variant '{}'", name);

        let sandbox = ProcessSandbox;
        let start = std::time::Instant::now();

        // Build the full command with any env prefix
        let full_command = if config.env_vars.is_empty() {
            command.to_string()
        } else {
            let env_prefix: String = config
                .env_vars
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{} {}", env_prefix, command)
        };

        // Run setup commands first
        for setup_cmd in &config.setup_commands {
            debug!("Multivers variant '{}' setup: {}", name, setup_cmd);
            let _ = sandbox.exec(setup_cmd, &self.workspace, SandboxLevel::Standard);
        }

        // Run main command
        let result =
            sandbox.exec(&full_command, &self.workspace, SandboxLevel::Standard);
        let duration_ms = start.elapsed().as_millis() as u64;

        ForkResult {
            variant_name: name.to_string(),
            status: if result.success {
                ForkStatus::Completed
            } else {
                ForkStatus::Failed
            },
            exit_code: if result.success { 0 } else { 1 },
            stdout: result.stdout,
            stderr: result.stderr,
            duration_ms,
            score: 0.0, // Calculated in evaluate()
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct VariantConfig {
    #[serde(default)]
    pub env_vars: HashMap<String, String>,
    #[serde(default)]
    pub setup_commands: Vec<String>,
}

#[derive(Deserialize)]
struct MultiversArgs {
    command: String,
    variants: HashMap<String, VariantConfig>,
}

#[async_trait]
impl Toolkit for MultiversToolkit {
    fn name(&self) -> &str {
        "multivers"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![ToolDef {
            name: "multivers_run".to_string(),
            description: "Run a command with multiple variant configurations in parallel and compare results".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Command to run in each variant"
                    },
                    "variants": {
                        "type": "object",
                        "description": "Map of variant names to configs with optional env_vars and setup_commands"
                    }
                },
                "required": ["command", "variants"]
            }),
        }]
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        match tool_name {
            "multivers_run" => {
                let parsed: MultiversArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                let run = self.fork_and_run(&parsed.command, &parsed.variants);
                let output = serde_json::to_string_pretty(&run)
                    .unwrap_or_else(|_| format!("winner: {:?}", run.winner));
                Ok(ToolResult::success(output))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}
