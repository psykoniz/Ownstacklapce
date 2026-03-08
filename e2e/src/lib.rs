//! OwnStack IDE — E2E test client library.
//!
//! Provides `IdeProcess` to launch the IDE in E2E mode and `E2eClient` to
//! drive it via the JSON-RPC control server.

use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

// ── IDE process management ───────────────────────────────────────────────────

/// Wraps a running IDE process started in `--e2e` mode.
pub struct IdeProcess {
    child: Child,
    pub port: u16,
}

impl IdeProcess {
    /// Launch the IDE binary in E2E mode, wait for `E2E_READY:<port>`,
    /// and return the handle.
    ///
    /// `binary` — path to the `ownstack-ide` binary.
    /// `workspace` — optional workspace directory to open.
    /// `extra_env` — additional environment variables.
    pub fn launch(
        binary: &Path,
        workspace: Option<&Path>,
        extra_env: Vec<(&str, &str)>,
    ) -> Result<Self, String> {
        let mut cmd = Command::new(binary);
        cmd.env("OWNSTACK_E2E", "1")
            .env("OWNSTACK_E2E_PORT", "0")
            .stdout(Stdio::piped())
            // Avoid deadlocks from unconsumed stderr when the IDE logs heavily.
            .stderr(Stdio::inherit());

        for (k, v) in &extra_env {
            cmd.env(k, v);
        }

        if let Some(ws) = workspace {
            cmd.arg(ws);
        }

        // Use --wait so the process doesn't fork
        cmd.arg("--wait");

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn IDE: {e}"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "stdout was not piped".to_string())?;
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u16, String>>();

        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        eprintln!("[e2e-client] IDE stdout: {line}");
                        if let Some(p) = line.strip_prefix("E2E_READY:") {
                            let parsed = p
                                .trim()
                                .parse::<u16>()
                                .map_err(|e| format!("bad port: {e}"));
                            let _ = ready_tx.send(parsed);
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("reading stdout: {e}")));
                        return;
                    }
                }
            }
            let _ =
                ready_tx.send(Err("IDE stdout closed before E2E_READY".to_string()));
        });

        let port = match ready_rx.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(port)) => port,
            Ok(Err(e)) => return Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err("IDE did not print E2E_READY within 30s".to_string());
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("stdout reader thread disconnected".to_string());
            }
        };

        Ok(Self { child, port })
    }

    /// Kill the IDE process.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for IdeProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

// ── JSON-RPC client ──────────────────────────────────────────────────────────

/// Synchronous client that talks to the IDE's E2E control server.
pub struct E2eClient {
    base_url: String,
    http: reqwest::blocking::Client,
    next_id: u64,
}

impl E2eClient {
    pub fn new(port: u16) -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
            next_id: 1,
        }
    }

    /// Send a JSON-RPC call and return the "result" field.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;

        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let resp = self
            .http
            .post(&self.base_url)
            .json(&body)
            .send()
            .map_err(|e| format!("HTTP error: {e}"))?;

        let json: Value =
            resp.json().map_err(|e| format!("JSON parse error: {e}"))?;

        if let Some(err) = json.get("result").and_then(|r| r.get("error")) {
            return Err(format!("server error: {err}"));
        }

        json.get("result")
            .cloned()
            .ok_or_else(|| format!("no 'result' in response: {json}"))
    }

    // ── Convenience methods ──────────────────────────────────────────────

    pub fn ping(&mut self) -> Result<Value, String> {
        self.call("ping", json!({}))
    }

    pub fn open_file(&mut self, path: &Path) -> Result<Value, String> {
        self.call("open_file", json!({ "path": path.display().to_string() }))
    }

    pub fn editor_set_text(&mut self, text: &str) -> Result<Value, String> {
        self.call("editor_set_text", json!({ "text": text }))
    }

    pub fn save(&mut self) -> Result<Value, String> {
        self.call("save", json!({}))
    }

    pub fn undo(&mut self) -> Result<Value, String> {
        self.call("undo", json!({}))
    }

    pub fn redo(&mut self) -> Result<Value, String> {
        self.call("redo", json!({}))
    }

    pub fn find_replace(
        &mut self,
        find: &str,
        replace: &str,
    ) -> Result<Value, String> {
        self.call("find_replace", json!({ "find": find, "replace": replace }))
    }

    pub fn run_command(&mut self, name: &str) -> Result<Value, String> {
        self.call("run_command", json!({ "name": name }))
    }

    pub fn get_state(&mut self) -> Result<Value, String> {
        self.call("get_state", json!({}))
    }

    pub fn get_diagnostics(&mut self) -> Result<Value, String> {
        self.call("get_diagnostics", json!({}))
    }

    pub fn get_editor_text(&mut self) -> Result<Value, String> {
        self.call("get_editor_text", json!({}))
    }

    pub fn wait_idle(&mut self, timeout_ms: u64) -> Result<Value, String> {
        self.call("wait_idle", json!({ "timeout_ms": timeout_ms }))
    }

    pub fn screenshot(&mut self, path: &str) -> Result<Value, String> {
        self.call("screenshot", json!({ "path": path }))
    }

    /// Convenience: wait for the IDE to be idle, retrying ping if needed.
    pub fn wait_ready(&mut self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.ping() {
                Ok(_) => {
                    // Now wait for actual idle
                    let remaining =
                        deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Ok(());
                    }
                    let _ = self.wait_idle(remaining.as_millis() as u64);
                    return Ok(());
                }
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(e) => return Err(format!("IDE not ready: {e}")),
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Find the IDE binary. Looks for `target/debug/ownstack-ide` relative to
/// the workspace root.
pub fn find_ide_binary() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let root = PathBuf::from(manifest_dir).parent().unwrap().to_path_buf();
    let binary = root.join("target/debug/ownstack-ide");
    if !binary.exists() {
        panic!(
            "IDE binary not found at {}. Build it first: cargo build -p lapce-app",
            binary.display()
        );
    }
    binary
}

/// Path to the fixture project.
pub fn fixtures_project() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir).join("fixtures/project")
}
