use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

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

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeRequest {
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeResponse {
    pub id: u64,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
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

pub struct PythonBridge {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: Option<tokio::process::ChildStdout>,
}

fn default_bundled_binary_name() -> &'static str {
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

    pub async fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, BridgeError> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let request_str = serde_json::to_string(&request)
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

        let response: serde_json::Value = serde_json::from_str(&response_line)
            .map_err(|e| BridgeError::Deserialization(e.to_string()))?;

        if let Some(error) = response.get("error") {
            if !error.is_null() {
                return Err(BridgeError::RpcError(error.to_string()));
            }
        }

        response.get("result").cloned().ok_or_else(|| {
            BridgeError::RpcError("No result in response".to_string())
        })
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
    use tempfile::tempdir;

    #[test]
    fn test_request_serialization() {
        let req = BridgeRequest {
            id: 123,
            method: "test_method".to_string(),
            params: serde_json::json!({"key": "value"}),
        };
        let json = serde_json::to_string(&req).expect("serialize request");
        assert!(json.contains("\"id\":123"));
        assert!(json.contains("\"method\":\"test_method\""));
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
}
