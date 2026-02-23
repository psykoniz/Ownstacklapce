use crate::sandbox::{Sandbox, SandboxLevel};
use crate::tool_result::ToolResult;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tracing::debug;
use wait_timeout::ChildExt;

#[cfg(windows)]
mod windows_job {
    use std::mem;
    use std::os::windows::io::AsRawHandle;
    use std::ptr;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::JobObjects::*;
    use windows_sys::Win32::System::Threading::*;

    pub struct WindowsJob {
        handle: HANDLE,
    }

    impl WindowsJob {
        pub fn new() -> std::io::Result<Self> {
            let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if handle == 0 {
                return Err(std::io::Error::last_os_error());
            }

            // Set basic limits: kill children on close
            unsafe {
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
                info.BasicLimitInformation.LimitFlags =
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

                let res = SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );

                if res == 0 {
                    let err = std::io::Error::last_os_error();
                    CloseHandle(handle);
                    return Err(err);
                }
            }

            Ok(Self { handle })
        }

        pub fn assign_process(
            &self,
            process: &std::process::Child,
        ) -> std::io::Result<()> {
            let res = unsafe {
                AssignProcessToJobObject(
                    self.handle,
                    process.as_raw_handle() as HANDLE,
                )
            };
            if res == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

        pub fn set_limits(
            &self,
            level: crate::sandbox::SandboxLevel,
        ) -> std::io::Result<()> {
            unsafe {
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
                info.BasicLimitInformation.LimitFlags =
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                        | JOB_OBJECT_LIMIT_PROCESS_MEMORY
                        | JOB_OBJECT_LIMIT_JOB_MEMORY
                        | JOB_OBJECT_LIMIT_PRIORITY_CLASS;
                info.BasicLimitInformation.PriorityClass =
                    BELOW_NORMAL_PRIORITY_CLASS;

                let cpu_rate: u32;

                match level {
                    crate::sandbox::SandboxLevel::Light => {
                        info.BasicLimitInformation.ActiveProcessLimit = 10;
                        info.ProcessMemoryLimit = 512 * 1024 * 1024; // 512MB
                        info.JobMemoryLimit = 512 * 1024 * 1024; // 512MB
                        cpu_rate = 5000; // 50%
                    }
                    crate::sandbox::SandboxLevel::Standard => {
                        info.BasicLimitInformation.ActiveProcessLimit = 5;
                        info.ProcessMemoryLimit = 256 * 1024 * 1024; // 256MB
                        info.JobMemoryLimit = 256 * 1024 * 1024; // 256MB
                        cpu_rate = 3000; // 30%
                    }
                    crate::sandbox::SandboxLevel::Strict => {
                        info.BasicLimitInformation.ActiveProcessLimit = 2;
                        info.ProcessMemoryLimit = 128 * 1024 * 1024; // 128MB
                        info.JobMemoryLimit = 128 * 1024 * 1024; // 128MB
                        cpu_rate = 1500; // 15%
                    }
                }

                let res_ext = SetInformationJobObject(
                    self.handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );

                if res_ext == 0 {
                    return Err(std::io::Error::last_os_error());
                }

                // Apply a hard CPU cap.
                let mut cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = mem::zeroed();
                cpu.ControlFlags = JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                    | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
                cpu.Anonymous.CpuRate = cpu_rate;
                let res_cpu = SetInformationJobObject(
                    self.handle,
                    JobObjectCpuRateControlInformation,
                    &cpu as *const _ as *const _,
                    mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                );
                if res_cpu == 0 {
                    return Err(std::io::Error::last_os_error());
                }

                // Restrict UI/system interaction privileges for sandboxed processes.
                let mut ui: JOBOBJECT_BASIC_UI_RESTRICTIONS = mem::zeroed();
                ui.UIRestrictionsClass = JOB_OBJECT_UILIMIT_DESKTOP
                    | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                    | JOB_OBJECT_UILIMIT_EXITWINDOWS
                    | JOB_OBJECT_UILIMIT_GLOBALATOMS
                    | JOB_OBJECT_UILIMIT_HANDLES
                    | JOB_OBJECT_UILIMIT_READCLIPBOARD
                    | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
                    | JOB_OBJECT_UILIMIT_WRITECLIPBOARD;
                let res_ui = SetInformationJobObject(
                    self.handle,
                    JobObjectBasicUIRestrictions,
                    &ui as *const _ as *const _,
                    mem::size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
                );
                if res_ui == 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        }
    }

    impl Drop for WindowsJob {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}

pub struct ProcessSandbox;

fn split_command(command_str: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = command_str.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
            }
            '\\' => {
                // Basic escaping for embedded quotes: \" => "
                if let Some('"') = chars.peek().copied() {
                    chars.next();
                    current.push('"');
                } else {
                    current.push('\\');
                }
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        parts.push(current);
    }

    parts
}

fn spawn_pipe_reader<R>(
    pipe: Option<R>,
) -> Option<std::thread::JoinHandle<std::io::Result<Vec<u8>>>>
where
    R: Read + Send + 'static,
{
    pipe.map(|mut stream| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf)?;
            Ok(buf)
        })
    })
}

