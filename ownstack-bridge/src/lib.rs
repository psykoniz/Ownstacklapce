use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BridgeError {
    #[error("Failed to spawn Python process: {0}")]
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

pub struct PythonBridge {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
}

impl PythonBridge {
    /// Start the Python bridge process
    pub async fn start(python_path: PathBuf, workspace: PathBuf) -> Result<Self, BridgeError> {
        // Locate the bridge_rpc.py entry point
        let script_path = python_path.join("app").join("bridge_rpc.py");
        
        if !script_path.exists() {
            return Err(BridgeError::ProcessSpawn(format!(
                "bridge_rpc.py not found at {:?}",
                script_path
            )));
        }

        // Spawn Python process with stdio pipes
        let mut child = Command::new("python")
            .arg(&script_path)
            .current_dir(&workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| BridgeError::ProcessSpawn(e.to_string()))?;

        let stdin = child.stdin.take().ok_or_else(|| {
            BridgeError::ProcessSpawn("Failed to capture stdin".to_string())
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            BridgeError::ProcessSpawn("Failed to capture stdout".to_string())
        })?;

        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }

    /// Send a JSON-RPC request to the Python bridge
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

        // Write request to stdin
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

        // Read response from stdout
        let mut reader = BufReader::new(&mut self.stdout);
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

        response
            .get("result")
            .cloned()
            .ok_or_else(|| BridgeError::RpcError("No result in response".to_string()))
    }

    /// Shutdown the bridge gracefully
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

    // ─── Serialization Tests ─────────────────────────────────────
    #[test]
    fn test_request_serialization() {
        let req = BridgeRequest {
            id: 123,
            method: "test_method".to_string(),
            params: serde_json::json!({"key": "value"}),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"id\":123"));
        assert!(json.contains("\"method\":\"test_method\""));
    }

    #[test]
    fn test_response_deserialization() {
        let json = r#"{"id": 456, "result": {"status": "ok"}, "error": null}"#;
        let res: BridgeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(res.id, 456);
        assert_eq!(res.result.unwrap().get("status").unwrap(), "ok");
        assert!(res.error.is_none());
    }

    #[test]
    fn test_response_with_error() {
        let json = r#"{"id": 789, "result": null, "error": "something went wrong"}"#;
        let res: BridgeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(res.error.unwrap(), "something went wrong");
    }

    // ─── Error Handling ──────────────────────────────────────────
    #[test]
    fn test_error_display() {
        let e = BridgeError::ProcessSpawn("failed to spawn".to_string());
        assert!(e.to_string().contains("failed to spawn"));

        let e2 = BridgeError::RpcError("remote failure".to_string());
        assert!(e2.to_string().contains("remote failure"));
    }

    // ─── Bridge Interaction (Integration) ────────────────────────
    #[tokio::test]
    async fn test_bridge_start_failure_missing_script() {
        let dir = tempdir().unwrap();
        let python_path = dir.path().to_path_buf();
        let result = PythonBridge::start(python_path, dir.path().to_path_buf()).await;
        assert!(result.is_err());
        if let Err(BridgeError::ProcessSpawn(msg)) = result {
            assert!(msg.contains("bridge_rpc.py not found"));
        } else {
            panic!("Expected ProcessSpawn error");
        }
    }

    /// This test creates a mock bridge_rpc.py that echoes input back
    #[tokio::test]
    async fn test_bridge_mock_interaction() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().join("app");
        fs::create_dir_all(&app_dir).unwrap();
        
        // Mock Python script that reads line and echoes it back as result
        let script_content = r#"
import sys
import json

for line in sys.stdin:
    try:
        req = json.loads(line)
        resp = {
            "jsonrpc": "2.0",
            "id": req.get("id", 1),
            "result": req.get("params", {}),
            "error": None
        }
        print(json.dumps(resp))
        sys.stdout.flush()
    except Exception as e:
        print(json.dumps({"error": str(e)}))
        sys.stdout.flush()
"#;
        let script_path = app_dir.join("bridge_rpc.py");
        fs::write(&script_path, script_content).unwrap();

        // Start bridge
        let mut bridge = PythonBridge::start(dir.path().to_path_buf(), dir.path().to_path_buf())
            .await
            .expect("Failed to start mock bridge");

        // Send request
        let params = serde_json::json!({"test": 123});
        let result = bridge.send_request("echo", params.clone()).await
            .expect("Failed to send request");

        assert_eq!(result, params);

        // Shutdown
        bridge.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_bridge_error_from_python() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().join("app");
        fs::create_dir_all(&app_dir).unwrap();
        
        let script_content = r#"
import sys
import json
print(json.dumps({"id": 1, "error": "mock error", "result": None}))
sys.stdout.flush()
"#;
        fs::write(app_dir.join("bridge_rpc.py"), script_content).unwrap();

        let mut bridge = PythonBridge::start(dir.path().to_path_buf(), dir.path().to_path_buf())
            .await
            .unwrap();

        let result = bridge.send_request("fail", serde_json::json!({})).await;
        assert!(result.is_err());
        if let Err(BridgeError::RpcError(msg)) = result {
            assert!(msg.contains("mock error"));
        } else {
            panic!("Expected RpcError");
        }
    }

    // ─── Edge Cases ─────────────────────────────────────────────
    #[test]
    fn test_serialize_large_params() {
        let req = BridgeRequest {
            id: 1,
            method: "large".to_string(),
            params: serde_json::json!({"data": "x".repeat(10000)}),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.len() > 10000);
    }

    // ─── Stress Tests ───────────────────────────────────────────
    #[tokio::test]
    async fn stress_test_rapid_requests() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().join("app");
        fs::create_dir_all(&app_dir).unwrap();
        
        let script_content = r#"
import sys
import json
for line in sys.stdin:
    req = json.loads(line)
    print(json.dumps({"id": req["id"], "result": "ok", "error": None}))
    sys.stdout.flush()
"#;
        fs::write(app_dir.join("bridge_rpc.py"), script_content).unwrap();

        let mut bridge = PythonBridge::start(dir.path().to_path_buf(), dir.path().to_path_buf())
            .await
            .unwrap();

        for _ in 0..100 {
            let res = bridge.send_request("test", serde_json::json!({})).await.unwrap();
            assert_eq!(res, "ok");
        }
    }
}

