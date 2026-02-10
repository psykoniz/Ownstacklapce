use std::process::Command;
use std::path::Path;
use std::time::Duration;
use crate::tool_result::ToolResult;
use crate::sandbox::{Sandbox, SandboxLevel};

pub struct ProcessSandbox;

impl Sandbox for ProcessSandbox {
    fn exec(
        &self,
        command_str: &str,
        cwd: &Path,
        level: SandboxLevel,
    ) -> ToolResult {
        let parts: Vec<&str> = command_str.split_whitespace().collect();
        if parts.is_empty() {
            return ToolResult::failure("Empty command".to_string(), None);
        }

        let cmd_name = parts[0];
        let args = &parts[1..];

        let mut child = Command::new(cmd_name);
        child.args(args)
            .current_dir(cwd)
            .env_clear() // Critical security step
            .env("PATH", "/usr/bin:/bin:/usr/local/bin") // Minimal path
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Level-specific configurations
        let _timeout = match level {
            SandboxLevel::Light => Duration::from_secs(60),
            SandboxLevel::Standard => Duration::from_secs(300),
            SandboxLevel::Strict => Duration::from_secs(600),
        };
        // TODO: Implement actual process timeout using wait-timeout crate or similar mechanism


        match child.spawn() {
            Ok(child_proc) => {
                // In a real implementation, we would use wait_timeout here.
                // For this version, we'll perform a standard wait or use a thread for timeout.
                match child_proc.wait_with_output() {
                    Ok(output) => ToolResult {
                        success: output.status.success(),
                        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                        exit_code: output.status.code(),
                        metadata: std::collections::HashMap::new(),
                    },
                    Err(e) => ToolResult::failure(format!("Execution error: {}", e), None),
                }
            },
            Err(e) => ToolResult::failure(format!("Failed to spawn process: {}", e), None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn cwd() -> std::path::PathBuf {
        env::current_dir().unwrap()
    }

    // ─── Empty Command ──────────────────────────────────────────
    #[test]
    fn test_empty_command() {
        let sandbox = ProcessSandbox;
        let result = sandbox.exec("", &cwd(), SandboxLevel::Light);
        assert!(!result.success);
        assert!(result.stderr.contains("Empty command"));
    }

    #[test]
    fn test_whitespace_only_command() {
        let sandbox = ProcessSandbox;
        let result = sandbox.exec("   ", &cwd(), SandboxLevel::Light);
        assert!(!result.success);
    }

    // ─── Known Commands ─────────────────────────────────────────
    #[cfg(windows)]
    #[test]
    fn test_exec_echo_windows() {
        let sandbox = ProcessSandbox;
        let result = sandbox.exec("cmd /c echo hello", &cwd(), SandboxLevel::Light);
        // May or may not work depending on PATH; we test it doesn't crash
        let _ = result;
    }

    #[cfg(unix)]
    #[test]
    fn test_exec_echo() {
        let sandbox = ProcessSandbox;
        let result = sandbox.exec("echo hello", &cwd(), SandboxLevel::Light);
        assert!(result.success);
        assert!(result.stdout.contains("hello"));
    }

    #[cfg(unix)]
    #[test]
    fn test_exec_false() {
        let sandbox = ProcessSandbox;
        let result = sandbox.exec("false", &cwd(), SandboxLevel::Light);
        assert!(!result.success);
    }

    // ─── Unknown Commands ───────────────────────────────────────
    #[test]
    fn test_nonexistent_command() {
        let sandbox = ProcessSandbox;
        let result = sandbox.exec("nonexistent_command_xyz_12345", &cwd(), SandboxLevel::Light);
        assert!(!result.success);
        assert!(result.stderr.contains("spawn") || result.stderr.contains("Failed"));
    }

    // ─── All Sandbox Levels ─────────────────────────────────────
    #[test]
    fn test_all_sandbox_levels() {
        let sandbox = ProcessSandbox;
        for level in [SandboxLevel::Light, SandboxLevel::Standard, SandboxLevel::Strict] {
            // Should not panic at any level
            let result = sandbox.exec("nonexistent_xyz", &cwd(), level);
            assert!(!result.success);
        }
    }

    // ─── Struct Tests ───────────────────────────────────────────
    #[test]
    fn test_process_sandbox_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<ProcessSandbox>();
    }

    #[test]
    fn test_process_sandbox_is_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<ProcessSandbox>();
    }

    // ─── Command Parsing ────────────────────────────────────────
    #[test]
    fn test_command_with_multiple_args() {
        let sandbox = ProcessSandbox;
        // Even if command doesn't exist, parsing shouldn't panic
        let result = sandbox.exec("cmd arg1 arg2 arg3 arg4 arg5", &cwd(), SandboxLevel::Light);
        let _ = result;
    }

    #[test]
    fn test_command_with_special_chars() {
        let sandbox = ProcessSandbox;
        // Shouldn't panic
        let result = sandbox.exec("echo 'hello world'", &cwd(), SandboxLevel::Light);
        let _ = result;
    }

    // ─── Stress Tests ───────────────────────────────────────────
    #[test]
    fn stress_test_rapid_error_commands() {
        let sandbox = ProcessSandbox;
        for i in 0..100 {
            let cmd = format!("nonexistent_cmd_{}", i);
            let result = sandbox.exec(&cmd, &cwd(), SandboxLevel::Light);
            assert!(!result.success);
        }
    }

    #[test]
    fn stress_test_concurrent_sandbox() {
        use std::thread;
        let handles: Vec<_> = (0..20).map(|i| {
            thread::spawn(move || {
                let sandbox = ProcessSandbox;
                for j in 0..10 {
                    let cmd = format!("nonexistent_{}_{}", i, j);
                    let result = sandbox.exec(&cmd, &cwd(), SandboxLevel::Light);
                    assert!(!result.success);
                }
            })
        }).collect();
        for h in handles {
            h.join().unwrap();
        }
    }
}
