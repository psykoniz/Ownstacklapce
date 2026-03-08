use lapce_rpc::ownstack::{
    AgentModeState, AgentRunState, BudgetSnapshot, ContextSnapshot, MissionSnapshot,
    OwnStackRpc, PendingApprovalSnapshot, ToolEventSnapshot, UiStateDelta,
};
use ownstack_agent::orchestrator::{
    AgentBudget, AgentOrchestrator, AgentRunMode, RuntimeBudgetSnapshot,
    RuntimeContextSnapshot,
};
use ownstack_agent::policy_approval::{PolicyApprovalManager, RpcSink};
use ownstack_agent::provider::LlmProvider;
use ownstack_agent::providers::anthropic::AnthropicProvider;
use ownstack_agent::providers::local::LocalProvider;
use ownstack_agent::providers::openrouter::OpenRouterProvider;
use ownstack_agent::secret_store;
use ownstack_agent::toolkits::mcp::{McpServerConfig, McpToolkit};
use ownstack_engine::{PolicyEngine, ProcessSandbox};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

fn send_rpc_notification(rpc: OwnStackRpc) {
    let msg = serde_json::to_string(&rpc).unwrap_or_default() + "\n";
    let _ = std::io::Write::write_all(&mut std::io::stdout(), msg.as_bytes());
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    mcp: bool,

    #[arg(short, long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct BudgetFile {
    max_steps: Option<u32>,
    max_exec_commands: Option<u32>,
    max_files_modified: Option<u32>,
    max_duration_minutes: Option<u32>,
    max_consecutive_failures: Option<u32>,
    max_llm_calls: Option<u32>,
}

fn default_mcp_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct McpServersFile {
    #[serde(default)]
    servers: Vec<McpServerFileEntry>,
}

#[derive(Debug, Deserialize)]
struct McpServerFileEntry {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default = "default_mcp_enabled")]
    enabled: bool,
}

fn load_mcp_server_configs(workspace: &std::path::Path) -> Vec<McpServerConfig> {
    let path = workspace.join(".ownstack").join("mcp_servers.json");
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) => {
            debug!("No MCP server config at {:?}: {}", path, err);
            return Vec::new();
        }
    };

    let parsed: McpServersFile = match serde_json::from_str(&content) {
        Ok(parsed) => parsed,
        Err(err) => {
            warn!("Failed to parse MCP server config {:?}: {}", path, err);
            return Vec::new();
        }
    };

    parsed
        .servers
        .into_iter()
        .filter(|entry| entry.enabled)
        .map(|entry| McpServerConfig {
            name: entry.name,
            command: entry.command,
            args: entry.args,
            env: entry.env,
        })
        .collect()
}

#[derive(Debug, Clone)]
struct RuntimeUiState {
    mode: AgentModeState,
    run_state: AgentRunState,
}

impl RuntimeUiState {
    fn from_env() -> Self {
        let mode = match std::env::var("OWNSTACK_AGENT_MODE")
            .ok()
            .map(|v| v.to_ascii_lowercase())
            .as_deref()
        {
            Some("auto") => AgentModeState::Auto,
            Some("plan") => AgentModeState::Plan,
            _ => AgentModeState::Ask,
        };

        Self {
            mode,
            run_state: AgentRunState::Idle,
        }
    }
}

fn runtime_mode_from_rpc(mode: &AgentModeState) -> AgentRunMode {
    match mode {
        AgentModeState::Ask => AgentRunMode::Ask,
        AgentModeState::Auto => AgentRunMode::Auto,
        AgentModeState::Plan => AgentRunMode::Plan,
    }
}

fn send_ui_delta(delta: UiStateDelta) {
    send_rpc_notification(OwnStackRpc::UiStateDelta { delta });
}

