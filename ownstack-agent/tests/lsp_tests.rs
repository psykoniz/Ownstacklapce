use ownstack_agent::lsp::LspClient;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::sleep;
use url::Url;

#[tokio::test]
async fn test_lsp_client_integration() {
    // Locate the mock server script
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mock_server_path = manifest_dir.join("tests").join("mock_lsp.py");

    assert!(mock_server_path.exists(), "Mock server script not found at {:?}", mock_server_path);

    // Initial check for python
    let python_cmd = if cfg!(windows) { "python" } else { "python3" };

    // Start client
    let client = LspClient::start(python_cmd, &[mock_server_path.to_string_lossy().to_string()])
        .await
        .expect("Failed to start LSP client");

    // Initialize
    let root_uri = Url::from_directory_path(&manifest_dir).unwrap();
    let init_result = client.initialize(root_uri.clone()).await.expect("Failed to initialize");

    // Check capabilities
    let caps = init_result.capabilities;
    use lsp_types::HoverProviderCapability;
    match caps.hover_provider.unwrap() {
        HoverProviderCapability::Simple(true) => {},
        HoverProviderCapability::Options(_) => {},
        _ => panic!("Unexpected hover provider capability"),
    }

    // Check diagnostics notification (mock server sends one after initialized)
    // Wait a bit for notification to arrive
    sleep(Duration::from_millis(500)).await;

    let diag_uri = Url::parse("file:///workspace/test.rs").unwrap();
    let diags = client.get_diagnostics(&diag_uri).await;
    
    assert!(diags.is_some(), "Should have received diagnostics");
    let diags_vec = diags.unwrap();
    assert_eq!(diags_vec.len(), 1);
    assert_eq!(diags_vec[0].message, "Mock diagnostic error");

    // Test Hover
    let hover = client.hover(diag_uri, 0, 0).await.expect("Hover failed");
    assert!(hover.is_some());
    // Checking contents field of Hover is tricky because it's MarkedString/MarkupContent enum
    // But we know mock server sends { "contents": "Hover content..." }
    // serde_json handling inside LspClient should have parsed it.
}
