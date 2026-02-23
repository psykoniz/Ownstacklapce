//! Multivers - A/B Testing at Infrastructure Level
//!
//! Runs sandbox executions with different configurations and compares
//! results using multi-objective scoring.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use ownstack_engine::{ProcessSandbox, Sandbox, SandboxLevel};

use super::{ToolDef, ToolResult, Toolkit, ToolkitError};

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
    fn score_result(result: &ForkResult, min_duration: u64) -> f64 {
        let mut score = 0.0;

        if result.exit_code == 0 {
            score += 50.0;
            let perf =
                20.0 * (min_duration as f64 / result.duration_ms.max(1) as f64);
            score += perf;
            if !result.stdout.contains("WARNING")
                && !result.stderr.to_lowercase().contains("error")
            {
                score += 20.0;
            }
            if result.stderr.is_empty() {
                score += 10.0;
            }
        }

        (score * 100.0).round() / 100.0
    }

    /// Calculate scores and determine winner.
    ///
    /// Tie-break is deterministic: lexicographically smaller variant name wins.
    pub fn evaluate(&mut self) {
        let min_duration = self
            .results
            .values()
            .filter(|r| r.exit_code == 0)
            .map(|r| r.duration_ms)
            .min()
            .unwrap_or(1);

        let mut names: Vec<String> = self.results.keys().cloned().collect();
        names.sort();

        let mut best_score = -1.0_f64;
        let mut winner: Option<String> = None;

        for name in names {
            let Some(result) = self.results.get_mut(&name) else {
                continue;
            };
            let score = Self::score_result(result, min_duration);
            result.score = score;

            if result.exit_code != 0 {
                continue;
            }

            let is_better = score > best_score;
            let is_tie_better_name = (score - best_score).abs() < f64::EPSILON
                && winner.as_ref().map(|w| name < *w).unwrap_or(true);

            if is_better || is_tie_better_name {
                best_score = score;
                winner = Some(name);
            }
        }

        self.winner = winner;
        self.completed = true;
    }
}

pub struct MultiversToolkit {
    workspace: PathBuf,
}

impl MultiversToolkit {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    fn max_parallel() -> usize {
        std::env::var("OWNSTACK_MULTIVERS_MAX_PARALLEL")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(4)
    }

