//! Telemetry — Structured JSONL logging with OpenTelemetry-compatible tracing.
//!
//! Provides:
//! - `BlackBoxLogger`: appends structured events to `.ownstack/telemetry/{session}.jsonl`
//! - `TraceContext`: lightweight span context (trace_id, span_id, parent)
//! - `TokioLagMonitor`: detects high event loop lag in the async runtime

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{debug, warn};
use uuid::Uuid;

// ─── Trace Context ───────────────────────────────────────────────

/// Lightweight OpenTelemetry-compatible trace context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    pub operation: String,
    #[serde(skip)]
    pub start_time: Option<Instant>,
}

impl TraceContext {
    /// Create a new root trace.
    pub fn new_root(operation: &str) -> Self {
        Self {
            trace_id: Uuid::new_v4().to_string(),
            span_id: Uuid::new_v4().to_string()[..16].to_string(),
            parent_span_id: None,
            operation: operation.to_string(),
            start_time: Some(Instant::now()),
        }
    }

    /// Create a child span under this trace.
    pub fn child(&self, operation: &str) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: Uuid::new_v4().to_string()[..16].to_string(),
            parent_span_id: Some(self.span_id.clone()),
            operation: operation.to_string(),
            start_time: Some(Instant::now()),
        }
    }

    /// Elapsed time in milliseconds since this span started.
    pub fn elapsed_ms(&self) -> u64 {
        self.start_time
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }
}

// ─── Black Box Logger ────────────────────────────────────────────

/// Structured entry in the telemetry JSONL log.
#[derive(Serialize)]
struct TelemetryEntry {
    timestamp: f64,
    event: String,
    session_id: String,
    data: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    span_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_span_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
}

/// Appends structured events to a JSONL file for auditing and debugging.
pub struct BlackBoxLogger {
    session_id: String,
    log_file: PathBuf,
}

impl BlackBoxLogger {
    /// Create a new logger. Creates the telemetry directory if needed.
    pub fn new(session_id: &str, workspace: &std::path::Path) -> Self {
        let log_dir = workspace.join(".ownstack").join("telemetry");
        let _ = std::fs::create_dir_all(&log_dir);
        let log_file = log_dir.join(format!("{session_id}.jsonl"));

        Self {
            session_id: session_id.to_string(),
            log_file,
        }
    }

    /// Log an event with optional trace context.
    pub fn log(
        &self,
        event_type: &str,
        data: serde_json::Value,
        trace: Option<&TraceContext>,
    ) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        let entry = TelemetryEntry {
            timestamp,
            event: event_type.to_string(),
            session_id: self.session_id.clone(),
            data: sanitize_log_data(data),
            trace_id: trace.map(|t| t.trace_id.clone()),
            span_id: trace.map(|t| t.span_id.clone()),
            parent_span_id: trace.and_then(|t| t.parent_span_id.clone()),
            duration_ms: trace.map(|t| t.elapsed_ms()),
        };

        if let Ok(json) = serde_json::to_string(&entry) {
            use std::io::Write;
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.log_file)
            {
                Ok(mut f) => {
                    let _ = writeln!(f, "{json}");
                }
                Err(e) => {
                    debug!("Telemetry log write failed: {}", e);
                }
            }
        }
    }

    /// Log a simple event without trace context.
    pub fn log_simple(&self, event_type: &str, data: serde_json::Value) {
        self.log(event_type, data, None);
    }
}

/// Returns true if a JSON object key looks like it holds a secret.
fn is_sensitive_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    ["api_key", "apikey", "token", "secret", "password", "credential", "authorization", "bearer"]
        .iter()
        .any(|needle| k.contains(needle))
}

/// Scrub known secret formats from a free-text string value.
fn scrub_secret_strings(s: &str) -> String {
    // Common API-key prefixes: sk-..., sk-or-v1-..., sk-ant-..., etc.
    // Replace any token of the form (sk|pk|rk)-... with [REDACTED].
    let re = regex::Regex::new(r"\b(?:sk|pk|rk)-[A-Za-z0-9\-_]{8,}\b").unwrap();
    re.replace_all(s, "[REDACTED]").to_string()
}

