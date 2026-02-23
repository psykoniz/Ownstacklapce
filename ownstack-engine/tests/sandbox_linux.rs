#![cfg(target_os = "linux")]

use ownstack_engine::{ProcessSandbox, Sandbox, SandboxLevel};
use std::path::PathBuf;

fn cwd() -> PathBuf {
    std::env::current_dir().expect("current_dir")
}

#[tokio::test]
async fn linux_light_exec_echo() {
    let sandbox = ProcessSandbox;
    let result = sandbox
        .exec("echo hello", &cwd(), SandboxLevel::Light)
        .await;
    assert!(result.success, "stderr={}", result.stderr);
    assert!(result.stdout.contains("hello"));
}

#[tokio::test]
async fn linux_strict_exec_returns_result() {
    let sandbox = ProcessSandbox;
    let result = sandbox
        .exec("echo strict", &cwd(), SandboxLevel::Strict)
        .await;

    // Depending on host capabilities, strict mode can either:
    // - succeed (namespace/sandbox available), or
    // - fail fast with a non-empty error when strict wrapping is unavailable.
    if result.success {
        assert!(result.stdout.contains("strict"));
    } else {
        assert!(
            !result.stderr.trim().is_empty(),
            "strict mode failure should provide stderr"
        );
    }
}

#[tokio::test]
async fn linux_timeout_path_still_works() {
    let sandbox = ProcessSandbox;
    let result = sandbox.exec("sleep 1", &cwd(), SandboxLevel::Light).await;
    assert!(result.success, "stderr={}", result.stderr);
}