fn collect_pipe_output(
    reader: Option<std::thread::JoinHandle<std::io::Result<Vec<u8>>>>,
    stream_name: &str,
) -> (Vec<u8>, Option<String>) {
    let Some(reader) = reader else {
        return (Vec::new(), None);
    };

    match reader.join() {
        Ok(Ok(bytes)) => (bytes, None),
        Ok(Err(e)) => (
            Vec::new(),
            Some(format!("Failed to read {}: {}", stream_name, e)),
        ),
        Err(_) => (
            Vec::new(),
            Some(format!("{} reader thread panicked", stream_name)),
        ),
    }
}

fn append_read_error(stderr: &mut String, read_error: Option<String>) {
    if let Some(err) = read_error {
        if !stderr.is_empty() {
            stderr.push('\n');
        }
        stderr.push_str(&err);
    }
}

#[async_trait::async_trait]
impl Sandbox for ProcessSandbox {
    async fn exec(
        &self,
        command_str: &str,
        cwd: &Path,
        level: SandboxLevel,
    ) -> ToolResult {
        let command_str = command_str.trim();
        if command_str.is_empty() {
            return ToolResult::failure("Empty command".to_string(), None);
        }

        let parts = split_command(command_str);
        if parts.is_empty() {
            return ToolResult::failure("Empty command".to_string(), None);
        }

        #[allow(unused_mut)]
        let mut cmd_name = parts[0].clone();
        #[allow(unused_mut)]
        let mut args: Vec<String> = parts[1..].to_vec();

        #[cfg(target_os = "linux")]
        {
            let resolved =
                super::linux::resolve_command(&cmd_name, &args, command_str, level);
            cmd_name = resolved.0;
            args = resolved.1;
        }

        #[cfg(target_os = "macos")]
        {
            let resolved = super::macos::resolve_command(
                &cmd_name,
                &args,
                command_str,
                cwd,
                level,
            );
            cmd_name = resolved.0;
            args = resolved.1;
        }

        let mut child_cmd = Command::new(&cmd_name);
        child_cmd.args(&args);

        child_cmd
            .current_dir(cwd)
            .env_clear() // Critical security step
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        #[cfg(windows)]
        {
            let mut path = "C:\\Windows\\System32;C:\\Windows".to_string();
            path =
                format!("{};C:\\Windows\\System32\\WindowsPowerShell\\v1.0", path);
            // Add common Git locations on Windows
            path = format!(
                "{};C:\\Program Files\\Git\\cmd;C:\\Program Files\\Git\\bin",
                path
            );
            child_cmd.env("PATH", path);

            if let Ok(root) = std::env::var("SystemRoot") {
                child_cmd.env("SystemRoot", root);
            }
            if let Ok(windir) = std::env::var("windir") {
                child_cmd.env("windir", windir);
            }
            if let Ok(temp) = std::env::var("TEMP") {
                child_cmd.env("TEMP", temp);
            }
            if let Ok(comspec) = std::env::var("ComSpec") {
                child_cmd.env("ComSpec", comspec);
            }
            if let Ok(profile) = std::env::var("USERPROFILE") {
                child_cmd.env("USERPROFILE", profile);
            }
            if let Ok(appdata) = std::env::var("APPDATA") {
                child_cmd.env("APPDATA", appdata);
            }
            if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
                child_cmd.env("LOCALAPPDATA", localappdata);
            }
            if let Ok(homedrive) = std::env::var("HOMEDRIVE") {
                child_cmd.env("HOMEDRIVE", homedrive);
            }
            if let Ok(homepath) = std::env::var("HOMEPATH") {
                child_cmd.env("HOMEPATH", homepath);
            }
        }