    fn early_stop_threshold() -> f64 {
        std::env::var("OWNSTACK_MULTIVERS_EARLY_STOP_SCORE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(95.0)
    }

    /// Run a command with multiple variant configurations.
    pub async fn fork_and_run(
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

        let max_parallel = Self::max_parallel();
        let early_stop_threshold = Self::early_stop_threshold();
        let semaphore = Arc::new(Semaphore::new(max_parallel));

        let mut ordered_variants: Vec<(String, VariantConfig)> = variants
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        ordered_variants.sort_by(|a, b| a.0.cmp(&b.0));

        let mut join_set = tokio::task::JoinSet::new();
        for (name, config) in ordered_variants {
            let workspace = self.workspace.clone();
            let command = command.to_string();
            let semaphore = Arc::clone(&semaphore);
            join_set.spawn(async move {
                let permit = semaphore.acquire_owned().await;
                if permit.is_err() {
                    return ForkResult {
                        variant_name: name,
                        status: ForkStatus::Failed,
                        exit_code: 1,
                        stdout: String::new(),
                        stderr: "failed to acquire concurrency permit".to_string(),
                        duration_ms: 0,
                        score: 0.0,
                    };
                }
                let _permit = permit.ok();

                let name_for_spawn = name.clone();
                match tokio::task::spawn(async move {
                    Self::run_variant_in_workspace(
                        &workspace,
                        &name_for_spawn,
                        &command,
                        &config,
                    )
                    .await
                })
                .await
                {
                    Ok(result) => result,
                    Err(err) => ForkResult {
                        variant_name: name,
                        status: ForkStatus::Failed,
                        exit_code: 1,
                        stdout: String::new(),
                        stderr: format!("variant task join failure: {}", err),
                        duration_ms: 0,
                        score: 0.0,
                    },
                }
            });
        }

        let mut early_stop_triggered = false;

        while let Some(joined) = join_set.join_next().await {
            match joined {
                Ok(result) => {
                    debug!(
                        "Multivers: variant '{}' completed (exit_code={})",
                        result.variant_name, result.exit_code
                    );
                    let variant_name = result.variant_name.clone();
                    run.results.insert(variant_name, result);

                    // Optional early stop when an excellent candidate is found.
                    if !early_stop_triggered {
                        let min_duration = run
                            .results
                            .values()
                            .filter(|r| r.exit_code == 0)
                            .map(|r| r.duration_ms)
                            .min()
                            .unwrap_or(1);

                        let current_best = run
                            .results
                            .values()
                            .filter(|r| r.exit_code == 0)
                            .map(|r| MultiversRun::score_result(r, min_duration))
                            .fold(0.0_f64, f64::max);

                        if current_best >= early_stop_threshold {
                            early_stop_triggered = true;
                            info!(
                                "Multivers: early stop triggered at score {:.2}",
                                current_best
                            );
                            join_set.abort_all();
                        }
                    }
                }
                Err(err) => {
                    warn!("Multivers: join error while awaiting variant: {}", err);
                }
            }
        }

        run.evaluate();
        info!("Multivers: winner = {:?}", run.winner);
        run
    }

    async fn run_variant_in_workspace(
        workspace: &PathBuf,
        name: &str,
        command: &str,
        config: &VariantConfig,
    ) -> ForkResult {
        debug!("Multivers: running variant '{}'", name);

        let sandbox = ProcessSandbox;
        let start = std::time::Instant::now();

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

        for setup_cmd in &config.setup_commands {
            debug!("Multivers variant '{}' setup: {}", name, setup_cmd);
            let _ = sandbox
                .exec(setup_cmd, workspace, SandboxLevel::Standard)
                .await;
        }

        let result = sandbox
            .exec(&full_command, workspace, SandboxLevel::Standard)
            .await;
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
            score: 0.0,
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
            description: "Run a command with multiple variant configurations in parallel and compare results"
                .to_string(),
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
                let run = self.fork_and_run(&parsed.command, &parsed.variants).await;
                let output = serde_json::to_string_pretty(&run)
                    .unwrap_or_else(|_| format!("winner: {:?}", run.winner));
                Ok(ToolResult::success(output))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_deterministic_tie_break() {
        let mut run = MultiversRun {
            run_id: "x".to_string(),
            command: "cmd".to_string(),
            results: HashMap::from([
                (
                    "b-variant".to_string(),
                    ForkResult {
                        variant_name: "b-variant".to_string(),
                        status: ForkStatus::Completed,
                        exit_code: 0,
                        stdout: "ok".to_string(),
                        stderr: "".to_string(),
                        duration_ms: 10,
                        score: 0.0,
                    },
                ),
                (
                    "a-variant".to_string(),
                    ForkResult {
                        variant_name: "a-variant".to_string(),
                        status: ForkStatus::Completed,
                        exit_code: 0,
                        stdout: "ok".to_string(),
                        stderr: "".to_string(),
                        duration_ms: 10,
                        score: 0.0,
                    },
                ),
            ]),
            winner: None,
            completed: false,
        };

        run.evaluate();
        assert_eq!(run.winner.as_deref(), Some("a-variant"));
    }

    #[tokio::test]
    async fn test_fork_and_run_no_variants() {
        let toolkit = MultiversToolkit::new(PathBuf::from("."));
        let variants = HashMap::new();
        let run = toolkit.fork_and_run("echo test", &variants).await;
        assert!(run.completed);
        assert!(run.results.is_empty());
        assert!(run.winner.is_none());
    }
}
