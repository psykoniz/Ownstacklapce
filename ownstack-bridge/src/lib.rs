use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

pub const BRIDGE_CONTRACT_NAME: &str = "ownstack.bridge.jsonio";
pub const BRIDGE_CONTRACT_VERSION: u32 = 1;
const JSON_RPC_VERSION: &str = "2.0";

#[derive(Error, Debug)]
pub enum BridgeError {
    #[error("Failed to spawn bridge process: {0}")]
    ProcessSpawn(String),
    #[error("IO error: {0}")]
    Io(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    #[error("RPC error: {0}")]
    RpcError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeContract {
    pub name: String,
    pub version: u32,
}

impl BridgeContract {
    pub fn current() -> Self {
        Self {
            name: BRIDGE_CONTRACT_NAME.to_string(),
            version: BRIDGE_CONTRACT_VERSION,
        }
    }

    fn is_current(&self) -> bool {
        self.name == BRIDGE_CONTRACT_NAME && self.version == BRIDGE_CONTRACT_VERSION
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
    pub contract: BridgeContract,
}

impl BridgeRequest {
    fn new(id: u64, method: String, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id,
            method,
            params,
            contract: BridgeContract::current(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BridgeRpcError {
    Message(String),
    Detailed {
        #[serde(default)]
        code: Option<i64>,
        message: String,
        #[serde(default)]
        data: Option<serde_json::Value>,
    },
}

impl std::fmt::Display for BridgeRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message(message) => write!(f, "{message}"),
            Self::Detailed { code, message, .. } => {
                if let Some(code) = code {
                    write!(f, "code {code}: {message}")
                } else {
                    write!(f, "{message}")
                }
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeResponse {
    #[serde(default = "default_jsonrpc_version")]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<BridgeRpcError>,
    #[serde(default)]
    pub contract: Option<BridgeContract>,
}

fn default_jsonrpc_version() -> String {
    JSON_RPC_VERSION.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeRuntimeMode {
    Auto,
    DevPythonScript,
    BundledExecutable,
}

#[derive(Debug, Clone)]
pub struct BridgeLaunchConfig {
    pub mode: BridgeRuntimeMode,
    pub workspace: PathBuf,
    pub python_root: Option<PathBuf>,
    pub bundled_path: Option<PathBuf>,
}

impl BridgeLaunchConfig {
    pub fn from_workspace(workspace: PathBuf) -> Self {
        Self {
            mode: BridgeRuntimeMode::Auto,
            workspace,
            python_root: None,
            bundled_path: None,
        }
    }

    fn effective_mode(&self) -> BridgeRuntimeMode {
        match std::env::var("OWNSTACK_BRIDGE_MODE") {
            Ok(mode) => match mode.to_ascii_lowercase().as_str() {
                "dev" | "script" => BridgeRuntimeMode::DevPythonScript,
                "bundled" | "prod" => BridgeRuntimeMode::BundledExecutable,
                _ => self.mode,
            },
            Err(_) => self.mode,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct BridgeMetricEvent {
    timestamp_ms: u128,
    method: String,
    success: bool,
    latency_ms: u128,
    error: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct BridgeEndpointStatsInternal {
    calls: u64,
    errors: u64,
    total_latency_ms: u128,
    max_latency_ms: u128,
}

#[derive(Debug, Default, Clone)]
struct BridgeMetrics {
    total_calls: u64,
    total_errors: u64,
    endpoints: HashMap<String, BridgeEndpointStatsInternal>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct BridgeEndpointMetrics {
    pub calls: u64,
    pub errors: u64,
    pub avg_latency_ms: f64,
    pub max_latency_ms: u128,
    pub error_rate: f64,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct BridgeMetricsSnapshot {
    pub total_calls: u64,
    pub total_errors: u64,
    pub endpoints: BTreeMap<String, BridgeEndpointMetrics>,
}

impl BridgeMetrics {
    fn record(&mut self, method: &str, latency: Duration, success: bool) {
        self.total_calls = self.total_calls.saturating_add(1);
        if !success {
            self.total_errors = self.total_errors.saturating_add(1);
        }

        let endpoint = self.endpoints.entry(method.to_string()).or_default();
        endpoint.calls = endpoint.calls.saturating_add(1);
        if !success {
            endpoint.errors = endpoint.errors.saturating_add(1);
        }
        let latency_ms = latency.as_millis();
        endpoint.total_latency_ms =
            endpoint.total_latency_ms.saturating_add(latency_ms);
        endpoint.max_latency_ms = endpoint.max_latency_ms.max(latency_ms);
    }

    fn snapshot(&self) -> BridgeMetricsSnapshot {
        let mut endpoints = BTreeMap::new();

        for (method, stats) in &self.endpoints {
            let avg_latency_ms = if stats.calls == 0 {
                0.0
            } else {
                stats.total_latency_ms as f64 / stats.calls as f64
            };
            let error_rate = if stats.calls == 0 {
                0.0
            } else {
                stats.errors as f64 / stats.calls as f64
            };
            endpoints.insert(
                method.clone(),
                BridgeEndpointMetrics {
                    calls: stats.calls,
                    errors: stats.errors,
                    avg_latency_ms,
                    max_latency_ms: stats.max_latency_ms,
                    error_rate,
                },
            );
        }

        BridgeMetricsSnapshot {
            total_calls: self.total_calls,
            total_errors: self.total_errors,
            endpoints,
        }
    }
}

pub struct PythonBridge {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: Option<tokio::process::ChildStdout>,
    next_request_id: u64,
    strict_contract: bool,
    metrics: BridgeMetrics,
    metrics_path: PathBuf,
}

pub fn default_bundled_binary_name() -> &'static str {
    if cfg!(windows) {
        "ownstack_backend.exe"
    } else {
        "ownstack_backend"
    }
}

fn resolve_bundled_path(config: &BridgeLaunchConfig) -> PathBuf {
    if let Ok(path) = std::env::var("OWNSTACK_BRIDGE_BIN") {
        return PathBuf::from(path);
    }
    if let Some(path) = &config.bundled_path {
        return path.clone();
    }
    config.workspace.join(default_bundled_binary_name())
}

fn resolve_python_root(config: &BridgeLaunchConfig) -> PathBuf {
    if let Ok(path) = std::env::var("OWNSTACK_BRIDGE_PYTHON_ROOT") {
        return PathBuf::from(path);
    }
    if let Some(path) = &config.python_root {
        return path.clone();
    }
    config.workspace.clone()
}

fn resolve_dev_script_path(config: &BridgeLaunchConfig) -> PathBuf {
    resolve_python_root(config)
        .join("app")
        .join("bridge_rpc.py")
}

fn resolve_metrics_path(config: &BridgeLaunchConfig) -> PathBuf {
    if let Ok(path) = std::env::var("OWNSTACK_BRIDGE_METRICS_PATH") {
        return PathBuf::from(path);
    }

    config
        .workspace
        .join(".ownstack")
        .join("python_bridge_metrics.jsonl")
}

fn strict_contract_enabled() -> bool {
    match std::env::var("OWNSTACK_BRIDGE_STRICT_CONTRACT") {
        Ok(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => true,
    }
}

fn resolve_launch_command(
    config: &BridgeLaunchConfig,
) -> Result<(PathBuf, Vec<String>), BridgeError> {
    let mode = config.effective_mode();
    let bundled = resolve_bundled_path(config);
    let script = resolve_dev_script_path(config);

    match mode {
        BridgeRuntimeMode::BundledExecutable => {
            if !bundled.exists() {
                return Err(BridgeError::ProcessSpawn(format!(
                    "Bundled backend not found at {:?}",
                    bundled
                )));
            }
            Ok((bundled, Vec::new()))
        }
        BridgeRuntimeMode::DevPythonScript => {
            if !script.exists() {
                return Err(BridgeError::ProcessSpawn(format!(
                    "bridge_rpc.py not found at {:?}",
                    script
                )));
            }
            Ok((
                PathBuf::from("python"),
                vec![script.to_string_lossy().to_string()],
            ))
        }
        BridgeRuntimeMode::Auto => {
            if bundled.exists() {
                return Ok((bundled, Vec::new()));
            }
            if script.exists() {
                return Ok((
                    PathBuf::from("python"),
                    vec![script.to_string_lossy().to_string()],
                ));
            }
            Err(BridgeError::ProcessSpawn(format!(
                "Could not resolve bridge launch target. Missing bundled binary {:?} and script {:?}",
                bundled, script
            )))
        }
    }
}

fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

impl PythonBridge {
    pub async fn start(config: BridgeLaunchConfig) -> Result<Self, BridgeError> {
        let (command, args) = resolve_launch_command(&config)?;

        let mut child = Command::new(&command)
            .args(&args)
            .current_dir(&config.workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                BridgeError::ProcessSpawn(format!(
                    "Failed to spawn {:?} with args {:?}: {}",
                    command, args, e
                ))
            })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            BridgeError::ProcessSpawn("Failed to capture stdin".to_string())
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            BridgeError::ProcessSpawn("Failed to capture stdout".to_string())
        })?;

        Ok(Self {
            child,
            stdin,
            stdout: Some(stdout),
            next_request_id: 1,
            strict_contract: strict_contract_enabled(),
            metrics: BridgeMetrics::default(),
            metrics_path: resolve_metrics_path(&config),
        })
    }

    pub fn take_stdout(
        &mut self,
    ) -> Result<BufReader<tokio::process::ChildStdout>, BridgeError> {
        let stdout = self
            .stdout
            .take()
            .ok_or_else(|| BridgeError::Io("stdout already taken".to_string()))?;
        Ok(BufReader::new(stdout))
    }

    pub fn metrics_snapshot(&self) -> BridgeMetricsSnapshot {
        self.metrics.snapshot()
    }

    fn next_request_id(&mut self) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        request_id
    }

    async fn record_metric(
        &mut self,
        method: &str,
        latency: Duration,
        error: Option<String>,
    ) {
        let success = error.is_none();
        self.metrics.record(method, latency, success);

        let metric_event = BridgeMetricEvent {
            timestamp_ms: unix_timestamp_ms(),
            method: method.to_string(),
            success,
            latency_ms: latency.as_millis(),
            error,
        };

        if let Err(write_error) = self.append_metric_event(&metric_event).await {
            tracing::warn!(
                method = method,
                error = %write_error,
                "Failed to persist Python bridge metric event"
            );
        }
    }

    async fn append_metric_event(
        &self,
        event: &BridgeMetricEvent,
    ) -> Result<(), BridgeError> {
        if let Some(parent_dir) = self.metrics_path.parent() {
            tokio::fs::create_dir_all(parent_dir)
                .await
                .map_err(|e| BridgeError::Io(e.to_string()))?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.metrics_path)
            .await
            .map_err(|e| BridgeError::Io(e.to_string()))?;

        let payload = serde_json::to_vec(event)
            .map_err(|e| BridgeError::Serialization(e.to_string()))?;

        file.write_all(&payload)
            .await
            .map_err(|e| BridgeError::Io(e.to_string()))?;
        file.write_all(b"\n")
            .await
            .map_err(|e| BridgeError::Io(e.to_string()))?;

        Ok(())
    }

    async fn send_request_internal(
        &mut self,
        request: &BridgeRequest,
    ) -> Result<serde_json::Value, BridgeError> {
        let request_str = serde_json::to_string(request)
            .map_err(|e| BridgeError::Serialization(e.to_string()))?;

        self.stdin
            .write_all(request_str.as_bytes())
            .await
            .map_err(|e| BridgeError::Io(e.to_string()))?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|e| BridgeError::Io(e.to_string()))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| BridgeError::Io(e.to_string()))?;

        let stdout = self.stdout.as_mut().ok_or_else(|| {
            BridgeError::Io("stdout already taken by external reader".to_string())
        })?;
        let mut reader = BufReader::new(stdout);
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .await
            .map_err(|e| BridgeError::Io(e.to_string()))?;

        if response_line.trim().is_empty() {
            return Err(BridgeError::RpcError(
                "Python bridge returned an empty response".to_string(),
            ));
        }

        let response: BridgeResponse = serde_json::from_str(&response_line)
            .map_err(|e| BridgeError::Deserialization(e.to_string()))?;

        if response.jsonrpc != JSON_RPC_VERSION {
            return Err(BridgeError::RpcError(format!(
                "Unsupported jsonrpc version '{}'",
                response.jsonrpc
            )));
        }

        if response.id != Some(request.id) {
            return Err(BridgeError::RpcError(format!(
                "Response id mismatch: expected {}, got {:?}",
                request.id, response.id
            )));
        }

        match response.contract {
            Some(contract) => {
                if !contract.is_current() {
                    return Err(BridgeError::RpcError(format!(
                        "Bridge contract mismatch: expected {}@{}, got {}@{}",
                        BRIDGE_CONTRACT_NAME,
                        BRIDGE_CONTRACT_VERSION,
                        contract.name,
                        contract.version
                    )));
                }
            }
            None if self.strict_contract => {
                return Err(BridgeError::RpcError(
                    "Missing bridge contract metadata in response".to_string(),
                ));
            }
            None => {
                tracing::warn!(
                    method = request.method.as_str(),
                    "Received legacy bridge response without contract metadata"
                );
            }
        }

        if let Some(error) = response.error {
            return Err(BridgeError::RpcError(error.to_string()));
        }

        response.result.ok_or_else(|| {
            BridgeError::RpcError("No result in response".to_string())
        })
    }

    pub async fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, BridgeError> {
        if method.trim().is_empty() {
            return Err(BridgeError::Serialization(
                "Method must not be empty".to_string(),
            ));
        }

        let request_id = self.next_request_id();
        let request = BridgeRequest::new(request_id, method.to_string(), params);

        let start = Instant::now();
        let result = self.send_request_internal(&request).await;
        let latency = start.elapsed();

        match &result {
            Ok(_) => self.record_metric(method, latency, None).await,
            Err(error) => {
                self.record_metric(method, latency, Some(error.to_string()))
                    .await
            }
        }

        result
    }

    pub async fn shutdown(mut self) -> Result<(), BridgeError> {
        let _ = self.child.kill().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command as StdCommand;
    use tempfile::tempdir;

    #[test]
    fn test_request_serialization() {
        let req = BridgeRequest::new(
            123,
            "test_method".to_string(),
            serde_json::json!({"key": "value"}),
        );
        let json = serde_json::to_string(&req).expect("serialize request");
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":123"));
        assert!(json.contains("\"method\":\"test_method\""));
        assert!(json.contains("\"contract\""));
    }

    #[test]
    fn test_metrics_recording() {
        let mut metrics = BridgeMetrics::default();

        metrics.record("tools.exec", Duration::from_millis(120), true);
        metrics.record("tools.exec", Duration::from_millis(80), false);
        metrics.record("planner.plan", Duration::from_millis(20), true);

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.total_calls, 3);
        assert_eq!(snapshot.total_errors, 1);

        let exec_metrics = snapshot
            .endpoints
            .get("tools.exec")
            .expect("tools.exec metrics");
        assert_eq!(exec_metrics.calls, 2);
        assert_eq!(exec_metrics.errors, 1);
        assert_eq!(exec_metrics.max_latency_ms, 120);
    }

    #[test]
    fn test_auto_mode_prefers_bundled_when_present() {
        let dir = tempdir().expect("tempdir");
        let exe = dir.path().join(default_bundled_binary_name());
        fs::write(&exe, b"placeholder").expect("write bundled placeholder");

        let cfg = BridgeLaunchConfig::from_workspace(dir.path().to_path_buf());
        let (cmd, args) = resolve_launch_command(&cfg).expect("resolve bundled");
        assert_eq!(cmd, exe);
        assert!(args.is_empty());
    }

    #[tokio::test]
    async fn test_bridge_start_failure_missing_target() {
        let dir = tempdir().expect("tempdir");
        let config = BridgeLaunchConfig::from_workspace(dir.path().to_path_buf());
        let result = PythonBridge::start(config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_request_records_metrics_jsonl() {
        if StdCommand::new("python").arg("--version").output().is_err() {
            return;
        }

        let workspace = tempdir().expect("tempdir");
        let app_dir = workspace.path().join("app");
        fs::create_dir_all(&app_dir).expect("create app dir");

        let bridge_script = app_dir.join("bridge_rpc.py");
        fs::write(
            &bridge_script,
            r#"import json, sys
CONTRACT = {"name":"ownstack.bridge.jsonio","version":1}
for line in sys.stdin:
    req = json.loads(line)
    method = req.get("method")
    if method == "fail":
        res = {"jsonrpc":"2.0","id":req.get("id"),"result":None,"error":{"code":-32000,"message":"forced failure"},"contract":CONTRACT}
    else:
        res = {"jsonrpc":"2.0","id":req.get("id"),"result":{"ok":True,"method":method},"error":None,"contract":CONTRACT}
    sys.stdout.write(json.dumps(res) + "\n")
    sys.stdout.flush()
"#,
        )
        .expect("write bridge script");

        let config = BridgeLaunchConfig {
            mode: BridgeRuntimeMode::DevPythonScript,
            workspace: workspace.path().to_path_buf(),
            python_root: Some(workspace.path().to_path_buf()),
            bundled_path: None,
        };

        let mut bridge = PythonBridge::start(config).await.expect("start bridge");

        let ok_result = bridge
            .send_request("tools.exec", serde_json::json!({"command":"echo ok"}))
            .await
            .expect("successful response");
        assert_eq!(ok_result.get("ok").and_then(|v| v.as_bool()), Some(true));

        let fail_result = bridge.send_request("fail", serde_json::json!({})).await;
        assert!(fail_result.is_err());

        let metrics_snapshot = bridge.metrics_snapshot();
        assert_eq!(metrics_snapshot.total_calls, 2);
        assert_eq!(metrics_snapshot.total_errors, 1);

        let metrics_path = std::env::var("OWNSTACK_BRIDGE_METRICS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                workspace
                    .path()
                    .join(".ownstack")
                    .join("python_bridge_metrics.jsonl")
            });
        let metrics_content =
            fs::read_to_string(metrics_path).expect("read metrics file");
        let line_count = metrics_content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        if std::env::var("OWNSTACK_BRIDGE_METRICS_PATH").is_ok() {
            assert!(line_count >= 2);
        } else {
            assert_eq!(line_count, 2);
        }

        bridge.shutdown().await.expect("shutdown bridge");
    }
}
