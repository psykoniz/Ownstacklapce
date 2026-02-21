use crate::tool_result::ToolResult;
use std::path::Path;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod process;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SandboxLevel {
    /// Level 1: Light isolation (standard process env_clear)
    Light,
    /// Level 2: Standard isolation (process + timeout + resource limits)
    Standard,
    /// Level 3: Strict isolation (Docker or OS-level restricted containers)
    Strict,
}

pub trait Sandbox {
    /// Executes a command in the sandbox.
    fn exec(&self, command: &str, cwd: &Path, level: SandboxLevel) -> ToolResult;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_level_clone() {
        let l = SandboxLevel::Light;
        let l2 = l.clone();
        assert_eq!(l, l2);
    }

    #[test]
    fn test_sandbox_level_copy() {
        let l = SandboxLevel::Standard;
        let l2 = l;
        assert_eq!(l, l2); // works because Copy
    }

    #[test]
    fn test_all_levels_exist() {
        let levels = [
            SandboxLevel::Light,
            SandboxLevel::Standard,
            SandboxLevel::Strict,
        ];
        assert_eq!(levels.len(), 3);
    }

    #[test]
    fn test_sandbox_level_debug() {
        let s = format!("{:?}", SandboxLevel::Light);
        assert!(s.contains("Light"));
        let s2 = format!("{:?}", SandboxLevel::Standard);
        assert!(s2.contains("Standard"));
        let s3 = format!("{:?}", SandboxLevel::Strict);
        assert!(s3.contains("Strict"));
    }

    #[test]
    fn test_sandbox_level_partial_eq() {
        assert_ne!(SandboxLevel::Light, SandboxLevel::Standard);
        assert_ne!(SandboxLevel::Standard, SandboxLevel::Strict);
        assert_ne!(SandboxLevel::Light, SandboxLevel::Strict);
        assert_eq!(SandboxLevel::Light, SandboxLevel::Light);
    }
}