        #[cfg(unix)]
        child_cmd.env("PATH", "/usr/bin:/bin:/usr/local/bin");

        let timeout = match level {
            SandboxLevel::Light => Duration::from_secs(60),
            SandboxLevel::Standard => Duration::from_secs(300),
            SandboxLevel::Strict => Duration::from_secs(600),
        };

        #[cfg(windows)]
        let job = match windows_job::WindowsJob::new() {
            Ok(job) => {
                if let Err(e) = job.set_limits(level) {
                    return ToolResult::failure(
                        format!("Failed to set sandbox limits: {}", e),
                        None,
                    );
                }
                job
            }
            Err(e) => {
                return ToolResult::failure(
                    format!("Failed to create Windows Job Object: {}", e),
                    None,
                );
            }
        };

        match child_cmd.spawn() {
            Ok(mut child_proc) => {
                #[cfg(windows)]
                if let Err(e) = job.assign_process(&child_proc) {
                    let _ = child_proc.kill();
                    return ToolResult::failure(
                        format!(
                            "Failed to assign process to Windows Job Object: {}",
                            e
                        ),
                        None,
                    );
                }

                let mut stdout_reader = spawn_pipe_reader(child_proc.stdout.take());
                let mut stderr_reader = spawn_pipe_reader(child_proc.stderr.take());

                match child_proc.wait_timeout(timeout) {
                    Ok(Some(status)) => {
                        let (output, stdout_read_error) =
                            collect_pipe_output(stdout_reader.take(), "stdout");
                        let (err_output, stderr_read_error) =
                            collect_pipe_output(stderr_reader.take(), "stderr");

                        let mut stderr =
                            String::from_utf8_lossy(&err_output).to_string();
                        append_read_error(&mut stderr, stdout_read_error.clone());
                        append_read_error(&mut stderr, stderr_read_error.clone());
                        let read_ok = stdout_read_error.is_none()
                            && stderr_read_error.is_none();

                        if !output.is_empty() || !err_output.is_empty() {
                            debug!(
                                command = %command_str,
                                stdout_bytes = output.len(),
                                stderr_bytes = err_output.len(),
                                "Sandbox captured child output"
                            );
                        } else {
                            debug!(
                                command = %command_str,
                                "Sandbox child finished with empty stdout/stderr"
                            );
                        }

                        ToolResult {
                            success: status.success() && read_ok,
                            stdout: String::from_utf8_lossy(&output).to_string(),
                            stderr,
                            exit_code: status.code(),
                            metadata: std::collections::HashMap::new(),
                        }
                    }
                    Ok(None) => {
                        let _ = child_proc.kill();
                        let _ = child_proc.wait();
                        let (_, stdout_read_error) =
                            collect_pipe_output(stdout_reader.take(), "stdout");
                        let (_, stderr_read_error) =
                            collect_pipe_output(stderr_reader.take(), "stderr");

                        let mut stderr =
                            format!("Process timed out after {:?}", timeout);
                        append_read_error(&mut stderr, stdout_read_error);
                        append_read_error(&mut stderr, stderr_read_error);
                        ToolResult::failure(stderr, None)
                    }
                    Err(e) => {
                        let _ = child_proc.kill();
                        let _ = child_proc.wait();
                        let (_, stdout_read_error) =
                            collect_pipe_output(stdout_reader.take(), "stdout");
                        let (_, stderr_read_error) =
                            collect_pipe_output(stderr_reader.take(), "stderr");

                        let mut stderr = format!("Execution error: {}", e);
                        append_read_error(&mut stderr, stdout_read_error);
                        append_read_error(&mut stderr, stderr_read_error);
                        ToolResult::failure(stderr, None)
                    }
                }
            }
            Err(e) => {
                ToolResult::failure(format!("Failed to spawn process: {}", e), None)
            }
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
    #[tokio::test]
    async fn test_empty_command() {
        let sandbox = ProcessSandbox;
        let result = sandbox.exec("", &cwd(), SandboxLevel::Light).await;
        assert!(!result.success);
        assert!(result.stderr.contains("Empty command"));
    }

    #[tokio::test]
    async fn test_whitespace_only_command() {
        let sandbox = ProcessSandbox;
        let result = sandbox.exec("   ", &cwd(), SandboxLevel::Light).await;
        assert!(!result.success);
    }

    // ─── Known Commands ─────────────────────────────────────────
    #[cfg(windows)]
    #[tokio::test]
    async fn test_exec_echo_windows() {
        let sandbox = ProcessSandbox;
        let result = sandbox
            .exec("cmd /c echo hello", &cwd(), SandboxLevel::Light)
            .await;
        assert!(
            result.success,
            "Expected echo command to succeed, stderr: {}",
            result.stderr
        );
        assert!(
            result.stdout.to_lowercase().contains("hello"),
            "Expected sandbox to capture stdout, got: {:?}",
            result.stdout
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn test_exec_set_windows_has_output() {
        let sandbox = ProcessSandbox;
        let result = sandbox
            .exec("cmd /c set", &cwd(), SandboxLevel::Light)
            .await;
        assert!(
            result.success,
            "Expected cmd /c set to succeed, stderr: {}",
            result.stderr
        );
        assert!(
            !result.stdout.trim().is_empty(),
            "Expected environment listing in stdout, got empty output"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_exec_echo() {
        let sandbox = ProcessSandbox;
        let result = sandbox
            .exec("echo hello", &cwd(), SandboxLevel::Light)
            .await;
        assert!(result.success);
        assert!(result.stdout.contains("hello"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_exec_false() {
        let sandbox = ProcessSandbox;
        let result = sandbox.exec("false", &cwd(), SandboxLevel::Light).await;
        assert!(!result.success);
    }

    // ─── Unknown Commands ───────────────────────────────────────
    #[tokio::test]
    async fn test_nonexistent_command() {
        let sandbox = ProcessSandbox;
        let result = sandbox
            .exec("nonexistent_command_xyz_12345", &cwd(), SandboxLevel::Light)
            .await;
        assert!(!result.success);
        assert!(
            !result.stderr.trim().is_empty() || !result.stdout.trim().is_empty(),
            "Expected error output for missing command, got stdout='{}', stderr='{}'",
            result.stdout,
            result.stderr
        );
        if let Some(code) = result.exit_code {
            assert_ne!(code, 0);
        }
    }

    // ─── All Sandbox Levels ─────────────────────────────────────
    #[tokio::test]
    async fn test_all_sandbox_levels() {
        let sandbox = ProcessSandbox;
        for level in [
            SandboxLevel::Light,
            SandboxLevel::Standard,
            SandboxLevel::Strict,
        ] {
            // Should not panic at any level
            let result = sandbox.exec("nonexistent_xyz", &cwd(), level).await;
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
    #[tokio::test]
    async fn test_command_with_multiple_args() {
        let sandbox = ProcessSandbox;
        // Even if command doesn't exist, parsing shouldn't panic
        let result = sandbox
            .exec("cmd arg1 arg2 arg3 arg4 arg5", &cwd(), SandboxLevel::Light)
            .await;
        let _ = result;
    }

    #[tokio::test]
    async fn test_command_with_special_chars() {
        let sandbox = ProcessSandbox;
        // Shouldn't panic
        let result = sandbox
            .exec("echo 'hello world'", &cwd(), SandboxLevel::Light)
            .await;
        let _ = result;
    }

    // ─── Stress Tests ───────────────────────────────────────────
    #[tokio::test]
    async fn stress_test_rapid_error_commands() {
        let sandbox = ProcessSandbox;
        for i in 0..100 {
            let cmd = format!("nonexistent_cmd_{}", i);
            let result = sandbox.exec(&cmd, &cwd(), SandboxLevel::Light).await;
            assert!(!result.success);
        }
    }

    #[tokio::test]
    async fn stress_test_concurrent_sandbox() {
        let mut handles = Vec::new();
        for i in 0..10 {
            let h = tokio::spawn(async move {
                let sandbox = ProcessSandbox;
                for j in 0..5 {
                    let cmd = format!("nonexistent_{}_{}", i, j);
                    let result =
                        sandbox.exec(&cmd, &cwd(), SandboxLevel::Light).await;
                    assert!(!result.success);
                }
            });
            handles.push(h);
        }
        for h in handles {
            h.await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_exec_timeout() {
        let sandbox = ProcessSandbox;
        #[cfg(windows)]
        let cmd = "cmd /c ping 127.0.0.1 -n 3 > nul";
        #[cfg(unix)]
        let cmd = "sleep 2";

        // This should succeed because 2s < 60s
        let result = sandbox.exec(cmd, &cwd(), SandboxLevel::Light).await;
        assert!(
            result.success,
            "Expected command to finish before timeout, stderr: {}",
            result.stderr
        );
    }
}
