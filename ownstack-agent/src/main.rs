use lapce_rpc::ownstack::OwnStackRpc;
use ownstack_agent::orchestrator::{AgentBudget, AgentOrchestrator};
use ownstack_agent::policy_approval::{PolicyApprovalManager, RpcSink};
use ownstack_agent::provider::LlmProvider;
use ownstack_agent::providers::anthropic::AnthropicProvider;
use ownstack_agent::providers::local::LocalProvider;
use ownstack_agent::providers::openrouter::OpenRouterProvider;
use ownstack_engine::{PolicyEngine, ProcessSandbox};
use serde::Deserialize;
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

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

    // Initialize provider based on env
    let provider: Arc<dyn LlmProvider> =
        if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            info!("LLM Provider: Anthropic");
            Arc::new(AnthropicProvider::from_env()?)
        } else if std::env::var("OPENROUTER_API_KEY").is_ok() {
            info!("LLM Provider: OpenRouter");
            Arc::new(OpenRouterProvider::from_env()?)
        } else {
            info!("LLM Provider: Local");
            Arc::new(LocalProvider::from_env()?)
        };

    let mut orchestrator =
        AgentOrchestrator::new(provider.clone(), workspace.clone(), 128000);

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
        info!("No budgets file found at {:?} (using defaults)", budgets_path);
    }

    let rpc_sink: RpcSink = Arc::new(send_rpc_notification);
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
    let healer_toolkit = Arc::new(
        ownstack_agent::toolkits::healer::HealerToolkit::new(workspace.clone()),
    );
    let multivers_toolkit =
        Arc::new(ownstack_agent::toolkits::multivers::MultiversToolkit::new(
            workspace.clone(),
        ));

    // Register default toolkits
    orchestrator.register_toolkit(core_toolkit.clone());
    orchestrator.register_toolkit(git_toolkit.clone());
    orchestrator.register_toolkit(lsp_toolkit.clone());
    orchestrator.register_toolkit(healer_toolkit.clone());
    orchestrator.register_toolkit(multivers_toolkit.clone());

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
                                OwnStackRpc::PolicyResponse { approved } => {
                                    if let Some(mgr) = approval_for_reader.as_ref() {
                                        mgr.resolve(approved).await;
                                    }
                                }
                                OwnStackRpc::AiPrompt { .. }
                                | OwnStackRpc::ToolExec { .. } => {
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
                            },
                            |mission| {
                                let mission_rpc = OwnStackRpc::MissionUpdate {
                                    goal: mission.goal,
                                    steps: mission
                                        .steps
                                        .into_iter()
                                        .map(|s| {
                                            (
                                                s.description,
                                                format!("{:?}", s.status),
                                            )
                                        })
                                        .collect(),
                                };
                                send_rpc_notification(mission_rpc);
                            },
                        )
                        .await;

                    if let Err(e) = result {
                        error!("Process error: {}", e);
                    }
                }
                OwnStackRpc::ToolExec { command, tool_name } => {
                    info!(
                        "Executing requested tool: {} with args: {}",
                        tool_name, command
                    );
                    let args = if tool_name == "exec" {
                        serde_json::json!({ "command": command })
                    } else {
                        serde_json::from_str(&command)
                            .unwrap_or(serde_json::json!({}))
                    };

                    let result = orchestrator.execute_tool(&tool_name, args).await;
                    let json_result =
                        serde_json::to_string(&result).unwrap_or_default();

                    send_rpc_notification(OwnStackRpc::ToolResultMsg { json_result });
                }
                _ => {
                    debug!("Unhandled RPC (work queue): {:?}", rpc);
                }
            }
        }
        Ok(())
    }
}
