use ed25519_dalek::{Signer, SigningKey};
use ownstack_agent::plugins::WasiPluginHost;
use serde_json::json;
use std::path::Path;
use tempfile::tempdir;

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn sign_plugin(path: &Path) {
    let mut seed = [0u8; 32];
    seed[0] = 42;
    let signing_key = SigningKey::from_bytes(&seed);
    let plugin_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("plugin stem");
    let signature = signing_key.sign(plugin_name.as_bytes());

    std::fs::write(path.with_extension("wasm.sig"), signature.to_bytes())
        .expect("write signature");
    std::env::set_var(
        "OWNSTACK_PLUGIN_TRUSTED_PUBLIC_KEY_HEX",
        to_hex(&signing_key.verifying_key().to_bytes()),
    );
}

fn wasm_with_stdout_json(json_payload: &str) -> Vec<u8> {
    let escaped = json_payload.replace('\\', "\\\\").replace('"', "\\\"");
    let payload_len = json_payload.as_bytes().len();

    let wat = format!(
        r#"(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 8) "{escaped}")
  (func (export "run") (result i32)
    (i32.store (i32.const 0) (i32.const 8))
    (i32.store (i32.const 4) (i32.const {payload_len}))
    (call $fd_write
      (i32.const 1)
      (i32.const 0)
      (i32.const 1)
      (i32.const 64))
    drop
    (i32.const 0))
)"#
    );

    wat::parse_str(wat).expect("WAT must compile")
}

#[tokio::test]
async fn wasi_plugin_roundtrip_success() {
    let workspace = tempdir().expect("tempdir");
    let wasm_path = workspace.path().join("hello_world.wasm");
    let wasm = wasm_with_stdout_json(r#"{"success":true,"output":"Hello, Alice!"}"#);
    tokio::fs::write(&wasm_path, wasm)
        .await
        .expect("write wasm");
    sign_plugin(&wasm_path);

    let host = WasiPluginHost::new(workspace.path().to_path_buf());
    let toolkit = host.load_plugin(&wasm_path).await.expect("load plugin");

    let result = toolkit
        .execute("hello_world", json!({"name":"Alice"}))
        .await
        .expect("execute plugin");

    assert!(result.success);
    assert_eq!(result.stdout, "Hello, Alice!");
}

#[tokio::test]
async fn wasi_plugin_roundtrip_failure_payload() {
    let workspace = tempdir().expect("tempdir");
    let wasm_path = workspace.path().join("failing_tool.wasm");
    let wasm =
        wasm_with_stdout_json(r#"{"success":false,"output":"","error":"boom"}"#);
    tokio::fs::write(&wasm_path, wasm)
        .await
        .expect("write wasm");
    sign_plugin(&wasm_path);

    let host = WasiPluginHost::new(workspace.path().to_path_buf());
    let toolkit = host.load_plugin(&wasm_path).await.expect("load plugin");

    let result = toolkit
        .execute("failing_tool", json!({}))
        .await
        .expect("execute plugin");

    assert!(!result.success);
    assert!(result.stderr.contains("boom"));
}
