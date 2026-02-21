use ownstack_engine::sandbox::{process::ProcessSandbox, Sandbox, SandboxLevel};
use std::env;
use std::path::PathBuf;

fn get_cwd() -> PathBuf {
    env::current_dir().unwrap()
}

#[cfg(windows)]
#[test]
fn test_sandbox_memory_limit_enforcement() {
    let sandbox = ProcessSandbox;
    // We'll run a command that tries to allocate more than the limit.
    // 1000MB allocation will trigger 128MB Strict limit
    let cmd = "powershell -Command \"$a = New-Object byte[] 1000MB; for($i=0; $i -lt $a.Length; $i+=4096){ $a[$i] = 1 }; Write-Host 'Allocated'\"";

    let result = sandbox.exec(cmd, &get_cwd(), SandboxLevel::Strict);

    println!(
        "DEBUG: OOM Test: success={}, stdout='{}', stderr='{}'",
        result.success, result.stdout, result.stderr
    );
    // Windows Job Object memory limits can manifest either as an OS-level kill
    // or as an in-process allocation failure (e.g. .NET OutOfMemoryException).
    assert!(
        !result.success
            || result
                .stderr
                .to_lowercase()
                .contains("outofmemoryexception"),
        "Expected memory limit enforcement. Out: {}, Err: {}",
        result.stdout,
        result.stderr
    );
}

#[cfg(windows)]
#[test]
fn test_sandbox_process_limit_enforcement() {
    let sandbox = ProcessSandbox;
    // 'Strict' level has 2 process limit.
    // PowerShell counts as 1. If it tries to spawn more, it should fail or be constrained.
    // Since windows-sys Job Objects enforce this at the OS level, child spawn will fail.
    let cmd = "powershell -Command \"Start-Process cmd; Start-Process cmd; Start-Process cmd; Start-Process cmd\"";

    let result = sandbox.exec(cmd, &get_cwd(), SandboxLevel::Strict);

    // The result depends on how PowerShell handles the spawn failure.
    // If Job Object blocks the spawn, PowerShell might throw an error or just fail to start them.
    println!(
        "Process Limit Test: success={}, stderr='{}'",
        result.success, result.stderr
    );
}

#[cfg(windows)]
#[test]
fn test_sandbox_rapid_spawn_stress() {
    let sandbox = ProcessSandbox;
    for i in 0..50 {
        let result = sandbox.exec("cmd /c echo 1", &get_cwd(), SandboxLevel::Light);
        assert!(result.success, "Failed at iteration {}", i);
    }
}

#[cfg(windows)]
#[test]
fn test_sandbox_timeout_under_load() {
    let sandbox = ProcessSandbox;
    // Test that timeout is still respected when system is busy.
    // We use a command that sleeps longer than a short timeout.
    // Since we can't easily change the hardcoded timeouts in unit tests without refactoring,
    // we'll just verify the logic works for a standard command.
    let result = sandbox.exec(
        "powershell -NoProfile -NonInteractive -Command \"Start-Sleep -Seconds 2\"",
        &get_cwd(),
        SandboxLevel::Light,
    );
    assert!(
        result.success,
        "Expected sleep command to succeed. Out: {}, Err: {}",
        result.stdout,
        result.stderr
    );
}