fn send_budget_context_updates(
    budget: RuntimeBudgetSnapshot,
    context: RuntimeContextSnapshot,
) {
    send_rpc_notification(OwnStackRpc::BudgetUpdate {
        tokens: budget.tokens,
        max_tokens: budget.max_tokens,
        steps: u64::from(budget.steps),
        max_steps: u64::from(budget.max_steps),
        calls: u64::from(budget.calls),
        max_calls: u64::from(budget.max_calls),
    });
    send_rpc_notification(OwnStackRpc::ContextUpdate {
        current: context.current,
        max: context.max,
    });
    send_ui_delta(UiStateDelta {
        mode: None,
        run_state: None,
        budget: Some(BudgetSnapshot {
            tokens: budget.tokens,
            max_tokens: budget.max_tokens,
            steps: u64::from(budget.steps),
            max_steps: u64::from(budget.max_steps),
            calls: u64::from(budget.calls),
            max_calls: u64::from(budget.max_calls),
        }),
        context: Some(ContextSnapshot {
            current: context.current,
            max: context.max,
        }),
        mission: None,
        pending_approval: None,
        tool_event: None,
        alert: None,
    });
}

fn emit_runtime_state(state: &RuntimeUiState) {
    send_ui_delta(UiStateDelta {
        mode: Some(state.mode.clone()),
        run_state: Some(state.run_state.clone()),
        budget: None,
        context: None,
        mission: None,
        pending_approval: None,
        tool_event: None,
        alert: None,
    });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    // Synchronize provider secrets between env and OS keyring.
    secret_store::sync_env_and_keyring();

    let args = Args::parse();
    let workspace = args.workspace.unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    });

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let session_id = format!("sess-{}-{}", std::process::id(), now_ms);

    info!("OwnStack Agent starting in {:?}", workspace);
    info!("Session: {}", session_id);

    // Initialize provider based on env.
    let provider_preference = std::env::var("OWNSTACK_PROVIDER")
        .ok()
        .map(|v| v.to_ascii_lowercase());
    let has_anthropic = secret_store::has_secret("ANTHROPIC_API_KEY");
    let has_openrouter = secret_store::has_secret("OPENROUTER_API_KEY");
    let provider: Arc<dyn LlmProvider> = match provider_preference.as_deref() {
        Some("anthropic") if has_anthropic => {
            info!("LLM Provider: Anthropic (preferred)");
            Arc::new(AnthropicProvider::from_env()?)
        }
        Some("openrouter") if has_openrouter => {
            info!("LLM Provider: OpenRouter (preferred)");
            Arc::new(OpenRouterProvider::from_env()?)
        }
        Some("local") | Some("ollama") => {
            info!("LLM Provider: Local (preferred)");
            Arc::new(LocalProvider::from_env()?)
        }
        Some(pref) => {
            warn!(
                "Preferred provider '{}' is unavailable; falling back to auto selection",
                pref
            );
            if has_anthropic {
                info!("LLM Provider: Anthropic");
                Arc::new(AnthropicProvider::from_env()?)
            } else if has_openrouter {
                info!("LLM Provider: OpenRouter");
                Arc::new(OpenRouterProvider::from_env()?)
            } else {
                info!("LLM Provider: Local");
                Arc::new(LocalProvider::from_env()?)
            }
        }
        None => {
            if has_anthropic {
                info!("LLM Provider: Anthropic");
                Arc::new(AnthropicProvider::from_env()?)
            } else if has_openrouter {
                info!("LLM Provider: OpenRouter");
                Arc::new(OpenRouterProvider::from_env()?)
            } else {
                info!("LLM Provider: Local");
                Arc::new(LocalProvider::from_env()?)
            }
        }
    };

    let mut orchestrator = AgentOrchestrator::new(
        provider.clone(),
        workspace.clone(),
        128000,
        &session_id,
    );
    let mut runtime_state = RuntimeUiState::from_env();
    orchestrator.set_mode(runtime_mode_from_rpc(&runtime_state.mode));

    // Load budgets from .ownstack/budgets.json when available.
    let budgets_path = workspace.join(".ownstack").join("budgets.json");
    if let Ok(content) = std::fs::read_to_string(&budgets_path) {
        match serde_json::from_str::<BudgetFile>(&content) {
            Ok(bf) => {
                let mut budget = AgentBudget::default();
                if let Some(v) = bf.max_steps {
                    budget.max_steps = v;
                }
                if let Some(v) = bf.max_exec_commands {
                    budget.max_tool_calls = v;
                }
                if let Some(v) = bf.max_llm_calls {
                    budget.max_llm_calls = v;
                }
                if let Some(v) = bf.max_files_modified {
                    budget.max_files_modified = v;
                }
                if let Some(v) = bf.max_duration_minutes {
                    budget.max_duration_minutes = v;
                }
                if let Some(v) = bf.max_consecutive_failures {
                    budget.max_consecutive_failures = v;
                }
                orchestrator.set_budget(budget);
                info!("Budgets loaded from {:?}", budgets_path);
            }
            Err(e) => {
                warn!("Failed to parse budgets at {:?}: {}", budgets_path, e);
            }
        }
    } else {
        info!(
            "No budgets file found at {:?} (using defaults)",
            budgets_path
        );
    }

    let rpc_sink: RpcSink = Arc::new(|rpc: OwnStackRpc| {
        if let OwnStackRpc::PolicyPrompt {
            command, reason, ..
        } = &rpc
        {
            send_ui_delta(UiStateDelta {
                mode: None,
                run_state: Some(AgentRunState::AwaitingApproval),
                budget: None,
                context: None,
                mission: None,
                pending_approval: Some(PendingApprovalSnapshot {
                    command: command.clone(),
                    reason: reason.clone(),
                    timeout_ms: Some(300_000),
                }),
                tool_event: None,
                alert: None,
            });
        }
        send_rpc_notification(rpc);
    });
    let approval_manager = if args.mcp {
        None
    } else {
        Some(Arc::new(PolicyApprovalManager::new(rpc_sink)))
    };

    // Create shared resources
    let policy = Arc::new(PolicyEngine);
    let sandbox = Arc::new(ProcessSandbox);

    // Create toolkits
    let core_toolkit = Arc::new(ownstack_agent::toolkits::core::CoreToolkit::new(
        workspace.clone(),
        session_id.clone(),
        approval_manager.clone(),
    ));
    let git_toolkit = Arc::new(ownstack_agent::toolkits::git::GitToolkit::new(
        workspace.clone(),
        session_id.clone(),
        approval_manager.clone(),
        policy.clone(),
        sandbox.clone(),
        provider.clone(),
    ));
    let lsp_toolkit = Arc::new(ownstack_agent::toolkits::lsp::LspToolkit::new(
        workspace.clone(),
    ));
    let healer_toolkit =
        Arc::new(ownstack_agent::toolkits::healer::HealerToolkit::new(
            workspace.clone(),
            Some(provider.clone()),
        ));
    let multivers_toolkit =
        Arc::new(ownstack_agent::toolkits::multivers::MultiversToolkit::new(
            workspace.clone(),
        ));
    let vision_toolkit =
        Arc::new(ownstack_agent::toolkits::vision::VisionToolkit::new(
            workspace.clone(),
            session_id.clone(),
        ));

    // Register default toolkits
    orchestrator.register_toolkit(core_toolkit.clone());
    orchestrator.register_toolkit(git_toolkit.clone());
    orchestrator.register_toolkit(lsp_toolkit.clone());
    orchestrator.register_toolkit(healer_toolkit.clone());
    orchestrator.register_toolkit(multivers_toolkit.clone());
    orchestrator.register_toolkit(vision_toolkit.clone());
    orchestrator
        .register_toolkit(Arc::new(ownstack_agent::toolkits::extra::ExtraToolkit));
    orchestrator.register_toolkit(Arc::new(
        ownstack_agent::toolkits::browser::BrowserToolkit,
    ));
    orchestrator.register_toolkit(Arc::new(
        ownstack_agent::toolkits::time_machine::TimeMachineToolkit::new(
            workspace.clone(),
        ),
    ));
    orchestrator.register_toolkit(Arc::new(
        ownstack_agent::toolkits::specialists::PMToolkit,
    ));
    orchestrator.register_toolkit(Arc::new(
        ownstack_agent::toolkits::specialists::QAToolkit,
    ));
    orchestrator.register_toolkit(Arc::new(
        ownstack_agent::toolkits::specialists::SecurityToolkit,
    ));
    orchestrator.register_toolkit(Arc::new(
        ownstack_agent::toolkits::specialists::ReviewerToolkit,
    ));
    orchestrator.register_toolkit(Arc::new(
        ownstack_agent::toolkits::specialists::DocsToolkit,
    ));
    orchestrator.register_toolkit(Arc::new(
        ownstack_agent::toolkits::specialists::DesignerToolkit,
    ));

    // Connect configured MCP servers and expose their tools as part of the toolkit set.
    let mut mcp_toolkit = McpToolkit::new();
    let mut connected_mcp_servers = 0usize;
    for server in load_mcp_server_configs(&workspace) {
        let server_name = server.name.clone();
        match mcp_toolkit.add_server(server).await {
            Ok(()) => {
                connected_mcp_servers += 1;
                info!("Connected MCP server '{}'", server_name);
            }
            Err(err) => {
                warn!("Failed to connect MCP server '{}': {}", server_name, err);
            }
        }
    }
    if connected_mcp_servers > 0 {
        orchestrator.register_toolkit(Arc::new(mcp_toolkit));
        info!(
            "Registered MCP toolkit with {} connected server(s)",
            connected_mcp_servers
        );
    }

    // Load WASI Plugins
    let plugin_host =
        ownstack_agent::plugins::WasiPluginHost::new(workspace.clone());
    let plugins_dir = workspace.join("plugins");
    info!("WASI plugin search in: {:?}", plugins_dir);
    let wasi_toolkits = plugin_host.load_all(&plugins_dir).await;
    info!("WASI plugins found: {}", wasi_toolkits.len());
    for tk in &wasi_toolkits {
        orchestrator.register_toolkit(tk.clone());
    }

    // Run InfraSense health check at startup
    let health_warnings =
        ownstack_agent::infra_sense::InfraSense::health_check(&workspace);
    for warning in &health_warnings {
        warn!("InfraSense: {}", warning);
    }

    if args.mcp {
        info!("Starting in MCP Server mode");
        let mut mcp_server =
            ownstack_agent::mcp_server::McpServer::new("ownstack-agent", "0.1.0");
        mcp_server.register_toolkit(core_toolkit);
        mcp_server.register_toolkit(git_toolkit);
        mcp_server.register_toolkit(lsp_toolkit);
        mcp_server.register_toolkit(healer_toolkit);
        mcp_server.register_toolkit(multivers_toolkit);

        mcp_server.run_stdio().await.map_err(|e| e.into())
    } else {
        info!("Starting in IDE RPC mode");

        // Emit any InfraSense warnings to the UI
        for warning in &health_warnings {
            send_ui_delta(UiStateDelta {
                mode: None,
                run_state: None,
                budget: None,
                context: None,
                mission: None,
                pending_approval: None,
                tool_event: None,
                alert: Some(lapce_rpc::ownstack::AlertSnapshot {
                    severity: lapce_rpc::ownstack::AlertSeverity::Warning,
                    message: warning.clone(),
                }),
            });
        }

        runtime_state.run_state = AgentRunState::Idle;
        emit_runtime_state(&runtime_state);
        send_budget_context_updates(
            orchestrator.budget_snapshot(),
            orchestrator.context_snapshot(),
        );
        let (work_tx, mut work_rx) = mpsc::unbounded_channel::<OwnStackRpc>();
        let work_tx_reader = work_tx.clone();
        let approval_for_reader = approval_manager.clone();

        tokio::spawn(async move {
            let mut reader = BufReader::new(tokio::io::stdin());
            info!("Agent RPC stdin loop started");

            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<OwnStackRpc>(trimmed) {
                            Ok(rpc) => match rpc {
                                OwnStackRpc::PolicyResponse {
                                    approved,
                                    correlation_id,
                                } => {
                                    if let Some(mgr) = approval_for_reader.as_ref() {
                                        mgr.resolve(approved, &correlation_id).await;
                                    }
                                }
                                OwnStackRpc::KillSwitch => {
                                    info!("Kill switch received — shutting down.");
                                    std::process::exit(0);
                                }
                                OwnStackRpc::AiPrompt { .. }
                                | OwnStackRpc::ToolExec { .. }
                                | OwnStackRpc::SetAgentMode { .. }
                                | OwnStackRpc::SuggestionDecision { .. }
                                | OwnStackRpc::UiSnapshot { .. }
                                | OwnStackRpc::CaptureScreenshot
                                | OwnStackRpc::UiSnapshotRequest => {
                                    let _ = work_tx_reader.send(rpc);
                                }
                                _ => {
                                    debug!("Unhandled RPC (stdin): {:?}", rpc);
                                }
                            },
                            Err(e) => {
                                error!(
                                    "Failed to parse RPC: {} | Line: {}",
                                    e, trimmed
                                );
                            }
                        }
                    }
                    Err(e) => {
                        error!("Read error: {}", e);
                        break;
                    }
                }
            }
        });

        while let Some(rpc) = work_rx.recv().await {
            match rpc {
                OwnStackRpc::AiPrompt { prompt } => {
                    runtime_state.run_state = AgentRunState::Running;
                    emit_runtime_state(&runtime_state);
                    send_budget_context_updates(
                        orchestrator.budget_snapshot(),
                        orchestrator.context_snapshot(),
                    );
                    let result = orchestrator
                        .stream_process(
                            &prompt,
                            |chunk| {
                                let chunk_rpc = OwnStackRpc::AiStreamChunk {
                                    content_delta: chunk.delta_content,
                                    tool_call_delta: if chunk
                                        .delta_tool_calls
                                        .is_empty()
                                    {
                                        None
                                    } else {
                                        serde_json::to_value(&chunk.delta_tool_calls)
                                            .ok()
                                    },
                                    finish_reason: chunk
                                        .finish_reason
                                        .map(|r| format!("{:?}", r)),
                                };
                                send_rpc_notification(chunk_rpc);

                                if let Some(name) = chunk
                                    .delta_tool_calls
                                    .iter()
                                    .find_map(|tc| tc.name.clone())
                                {
                                    send_ui_delta(UiStateDelta {
                                        mode: None,
                                        run_state: None,
                                        budget: None,
                                        context: None,
                                        mission: None,
                                        pending_approval: None,
                                        tool_event: Some(ToolEventSnapshot {
                                            tool_name: name,
                                            status: "streaming".to_string(),
                                            summary: None,
                                            duration_ms: None,
                                        }),
                                        alert: None,
                                    });
                                }
                            },
                            |mission| {
                                let goal = mission.goal;
                                let steps: Vec<(String, String)> = mission
                                    .steps
                                    .iter()
                                    .map(|s| {
                                        (
                                            s.description.clone(),
                                            format!("{:?}", s.status),
                                        )
                                    })
                                    .collect();
                                let mission_rpc = OwnStackRpc::MissionUpdate {
                                    goal: goal.clone(),
                                    steps: steps.clone(),
                                };
                                send_rpc_notification(mission_rpc);
                                send_ui_delta(UiStateDelta {
                                    mode: None,
                                    run_state: None,
                                    budget: None,
                                    context: None,
                                    mission: Some(MissionSnapshot { goal, steps }),
                                    pending_approval: None,
                                    tool_event: None,
                                    alert: None,
                                });
                            },
                            |budget, context| {
                                send_budget_context_updates(budget, context);
                            },
                        )
                        .await;

                    if let Err(e) = result {
                        error!("Process error: {}", e);
                        runtime_state.run_state = AgentRunState::Error;
                        emit_runtime_state(&runtime_state);
                        send_ui_delta(UiStateDelta {
                            mode: None,
                            run_state: None,
                            budget: None,
                            context: None,
                            mission: None,
                            pending_approval: None,
                            tool_event: None,
                            alert: Some(lapce_rpc::ownstack::AlertSnapshot {
                                severity: lapce_rpc::ownstack::AlertSeverity::Error,
                                message: format!("Agent run failed: {}", e),
                            }),
                        });
                    } else {
                        runtime_state.run_state = AgentRunState::Idle;
                        emit_runtime_state(&runtime_state);
                        send_budget_context_updates(
                            orchestrator.budget_snapshot(),
                            orchestrator.context_snapshot(),
                        );
                    }
                }
                OwnStackRpc::SetAgentMode { mode } => {
                    runtime_state.mode = mode.clone();
                    orchestrator.set_mode(runtime_mode_from_rpc(&mode));
                    emit_runtime_state(&runtime_state);
                    info!("Agent runtime mode set to {:?}", mode);
                }
                OwnStackRpc::ToolExec { command, tool_name } => {
                    info!(
                        "Executing requested tool: {} with args: {}",
                        tool_name, command
                    );
                    let args = if tool_name == "exec" || tool_name == "core:exec" {
                        serde_json::json!({ "command": command })
                    } else {
                        serde_json::from_str(&command)
                            .unwrap_or(serde_json::json!({}))
                    };

                    let result = orchestrator.execute_tool(&tool_name, args).await;
                    let json_result =
                        serde_json::to_string(&result).unwrap_or_default();

                    send_rpc_notification(OwnStackRpc::ToolResultMsg {
                        json_result,
                    });
                    send_ui_delta(UiStateDelta {
                        mode: None,
                        run_state: None,
                        budget: None,
                        context: None,
                        mission: None,
                        pending_approval: None,
                        tool_event: Some(ToolEventSnapshot {
                            tool_name,
                            status: if result.success {
                                "completed".to_string()
                            } else {
                                "failed".to_string()
                            },
                            summary: if result.success {
                                Some(result.stdout.clone())
                            } else {
                                Some(result.stderr.clone())
                            },
                            duration_ms: None,
                        }),
                        alert: None,
                    });
                    send_budget_context_updates(
                        orchestrator.budget_snapshot(),
                        orchestrator.context_snapshot(),
                    );
                }
                OwnStackRpc::SuggestionDecision {
                    decision,
                    message_id,
                } => {
                    info!(
                        "Suggestion decision received: {} ({})",
                        decision, message_id
                    );
                }
                OwnStackRpc::UiSnapshotRequest => {
                    // Forward to UI through proxy so lapce-app can emit UiSnapshot metadata.
                    send_rpc_notification(OwnStackRpc::UiSnapshotRequest);
                }
                OwnStackRpc::UiSnapshot { metadata } => {
                    let ownstack_dir = workspace.join(".ownstack");
                    let snapshot_path = ownstack_dir.join("ui_snapshot.json");
                    if let Err(err) = std::fs::create_dir_all(&ownstack_dir) {
                        warn!(
                            "Failed to create .ownstack directory for UI snapshot: {}",
                            err
                        );
                    } else if let Err(err) =
                        std::fs::write(&snapshot_path, metadata.as_bytes())
                    {
                        warn!(
                            "Failed to persist UI snapshot metadata at {:?}: {}",
                            snapshot_path, err
                        );
                    } else {
                        info!(
                            "UI snapshot metadata received and stored at {:?}",
                            snapshot_path
                        );
                    }
                }
                OwnStackRpc::CaptureScreenshot => {
                    let ownstack_dir = workspace.join(".ownstack");
                    let screenshot_path = ownstack_dir.join("ui_screenshot.png");
                    let response = match std::fs::create_dir_all(&ownstack_dir) {
                        Ok(()) => {
                            match ownstack_engine::vision::capture_active_window(
                                &screenshot_path,
                            ) {
                                Ok(()) => serde_json::json!({
                                    "success": true,
                                    "path": screenshot_path.to_string_lossy(),
                                }),
                                Err(err) => serde_json::json!({
                                    "success": false,
                                    "error": err,
                                    "path": screenshot_path.to_string_lossy(),
                                }),
                            }
                        }
                        Err(err) => serde_json::json!({
                            "success": false,
                            "error": format!("Failed to create .ownstack directory: {}", err),
                            "path": screenshot_path.to_string_lossy(),
                        }),
                    };

                    send_rpc_notification(OwnStackRpc::ToolResultMsg {
                        json_result: response.to_string(),
                    });
                }
                _ => {
                    debug!("Unhandled RPC (work queue): {:?}", rpc);
                }
            }
        }
        Ok(())
    }
}
