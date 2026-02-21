use ownstack_agent::orchestrator::AgentOrchestrator;
use ownstack_agent::provider::{LlmMessage, LlmProvider, ToolCall, ToolDefinition};
use ownstack_agent::providers::openrouter::OpenRouterProvider;
use ownstack_agent::toolkits::{CoreToolkit, Toolkit};
use std::sync::Arc;

fn to_tool_definitions(toolkit: &dyn Toolkit) -> Vec<ToolDefinition> {
    toolkit
        .tools()
        .into_iter()
        .map(|tool| ToolDefinition {
            name: tool.name,
            description: tool.description,
            parameters: tool.parameters,
        })
        .collect()
}

fn add_assistant_tool_calls(
    messages: &mut Vec<LlmMessage>,
    tool_calls: Vec<ToolCall>,
) {
    let mut assistant = LlmMessage::assistant("");
    assistant.tool_calls = Some(tool_calls);
    messages.push(assistant);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("OPENROUTER_MODEL").is_err() {
        std::env::set_var("OPENROUTER_MODEL", "openai/gpt-4o-mini");
    }
    let model = std::env::var("OPENROUTER_MODEL")
        .unwrap_or_else(|_| "openai/gpt-4o-mini".to_string());

    let provider: Arc<dyn LlmProvider> = Arc::new(OpenRouterProvider::from_env()?);
    let workspace = std::env::current_dir()?;
    let session_id = format!("example-session-{}", std::process::id());

    println!("provider=openrouter");
    println!("model={model}");

    let core_toolkit =
        Arc::new(CoreToolkit::new(workspace.clone(), session_id, None));
    let tool_defs = to_tool_definitions(core_toolkit.as_ref());

    let mut orchestrator =
        AgentOrchestrator::new(provider.clone(), workspace.clone(), 64_000);
    orchestrator.register_toolkit(core_toolkit);

    let mut messages = vec![
        LlmMessage::system(
            "You can call tools. If the user asks for files, call the search tool.",
        ),
        LlmMessage::user(
            "Liste les fichiers .rs du dossier courant avec l'outil le plus adapte.",
        ),
    ];

    for round in 1..=4 {
        println!("round={round}");
        let response = provider
            .complete(messages.clone(), Some(tool_defs.clone()), None)
            .await?;

        if response.tool_calls.is_empty() {
            let final_text = response.content.unwrap_or_default();
            if final_text.trim().is_empty() {
                return Err("final response is empty and no tool was called".into());
            }
            println!("final_response={final_text}");
            return Ok(());
        }

        println!("tool_calls={}", response.tool_calls.len());
        add_assistant_tool_calls(&mut messages, response.tool_calls.clone());

        for tool_call in response.tool_calls {
            println!("tool_name={}", tool_call.name);
            println!("tool_args={}", tool_call.arguments);
            let tool_result = orchestrator
                .execute_tool(&tool_call.name, tool_call.arguments)
                .await;
            println!("tool_success={}", tool_result.success);
            println!("tool_stdout={}", tool_result.stdout);
            println!("tool_stderr={}", tool_result.stderr);

            messages.push(LlmMessage::tool_result(
                tool_call.id,
                serde_json::to_string(&tool_result)?,
            ));
        }
    }

    Err("max rounds reached without a final assistant answer".into())
}
