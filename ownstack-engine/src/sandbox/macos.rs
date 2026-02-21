use crate::sandbox::SandboxLevel;
use std::path::Path;
use std::process::Command;

fn has_sandbox_exec() -> bool {
    Command::new("sandbox-exec")
        .arg("-h")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn escape_profile_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn sandbox_profile(workspace: &Path) -> String {
    let ws = escape_profile_path(workspace);
    format!(
        "(version 1) \
         (deny default) \
         (allow process-exec) \
         (allow process-fork) \
         (allow file-read*) \
         (allow file-write* (subpath \"{ws}\"))"
    )
}

pub fn resolve_command(
    command: &str,
    args: &[String],
    original_command: &str,
    cwd: &Path,
    level: SandboxLevel,
) -> (String, Vec<String>) {
    if level != SandboxLevel::Strict || !has_sandbox_exec() {
        return (command.to_string(), args.to_vec());
    }

    (
        "sandbox-exec".to_string(),
        vec![
            "-p".to_string(),
            sandbox_profile(cwd),
            "/bin/sh".to_string(),
            "-lc".to_string(),
            original_command.to_string(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_contains_workspace_subpath() {
        let profile = sandbox_profile(Path::new("/tmp/my-workspace"));
        assert!(profile.contains("subpath"));
        assert!(profile.contains("/tmp/my-workspace"));
    }

    #[test]
    fn strict_mode_uses_sandbox_exec_when_available() {
        let (cmd, _args) = resolve_command(
            "echo",
            &["hello".to_string()],
            "echo hello",
            Path::new("/tmp"),
            SandboxLevel::Strict,
        );
        if has_sandbox_exec() {
            assert_eq!(cmd, "sandbox-exec");
        } else {
            assert_eq!(cmd, "echo");
        }
    }
}