/// Recursively redact sensitive fields from telemetry data before persisting.
/// Port of `ownstack-python/app/utils/security.py::sanitize_log_data`.
fn sanitize_log_data(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                if is_sensitive_key(&k) {
                    out.insert(k, Value::String("[REDACTED]".to_string()));
                } else {
                    out.insert(k, sanitize_log_data(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(arr) => {
            Value::Array(arr.into_iter().map(sanitize_log_data).collect())
        }
        Value::String(s) => Value::String(scrub_secret_strings(&s)),
        other => other,
    }
}

// ─── Tokio Lag Monitor ───────────────────────────────────────────

/// Monitors the tokio event loop for high lag.
///
/// Runs a background task that sleeps for 1 second and measures how long
/// the sleep actually took. If it took significantly longer, the event
/// loop is congested.
pub struct TokioLagMonitor {
    threshold_ms: f64,
    running: Arc<AtomicBool>,
    last_lag_ms: Arc<Mutex<f64>>,
}

impl TokioLagMonitor {
    pub fn new(threshold_ms: f64) -> Self {
        Self {
            threshold_ms,
            running: Arc::new(AtomicBool::new(false)),
            last_lag_ms: Arc::new(Mutex::new(0.0)),
        }
    }

    /// Start the monitoring background task.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let last_lag = self.last_lag_ms.clone();
        let threshold = self.threshold_ms;

        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                let start = Instant::now();
                tokio::time::sleep(Duration::from_secs(1)).await;
                let elapsed = start.elapsed();

                // Expected 1s, anything above is event loop lag
                let lag_ms = (elapsed.as_secs_f64() - 1.0) * 1000.0;
                let lag_ms = lag_ms.max(0.0);

                *last_lag.lock().await = lag_ms;

                if lag_ms > threshold {
                    warn!("High event loop lag detected: {lag_ms:.2}ms");
                }
            }
        })
    }

    /// Stop the monitor.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Get the last measured lag in milliseconds.
    pub async fn get_lag_ms(&self) -> f64 {
        *self.last_lag_ms.lock().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_trace_context_root() {
        let ctx = TraceContext::new_root("test_op");
        assert!(!ctx.trace_id.is_empty());
        assert!(!ctx.span_id.is_empty());
        assert!(ctx.parent_span_id.is_none());
        assert_eq!(ctx.operation, "test_op");
    }

    #[test]
    fn test_trace_context_child() {
        let root = TraceContext::new_root("parent");
        let child = root.child("child_op");

        assert_eq!(child.trace_id, root.trace_id);
        assert_ne!(child.span_id, root.span_id);
        assert_eq!(child.parent_span_id, Some(root.span_id));
        assert_eq!(child.operation, "child_op");
    }

    #[test]
    fn test_blackbox_logger_writes_jsonl() {
        let dir = tempdir().unwrap();
        let logger = BlackBoxLogger::new("test-session", dir.path());

        logger.log_simple("tool_call", serde_json::json!({"tool": "search"}));
        logger.log_simple("tool_result", serde_json::json!({"success": true}));

        let content = std::fs::read_to_string(&logger.log_file).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        // Verify JSONL is valid
        let entry: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(entry["event"], "tool_call");
        assert_eq!(entry["session_id"], "test-session");
    }

    #[test]
    fn test_sanitize_redacts_keys_and_strings() {
        let input = serde_json::json!({
            "api_key": "sk-or-v1-abcdef0123456789",
            "nested": { "Authorization": "Bearer xyz", "ok": "value" },
            "message": "my key is sk-ant-0123456789abcd here",
            "list": ["sk-deadbeefcafebabe", "plain"],
            "count": 42
        });
        let out = sanitize_log_data(input);
        assert_eq!(out["api_key"], "[REDACTED]");
        assert_eq!(out["nested"]["Authorization"], "[REDACTED]");
        assert_eq!(out["nested"]["ok"], "value");
        assert!(out["message"].as_str().unwrap().contains("[REDACTED]"));
        assert!(!out["message"].as_str().unwrap().contains("sk-ant"));
        assert_eq!(out["list"][0], "[REDACTED]");
        assert_eq!(out["list"][1], "plain");
        assert_eq!(out["count"], 42);
    }

    #[test]
    fn test_blackbox_logger_redacts_secrets_on_disk() {
        let dir = tempdir().unwrap();
        let logger = BlackBoxLogger::new("sec-session", dir.path());
        logger.log_simple(
            "llm_call",
            serde_json::json!({"api_key": "sk-or-v1-shouldnotappear"}),
        );
        let content = std::fs::read_to_string(&logger.log_file).unwrap();
        assert!(!content.contains("shouldnotappear"));
        assert!(content.contains("[REDACTED]"));
    }

    #[test]
    fn test_blackbox_logger_with_trace() {
        let dir = tempdir().unwrap();
        let logger = BlackBoxLogger::new("traced-session", dir.path());
        let trace = TraceContext::new_root("test_span");

        logger.log(
            "llm_call",
            serde_json::json!({"model": "deepseek"}),
            Some(&trace),
        );

        let content = std::fs::read_to_string(&logger.log_file).unwrap();
        let entry: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert!(entry["trace_id"].is_string());
        assert!(entry["span_id"].is_string());
        assert!(entry["duration_ms"].is_number());
    }

    #[tokio::test]
    async fn test_lag_monitor_starts_and_stops() {
        let monitor = TokioLagMonitor::new(100.0);
        let handle = monitor.start();

        // Let it run briefly
        tokio::time::sleep(Duration::from_millis(100)).await;

        let lag = monitor.get_lag_ms().await;
        // Lag should be very small (we haven't blocked the loop)
        assert!(lag < 500.0, "lag was unexpectedly high: {lag}ms");

        monitor.stop();
        // Give the task time to notice the stop signal
        tokio::time::sleep(Duration::from_millis(50)).await;
        handle.abort();
    }
}
