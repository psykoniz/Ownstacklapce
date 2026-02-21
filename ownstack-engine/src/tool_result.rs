use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub metadata: std::collections::HashMap<String, String>,
}

impl ToolResult {
    pub fn success(stdout: String) -> Self {
        Self {
            success: true,
            stdout,
            stderr: String::new(),
            exit_code: Some(0),
            metadata: std::collections::HashMap::new(),
        }
    }

    pub fn failure(stderr: String, code: Option<i32>) -> Self {
        Self {
            success: false,
            stdout: String::new(),
            stderr,
            exit_code: code,
            metadata: std::collections::HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ─── Constructor Tests ───────────────────────────────────────
    #[test]
    fn test_success_constructor() {
        let r = ToolResult::success("hello world".to_string());
        assert!(r.success);
        assert_eq!(r.stdout, "hello world");
        assert!(r.stderr.is_empty());
        assert_eq!(r.exit_code, Some(0));
        assert!(r.metadata.is_empty());
    }

    #[test]
    fn test_failure_constructor_with_code() {
        let r = ToolResult::failure("error occurred".to_string(), Some(1));
        assert!(!r.success);
        assert!(r.stdout.is_empty());
        assert_eq!(r.stderr, "error occurred");
        assert_eq!(r.exit_code, Some(1));
    }

    #[test]
    fn test_failure_constructor_no_code() {
        let r = ToolResult::failure("unknown error".to_string(), None);
        assert!(!r.success);
        assert_eq!(r.exit_code, None);
    }

    #[test]
    fn test_success_empty_stdout() {
        let r = ToolResult::success(String::new());
        assert!(r.success);
        assert!(r.stdout.is_empty());
    }

    #[test]
    fn test_failure_empty_stderr() {
        let r = ToolResult::failure(String::new(), Some(127));
        assert!(!r.success);
        assert!(r.stderr.is_empty());
        assert_eq!(r.exit_code, Some(127));
    }

    // ─── Serialization ──────────────────────────────────────────
    #[test]
    fn test_serialize_success() {
        let r = ToolResult::success("ok".to_string());
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"stdout\":\"ok\""));
    }

    #[test]
    fn test_serialize_failure() {
        let r = ToolResult::failure("bad".to_string(), Some(2));
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"success\":false"));
        assert!(json.contains("\"stderr\":\"bad\""));
        assert!(json.contains("\"exit_code\":2"));
    }

    #[test]
    fn test_deserialize_roundtrip() {
        let r = ToolResult::success("output".to_string());
        let json = serde_json::to_string(&r).unwrap();
        let back: ToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.success, r.success);
        assert_eq!(back.stdout, r.stdout);
        assert_eq!(back.stderr, r.stderr);
        assert_eq!(back.exit_code, r.exit_code);
    }

    #[test]
    fn test_serialize_with_metadata() {
        let mut r = ToolResult::success("ok".to_string());
        r.metadata.insert("key".to_string(), "value".to_string());
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"key\":\"value\""));

        let back: ToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.metadata.get("key").unwrap(), "value");
    }

    // ─── Clone ──────────────────────────────────────────────────
    #[test]
    fn test_clone() {
        let r = ToolResult::success("data".to_string());
        let r2 = r.clone();
        assert_eq!(r.success, r2.success);
        assert_eq!(r.stdout, r2.stdout);
        assert_eq!(r.exit_code, r2.exit_code);
    }

    // ─── Edge Cases ─────────────────────────────────────────────
    #[test]
    fn test_large_stdout() {
        let big = "x".repeat(100_000);
        let r = ToolResult::success(big.clone());
        assert_eq!(r.stdout.len(), 100_000);
    }

    #[test]
    fn test_large_stderr() {
        let big = "e".repeat(100_000);
        let r = ToolResult::failure(big.clone(), Some(1));
        assert_eq!(r.stderr.len(), 100_000);
    }

    #[test]
    fn test_negative_exit_code() {
        let r = ToolResult::failure("signal".to_string(), Some(-9));
        assert_eq!(r.exit_code, Some(-9));
    }

    #[test]
    fn test_exit_code_max_values() {
        let r = ToolResult::failure("max".to_string(), Some(i32::MAX));
        assert_eq!(r.exit_code, Some(i32::MAX));

        let r2 = ToolResult::failure("min".to_string(), Some(i32::MIN));
        assert_eq!(r2.exit_code, Some(i32::MIN));
    }

    #[test]
    fn test_metadata_multiple_entries() {
        let mut r = ToolResult::success("ok".to_string());
        for i in 0..100 {
            r.metadata
                .insert(format!("key_{}", i), format!("val_{}", i));
        }
        assert_eq!(r.metadata.len(), 100);
    }

    #[test]
    fn test_unicode_in_stdout() {
        let r = ToolResult::success("日本語テスト 🦀 émojis".to_string());
        assert!(r.stdout.contains("🦀"));
    }

    // ─── Stress Tests ───────────────────────────────────────────
    #[test]
    fn stress_test_create_1000_results() {
        for i in 0..1000 {
            let r = if i % 2 == 0 {
                ToolResult::success(format!("output_{}", i))
            } else {
                ToolResult::failure(format!("error_{}", i), Some(i))
            };
            let _ = serde_json::to_string(&r).unwrap();
        }
    }
}
