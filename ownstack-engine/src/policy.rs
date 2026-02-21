use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PolicyDecision {
    Auto,
    Ask,
    Blocked,
}

pub struct PolicyEngine;

impl PolicyEngine {
    /// Evaluates a command string against security rules and returns a decision.
    pub fn evaluate(command: &str) -> PolicyDecision {
        let cmd = command.trim().to_lowercase();

        // 1. Blocked commands (destructive or high risk)
        if Self::is_blocked(&cmd) {
            return PolicyDecision::Blocked;
        }

        // 2. Commands requiring confirmation
        if Self::needs_confirmation(&cmd) {
            return PolicyDecision::Ask;
        }

        // 3. Allowed commands (safe or read-only)
        PolicyDecision::Auto
    }

    fn is_blocked(cmd: &str) -> bool {
        let blocked_patterns = [
            "rm -rf /",
            "sudo ",
            "chmod 777",
            "mkfs",
            "dd if=",
            "shutdown",
            "reboot",
            "halt",
            "kill -9 1",
            "mount ",
            "umount ",
            "> /etc/",
            "> /usr/",
            "> /bin/",
            "> /sbin/",
            "> /var/",
        ];

        blocked_patterns.iter().any(|&p| cmd.contains(p))
    }

    fn needs_confirmation(cmd: &str) -> bool {
        let confirmation_patterns = [
            "git push",
            "git reset --hard",
            "npm publish",
            "cargo publish",
            "docker rm",
            "docker rmi",
            "rm -rf ",
            "curl ",
            "wget ",
        ];

        confirmation_patterns.iter().any(|&p| cmd.contains(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Blocked Commands ────────────────────────────────────────
    #[test]
    fn test_blocked_rm_rf_root() {
        assert_eq!(PolicyEngine::evaluate("rm -rf /"), PolicyDecision::Blocked);
    }

    #[test]
    fn test_blocked_sudo() {
        assert_eq!(
            PolicyEngine::evaluate("sudo apt update"),
            PolicyDecision::Blocked
        );
        assert_eq!(
            PolicyEngine::evaluate("sudo rm file.txt"),
            PolicyDecision::Blocked
        );
        assert_eq!(PolicyEngine::evaluate("sudo -i"), PolicyDecision::Blocked);
    }

    #[test]
    fn test_blocked_chmod_777() {
        assert_eq!(
            PolicyEngine::evaluate("chmod 777 /etc/passwd"),
            PolicyDecision::Blocked
        );
    }

    #[test]
    fn test_blocked_mkfs() {
        assert_eq!(
            PolicyEngine::evaluate("mkfs.ext4 /dev/sda"),
            PolicyDecision::Blocked
        );
    }

    #[test]
    fn test_blocked_dd() {
        assert_eq!(
            PolicyEngine::evaluate("dd if=/dev/zero of=/dev/sda"),
            PolicyDecision::Blocked
        );
    }

    #[test]
    fn test_blocked_shutdown_reboot_halt() {
        assert_eq!(
            PolicyEngine::evaluate("shutdown now"),
            PolicyDecision::Blocked
        );
        assert_eq!(PolicyEngine::evaluate("reboot"), PolicyDecision::Blocked);
        assert_eq!(PolicyEngine::evaluate("halt"), PolicyDecision::Blocked);
    }

    #[test]
    fn test_blocked_kill_init() {
        assert_eq!(PolicyEngine::evaluate("kill -9 1"), PolicyDecision::Blocked);
    }

    #[test]
    fn test_blocked_mount_umount() {
        assert_eq!(
            PolicyEngine::evaluate("mount /dev/sdb1 /mnt"),
            PolicyDecision::Blocked
        );
        assert_eq!(
            PolicyEngine::evaluate("umount /mnt"),
            PolicyDecision::Blocked
        );
    }

    #[test]
    fn test_blocked_write_to_system_dirs() {
        assert_eq!(
            PolicyEngine::evaluate("echo hacked > /etc/shadow"),
            PolicyDecision::Blocked
        );
        assert_eq!(
            PolicyEngine::evaluate("echo data > /usr/bin/test"),
            PolicyDecision::Blocked
        );
        assert_eq!(
            PolicyEngine::evaluate("echo data > /bin/sh"),
            PolicyDecision::Blocked
        );
        assert_eq!(
            PolicyEngine::evaluate("echo data > /sbin/init"),
            PolicyDecision::Blocked
        );
        assert_eq!(
            PolicyEngine::evaluate("echo data > /var/log/syslog"),
            PolicyDecision::Blocked
        );
    }

    // ─── Ask Commands ────────────────────────────────────────────
    #[test]
    fn test_ask_git_push() {
        assert_eq!(
            PolicyEngine::evaluate("git push origin main"),
            PolicyDecision::Ask
        );
        assert_eq!(
            PolicyEngine::evaluate("git push --force"),
            PolicyDecision::Ask
        );
    }

    #[test]
    fn test_ask_git_reset_hard() {
        assert_eq!(
            PolicyEngine::evaluate("git reset --hard HEAD~1"),
            PolicyDecision::Ask
        );
    }

    #[test]
    fn test_ask_publish() {
        assert_eq!(PolicyEngine::evaluate("npm publish"), PolicyDecision::Ask);
        assert_eq!(PolicyEngine::evaluate("cargo publish"), PolicyDecision::Ask);
    }

    #[test]
    fn test_ask_docker_rm() {
        assert_eq!(
            PolicyEngine::evaluate("docker rm container123"),
            PolicyDecision::Ask
        );
        assert_eq!(
            PolicyEngine::evaluate("docker rmi image:tag"),
            PolicyDecision::Ask
        );
    }

    #[test]
    fn test_ask_rm_rf_workspace() {
        assert_eq!(PolicyEngine::evaluate("rm -rf src/"), PolicyDecision::Ask);
        assert_eq!(
            PolicyEngine::evaluate("rm -rf node_modules"),
            PolicyDecision::Ask
        );
    }

    #[test]
    fn test_ask_network() {
        assert_eq!(
            PolicyEngine::evaluate("curl https://example.com"),
            PolicyDecision::Ask
        );
        assert_eq!(
            PolicyEngine::evaluate("wget https://example.com/file"),
            PolicyDecision::Ask
        );
    }

    // ─── Auto Commands ───────────────────────────────────────────
    #[test]
    fn test_auto_safe_commands() {
        assert_eq!(PolicyEngine::evaluate("ls -la"), PolicyDecision::Auto);
        assert_eq!(PolicyEngine::evaluate("cat file.txt"), PolicyDecision::Auto);
        assert_eq!(
            PolicyEngine::evaluate("grep -r pattern src/"),
            PolicyDecision::Auto
        );
        assert_eq!(
            PolicyEngine::evaluate("find . -name '*.rs'"),
            PolicyDecision::Auto
        );
        assert_eq!(
            PolicyEngine::evaluate("head -20 README.md"),
            PolicyDecision::Auto
        );
        assert_eq!(
            PolicyEngine::evaluate("tail -f log.txt"),
            PolicyDecision::Auto
        );
    }

    #[test]
    fn test_auto_build_commands() {
        assert_eq!(PolicyEngine::evaluate("cargo build"), PolicyDecision::Auto);
        assert_eq!(PolicyEngine::evaluate("cargo test"), PolicyDecision::Auto);
        assert_eq!(PolicyEngine::evaluate("cargo check"), PolicyDecision::Auto);
    }

    #[test]
    fn test_auto_git_readonly() {
        assert_eq!(PolicyEngine::evaluate("git status"), PolicyDecision::Auto);
        assert_eq!(PolicyEngine::evaluate("git diff"), PolicyDecision::Auto);
        assert_eq!(PolicyEngine::evaluate("git log -5"), PolicyDecision::Auto);
        assert_eq!(PolicyEngine::evaluate("git add ."), PolicyDecision::Auto);
        assert_eq!(
            PolicyEngine::evaluate("git commit -m 'test'"),
            PolicyDecision::Auto
        );
    }

    // ─── Edge Cases ──────────────────────────────────────────────
    #[test]
    fn test_empty_command() {
        assert_eq!(PolicyEngine::evaluate(""), PolicyDecision::Auto);
    }

    #[test]
    fn test_whitespace_only() {
        assert_eq!(PolicyEngine::evaluate("   "), PolicyDecision::Auto);
    }

    #[test]
    fn test_case_insensitivity() {
        assert_eq!(
            PolicyEngine::evaluate("SUDO apt update"),
            PolicyDecision::Blocked
        );
        assert_eq!(
            PolicyEngine::evaluate("Sudo rm test"),
            PolicyDecision::Blocked
        );
        assert_eq!(PolicyEngine::evaluate("SHUTDOWN"), PolicyDecision::Blocked);
        assert_eq!(
            PolicyEngine::evaluate("GIT PUSH origin"),
            PolicyDecision::Ask
        );
    }

    #[test]
    fn test_leading_trailing_whitespace() {
        assert_eq!(
            PolicyEngine::evaluate("  sudo apt update  "),
            PolicyDecision::Blocked
        );
        assert_eq!(
            PolicyEngine::evaluate("  git push origin  "),
            PolicyDecision::Ask
        );
    }

    #[test]
    fn test_very_long_command() {
        let long_cmd = "echo ".to_string() + &"a".repeat(10000);
        // Should not panic or crash
        let _ = PolicyEngine::evaluate(&long_cmd);
    }

    #[test]
    fn test_special_characters() {
        assert_eq!(
            PolicyEngine::evaluate("echo 'hello world'"),
            PolicyDecision::Auto
        );
        assert_eq!(
            PolicyEngine::evaluate("echo \"test $VAR\""),
            PolicyDecision::Auto
        );
        assert_eq!(
            PolicyEngine::evaluate("cat file\twith\ttabs"),
            PolicyDecision::Auto
        );
    }

    #[test]
    fn test_command_in_middle_of_string() {
        // "sudo" embedded in a longer command string
        assert_eq!(
            PolicyEngine::evaluate("echo sudo test"),
            PolicyDecision::Blocked
        );
    }

    #[test]
    fn test_newline_in_command() {
        assert_eq!(
            PolicyEngine::evaluate("echo hello\nsudo rm"),
            PolicyDecision::Blocked
        );
    }

    // ─── PolicyDecision Serialization ────────────────────────────
    #[test]
    fn test_policy_decision_serialization() {
        let auto = PolicyDecision::Auto;
        let json = serde_json::to_string(&auto).unwrap();
        assert!(json.contains("Auto"));

        let back: PolicyDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(back, PolicyDecision::Auto);
    }

    #[test]
    fn test_all_decisions_serialize_roundtrip() {
        for decision in [
            PolicyDecision::Auto,
            PolicyDecision::Ask,
            PolicyDecision::Blocked,
        ] {
            let json = serde_json::to_string(&decision).unwrap();
            let back: PolicyDecision = serde_json::from_str(&json).unwrap();
            assert_eq!(decision, back);
        }
    }

    #[test]
    fn test_policy_decision_clone_eq() {
        let d = PolicyDecision::Ask;
        let d2 = d.clone();
        assert_eq!(d, d2);
    }

    // ─── Stress Tests ────────────────────────────────────────────
    #[test]
    fn stress_test_sequential_evaluations() {
        let commands = vec![
            "ls",
            "cat foo",
            "rm -rf /",
            "sudo apt",
            "git push",
            "cargo build",
            "grep test",
            "shutdown",
            "reboot",
        ];
        for _ in 0..1000 {
            for cmd in &commands {
                let _ = PolicyEngine::evaluate(cmd);
            }
        }
        // 9000 evaluations without crash
    }

    #[test]
    fn stress_test_concurrent_evaluations() {
        use std::thread;
        let handles: Vec<_> = (0..100)
            .map(|i| {
                thread::spawn(move || {
                    for _ in 0..100 {
                        let cmd = format!("test_command_{}", i);
                        let _ = PolicyEngine::evaluate(&cmd);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
        // 10000 concurrent evaluations
    }

    #[test]
    fn stress_test_all_blocked_patterns() {
        let blocked = [
            "rm -rf /",
            "sudo anything",
            "chmod 777 file",
            "mkfs.ext4 x",
            "dd if=/dev/zero",
            "shutdown -h now",
            "reboot",
            "halt",
            "kill -9 1",
            "mount /dev/sda",
            "umount /mnt",
            "> /etc/test",
            "> /usr/test",
            "> /bin/test",
            "> /sbin/test",
            "> /var/test",
        ];
        for cmd in &blocked {
            assert_eq!(
                PolicyEngine::evaluate(cmd),
                PolicyDecision::Blocked,
                "Expected Blocked for: {}",
                cmd
            );
        }
    }
}
