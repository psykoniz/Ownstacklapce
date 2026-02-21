use crate::sandbox::SandboxLevel;
use std::process::Command;

fn has_unshare() -> bool {
    if std::env::var("OWNSTACK_DISABLE_UNSHARE").ok().as_deref() == Some("1") {
        return false;
    }
    Command::new("unshare")
        .arg("--help")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn resolve_command(
    command: &str,
    args: &[String],
    original_command: &str,
    level: SandboxLevel,
) -> (String, Vec<String>) {
    if level != SandboxLevel::Strict || !has_unshare() {
        return (command.to_string(), args.to_vec());
    }

    // Best-effort strict isolation on Linux:
    // - user namespace with mapped root
    // - pid/mount/net namespaces
    // - execute original command through /bin/sh -lc
    (
        "unshare".to_string(),
        vec![
            "--user".to_string(),
            "--map-root-user".to_string(),
            "--pid".to_string(),
            "--fork".to_string(),
            "--mount".to_string(),
            "--net".to_string(),
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
    fn light_mode_keeps_original_command() {
        let (cmd, args) = resolve_command(
            "echo",
            &["hello".to_string()],
            "echo hello",
            SandboxLevel::Light,
        );
        assert_eq!(cmd, "echo");
        assert_eq!(args, vec!["hello".to_string()]);
    }

    #[test]
    fn strict_mode_uses_unshare_when_available() {
        let (cmd, _args) = resolve_command(
            "echo",
            &["hello".to_string()],
            "echo hello",
            SandboxLevel::Strict,
        );
        if has_unshare() {
            assert_eq!(cmd, "unshare");
        } else {
            assert_eq!(cmd, "echo");
        }
    }
}
