use ownstack_bridge::{BridgeLaunchConfig, BridgeRuntimeMode, PythonBridge};
use serde_json::json;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut workspace = std::env::current_dir()?;
    // If running from ownstack-bridge/, move up to repo root
    if workspace.ends_with("ownstack-bridge") {
        workspace.pop();
    }
    println!("Simulating traffic in workspace: {:?}", workspace);

    let config = BridgeLaunchConfig {
        mode: BridgeRuntimeMode::Auto,
        workspace: workspace.clone(),
        python_root: Some(workspace.join("ownstack-python")),
        bundled_path: None,
    };

    let mut bridge = PythonBridge::start(config).await?;
    println!("Bridge started successfully.");

    let scenarios = vec![
        ("tools.exec", json!({"command": "ls -la"})),
        ("tools.exec", json!({"command": "grep -r 'todo' ."})),
        ("git.status", json!({})),
        (
            "lsp.hover",
            json!({"file": "src/lib.rs", "line": 10, "character": 5}),
        ),
        ("agent.plan", json!({"task": "implement new feature"})),
        ("fail", json!({"trigger": "error"})),
    ];

    for (method, params) in scenarios {
        println!("Sending request: {}...", method);
        match bridge.send_request(method, params).await {
            Ok(res) => println!("  Success: {}", res),
            Err(e) => println!("  Error: {}", e),
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    bridge.shutdown().await?;
    println!("Bridge shutdown. Metrics should be saved to .ownstack/python_bridge_metrics.jsonl");

    Ok(())
}
