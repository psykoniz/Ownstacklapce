use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PathError {
    #[error("Path is outside the allowed workspace: {0}")]
    OutsideWorkspace(String),
    #[error("Path contains forbidden sequences (..): {0}")]
    ForbiddenSequence(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct PathValidator {
    workspace_root: PathBuf,
}

impl PathValidator {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root: workspace_root.canonicalize().unwrap_or(workspace_root),
        }
    }

    /// Validates that a path is safe to access.
    pub fn validate(&self, path: &Path) -> Result<PathBuf, PathError> {
        let path_str = path.to_string_lossy();

        // 1. Check for ".." to prevent path traversal
        if path_str.contains("..") {
            return Err(PathError::ForbiddenSequence(path_str.into_owned()));
        }

        // 2. Canonicalize and check against workspace root
        let absolute_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        };

        let canonical_path = absolute_path.canonicalize().map_err(|_| {
            // If it doesn't exist, we still check the prefix of its parent
            absolute_path.clone()
        });

        let final_path = match canonical_path {
            Ok(p) => p,
            Err(p) => p,
        };

        if !final_path.starts_with(&self.workspace_root) {
            return Err(PathError::OutsideWorkspace(
                final_path.to_string_lossy().into_owned(),
            ));
        }

        Ok(final_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    fn workspace() -> PathBuf {
        env::current_dir().unwrap()
    }

    // ─── Valid Paths ─────────────────────────────────────────────
    #[test]
    fn test_valid_relative_path() {
        let v = PathValidator::new(workspace());
        assert!(v.validate(Path::new("src/lib.rs")).is_ok());
    }

    #[test]
    fn test_valid_nested_relative() {
        let v = PathValidator::new(workspace());
        assert!(v.validate(Path::new("src/policy.rs")).is_ok());
    }

    #[test]
    fn test_valid_current_dir() {
        let v = PathValidator::new(workspace());
        assert!(v.validate(Path::new(".")).is_ok());
    }

    // ─── Path Traversal Attacks ──────────────────────────────────
    #[test]
    fn test_reject_dotdot_simple() {
        let v = PathValidator::new(workspace());
        assert!(v.validate(Path::new("../etc/passwd")).is_err());
    }

    #[test]
    fn test_reject_dotdot_nested() {
        let v = PathValidator::new(workspace());
        assert!(v.validate(Path::new("src/../../etc/shadow")).is_err());
    }

    #[test]
    fn test_reject_dotdot_deeply_nested() {
        let v = PathValidator::new(workspace());
        assert!(v.validate(Path::new("a/b/c/../../../../etc")).is_err());
    }

    #[test]
    fn test_reject_dotdot_at_end() {
        let v = PathValidator::new(workspace());
        assert!(v.validate(Path::new("src/..")).is_err());
    }

    #[test]
    fn test_reject_just_dotdot() {
        let v = PathValidator::new(workspace());
        assert!(v.validate(Path::new("..")).is_err());
    }

    // ─── Absolute Paths Outside Workspace ────────────────────────
    #[test]
    fn test_reject_system_paths() {
        let v = PathValidator::new(workspace());
        #[cfg(windows)]
        {
            assert!(v.validate(Path::new("C:\\Windows\\System32")).is_err());
            assert!(v.validate(Path::new("C:\\Users")).is_err());
        }
        #[cfg(unix)]
        {
            assert!(v.validate(Path::new("/etc/passwd")).is_err());
            assert!(v.validate(Path::new("/usr/bin/rm")).is_err());
            assert!(v.validate(Path::new("/tmp/evil")).is_err());
        }
    }

    #[test]
    fn test_reject_root_path() {
        let v = PathValidator::new(workspace());
        #[cfg(windows)]
        assert!(v.validate(Path::new("C:\\")).is_err());
        #[cfg(unix)]
        assert!(v.validate(Path::new("/")).is_err());
    }

    // ─── Edge Cases ──────────────────────────────────────────────
    #[test]
    fn test_empty_path() {
        let v = PathValidator::new(workspace());
        // Empty relative path resolves to workspace root
        let result = v.validate(Path::new(""));
        // Behavior depends on platform; should not panic
        let _ = result;
    }

    #[test]
    fn test_dot_path() {
        let v = PathValidator::new(workspace());
        assert!(v.validate(Path::new(".")).is_ok());
    }

    #[test]
    fn test_path_with_spaces() {
        let v = PathValidator::new(workspace());
        // Should not panic even if path has spaces
        let _ = v.validate(Path::new("file with spaces.txt"));
    }

    #[test]
    fn test_path_with_unicode() {
        let v = PathValidator::new(workspace());
        let _ = v.validate(Path::new("fichier_éàü.txt"));
    }

    #[test]
    fn test_very_long_path() {
        let v = PathValidator::new(workspace());
        let long = "a/".repeat(200) + "file.txt";
        let _ = v.validate(Path::new(&long));
    }

    // ─── PathError Types ─────────────────────────────────────────
    #[test]
    fn test_error_type_forbidden_sequence() {
        let v = PathValidator::new(workspace());
        match v.validate(Path::new("../etc")) {
            Err(PathError::ForbiddenSequence(_)) => {}
            other => panic!("Expected ForbiddenSequence, got {:?}", other),
        }
    }

    #[test]
    fn test_error_display() {
        let e = PathError::OutsideWorkspace("/etc/passwd".to_string());
        assert!(e.to_string().contains("/etc/passwd"));

        let e2 = PathError::ForbiddenSequence("../etc".to_string());
        assert!(e2.to_string().contains(".."));
    }

    // ─── Constructor ─────────────────────────────────────────────
    #[test]
    fn test_new_with_existing_dir() {
        let v = PathValidator::new(workspace());
        // Should not panic
        let _ = v.validate(Path::new("Cargo.toml"));
    }

    #[test]
    fn test_new_with_nonexistent_dir() {
        // Should not panic even with invalid workspace
        let v = PathValidator::new(PathBuf::from("/nonexistent/path/12345"));
        let _ = v.validate(Path::new("file.txt"));
    }

    // ─── Stress Tests ────────────────────────────────────────────
    #[test]
    fn stress_test_1000_path_validations() {
        let v = PathValidator::new(workspace());
        for i in 0..1000 {
            let path = format!("src/test_file_{}.rs", i);
            let _ = v.validate(Path::new(&path));
        }
    }

    #[test]
    fn stress_test_concurrent_validations() {
        use std::sync::Arc;
        use std::thread;

        let ws = workspace();
        let handles: Vec<_> = (0..50)
            .map(|i| {
                let ws = ws.clone();
                thread::spawn(move || {
                    let v = PathValidator::new(ws);
                    for j in 0..100 {
                        let p = format!("file_{}_{}.txt", i, j);
                        let _ = v.validate(Path::new(&p));
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn stress_test_attack_patterns() {
        let v = PathValidator::new(workspace());
        let attacks = [
            "../etc/passwd",
            "../../etc/shadow",
            "../../../root/.ssh/id_rsa",
            "src/../../../etc/hosts",
            "../../../../bin/sh",
            "../..",
            "../../..",
            "../../../..",
        ];
        for attack in &attacks {
            assert!(
                v.validate(Path::new(attack)).is_err(),
                "Should reject traversal: {}",
                attack
            );
        }
    }
}
