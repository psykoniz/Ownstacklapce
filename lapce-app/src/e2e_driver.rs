//! E2E Driver — lightweight JSON-RPC control server for deterministic E2E testing.
//!
//! When the IDE is started with `--e2e` (or env `OWNSTACK_E2E=1`), this module
//! binds a tiny HTTP server on `127.0.0.1:<port>` and exposes a high-level API
//! for test clients to drive the IDE: open files, set text, save, undo/redo, etc.
//!
//! The server is *synchronous* and runs on its own thread to avoid pulling in
//! extra async dependencies. Commands are forwarded to the UI thread via
//! a crossbeam channel + `create_signal_from_channel` reactive bridge.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write as IoWrite},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    rc::Rc,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Sender, bounded};
use floem::reactive::{SignalGet, SignalWith};
use serde_json::{Value, json};

use crate::{command::InternalCommand, doc::DocContent, window_tab::WindowTabData};

type QueryFn = Box<dyn FnOnce(&Rc<WindowTabData>) -> Value + Send>;

// ── Public handle exposed to the app ─────────────────────────────────────────

/// Opaque handle that the application holds to keep the E2E server alive.
/// Dropping it signals the server thread to stop.
pub struct E2eHandle {
    _alive: std::sync::Arc<AtomicBool>,
    pub port: u16,
}

/// Start the E2E control server.  Returns the handle (keep it alive) and the
/// port that was actually bound.
pub fn start_e2e_server(port: u16) -> E2eHandle {
    let alive = std::sync::Arc::new(AtomicBool::new(true));
    let alive_clone = alive.clone();

    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .expect("E2E: failed to bind TCP listener");
    listener
        .set_nonblocking(true)
        .expect("E2E: failed to set non-blocking");
    let actual_port = listener.local_addr().unwrap().port();

    // Print the E2E ready line that test clients parse.
    // Format: E2E_READY:<port>
    println!("E2E_READY:{actual_port}");
    eprintln!("[e2e] Control server listening on 127.0.0.1:{actual_port}");

    thread::Builder::new()
        .name("e2e-control-server".into())
        .spawn(move || {
            server_loop(listener, alive_clone);
        })
        .expect("E2E: failed to spawn server thread");

    E2eHandle {
        _alive: alive,
        port: actual_port,
    }
}

// ── Global state: the WindowTabData handle set by the app after init ─────────

static TAB_DATA: once_cell::sync::OnceCell<Mutex<Option<E2eTabHandle>>> =
    once_cell::sync::OnceCell::new();

/// Thread-safe wrapper: we store a Sender for cross-thread query dispatch.
/// All IDE interactions go through closures executed on the UI thread.
struct E2eTabHandle {
    /// Query channel: send (closure, reply_tx) to the UI thread.
    query_tx: Sender<(QueryFn, Sender<Value>)>,
}

// E2eTabHandle is Send+Sync because it only contains the query sender
// (the closure + reply sender are Send).
unsafe impl Send for E2eTabHandle {}
unsafe impl Sync for E2eTabHandle {}

// Global slot for the tab reference — only accessed from the UI thread.
// We use a thread-local because Rc<WindowTabData> is !Send.
thread_local! {
    static UI_TAB: std::cell::RefCell<Option<Rc<WindowTabData>>> = const { std::cell::RefCell::new(None) };
}

/// Called from the UI thread once WindowTabData is ready.
///
/// Sets up a reactive bridge: queries arrive on a crossbeam channel, which
/// is converted to a Floem reactive signal via `create_signal_from_channel`.
/// An effect watches the signal and executes queries on the UI thread.
pub fn register_tab_data(tab: &Rc<WindowTabData>) {
    use floem::ext_event::create_signal_from_channel;
    use floem::reactive::create_effect;

    let (query_tx, query_rx) = bounded::<(QueryFn, Sender<Value>)>(64);

    // Store the tab in thread-local (UI thread only)
    UI_TAB.with(|cell| {
        *cell.borrow_mut() = Some(tab.clone());
    });

    // Channel for pinging the UI thread via reactive signal.
    // Uses std::sync::mpsc as required by create_signal_from_channel.
    let (ping_tx, ping_rx) = std::sync::mpsc::channel::<()>();

    // Create a Floem reactive signal from the ping channel.
    // This integrates with Floem's event loop natively — no ext_action needed.
    let ping_signal = create_signal_from_channel(ping_rx);

    // Global queue: the bridge thread pushes queries here, the effect pops them.
    #[allow(clippy::type_complexity)]
    let pending: std::sync::Arc<Mutex<Vec<(QueryFn, Sender<Value>)>>> =
        std::sync::Arc::new(Mutex::new(Vec::new()));
    let pending_for_effect = pending.clone();

    // Reactive effect: fires on the UI thread whenever ping_signal changes.
    create_effect(move |_| {
        if ping_signal.get().is_some() {
            // Drain all pending queries
            let queries: Vec<_> =
                pending_for_effect.lock().unwrap().drain(..).collect();
            UI_TAB.with(|cell| {
                if let Some(tab) = cell.borrow().as_ref() {
                    for (f, reply_tx) in queries {
                        let result = f(tab);
                        let _ = reply_tx.send(result);
                    }
                }
            });
        }
    });

    // Bridge thread: receives queries from the HTTP server thread,
    // pushes them into the shared queue, and pings the reactive signal.
    thread::Builder::new()
        .name("e2e-query-bridge".into())
        .spawn(move || {
            while let Ok((f, reply_tx)) = query_rx.recv() {
                pending.lock().unwrap().push((f, reply_tx));
                // Ping the UI thread via the reactive channel
                let _ = ping_tx.send(());
            }
        })
        .expect("E2E: failed to spawn query bridge");

    let handle = E2eTabHandle { query_tx };

    TAB_DATA
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .replace(handle);
    eprintln!("[e2e] WindowTabData registered — driver ready");
}

// ── HTTP server loop ─────────────────────────────────────────────────────────

fn server_loop(listener: TcpListener, alive: std::sync::Arc<AtomicBool>) {
    while alive.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                if let Err(e) = handle_connection(stream) {
                    eprintln!("[e2e] connection error: {e}");
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                eprintln!("[e2e] accept error: {e}");
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn handle_connection(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;

    let mut reader = BufReader::new(&stream);

    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let mut content_length: usize = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        let header = header.trim();
        if header.is_empty() {
            break;
        }
        // HTTP header names are case-insensitive: curl sends "Content-Length"
        // while reqwest/hyper sends "content-length". Match either form.
        if let Some((name, val)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = val.trim().parse().unwrap_or(0);
            }
        }
    }

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;

    let request: Value = serde_json::from_slice(&body).unwrap_or(json!({}));

    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(json!({}));
    let id = request.get("id").cloned().unwrap_or(json!(null));

    let result = dispatch_method(method, params);

    let response = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });

    let response_body = serde_json::to_vec(&response).unwrap();
    let http_response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response_body.len()
    );
    stream.write_all(http_response.as_bytes())?;
    stream.write_all(&response_body)?;
    stream.flush()?;
    Ok(())
}

// ── Command dispatch ─────────────────────────────────────────────────────────

fn dispatch_method(method: &str, params: Value) -> Value {
    match method {
        "ping" => json!({ "status": "ok", "message": "pong" }),
        "open_workspace" => cmd_open_workspace(params),
        "open_file" => cmd_open_file(params),
        "editor_set_text" => cmd_editor_set_text(params),
        "save" => cmd_save(params),
        "undo" => cmd_undo(params),
        "redo" => cmd_redo(params),
        "find_replace" => cmd_find_replace(params),
        "run_command" => cmd_run_command(params),
        "get_state" => cmd_get_state(params),
        "get_diagnostics" => cmd_get_diagnostics(params),
        "get_editor_text" => cmd_get_editor_text(params),
        "wait_idle" => cmd_wait_idle(params),
        "screenshot" => cmd_screenshot(params),
        _ => json!({ "error": format!("unknown method: {method}") }),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn with_handle<F: FnOnce(&E2eTabHandle) -> Value>(f: F) -> Value {
    let cell = TAB_DATA.get_or_init(|| Mutex::new(None));
    let guard = cell.lock().unwrap();
    match guard.as_ref() {
        Some(handle) => f(handle),
        None => {
            json!({ "error": "IDE not yet initialized (WindowTabData not registered)" })
        }
    }
}

fn query_ui<F>(f: F) -> Value
where
    F: FnOnce(&Rc<WindowTabData>) -> Value + Send + 'static,
{
    with_handle(|handle| {
        let (reply_tx, reply_rx) = bounded(1);
        let boxed: QueryFn = Box::new(f);
        match handle
            .query_tx
            .send_timeout((boxed, reply_tx), Duration::from_secs(5))
        {
            Ok(()) => match reply_rx.recv_timeout(Duration::from_secs(10)) {
                Ok(val) => val,
                Err(e) => json!({ "error": format!("query timeout: {e}") }),
            },
            Err(e) => json!({ "error": format!("query send failed: {e}") }),
        }
    })
}

// ── Command implementations ──────────────────────────────────────────────────

fn cmd_open_workspace(params: Value) -> Value {
    let path = match params.get("path").and_then(Value::as_str) {
        Some(p) => PathBuf::from(p),
        None => return json!({ "error": "missing 'path' param" }),
    };
    if !path.is_dir() {
        return json!({ "error": format!("not a directory: {}", path.display()) });
    }
    query_ui(move |tab| {
        let ws_path = tab.workspace.path.as_ref().map(|p| p.display().to_string());
        json!({
            "status": "ok",
            "note": "workspace should be set at launch via CLI arg",
            "current_workspace": ws_path,
        })
    })
}

fn cmd_open_file(params: Value) -> Value {
    let path = match params.get("path").and_then(Value::as_str) {
        Some(p) => PathBuf::from(p),
        None => return json!({ "error": "missing 'path' param" }),
    };
    query_ui(move |tab| {
        tab.common
            .internal_command
            .send(InternalCommand::OpenFile { path });
        json!({ "status": "ok" })
    })
}

fn cmd_editor_set_text(params: Value) -> Value {
    let text = match params.get("text").and_then(Value::as_str) {
        Some(t) => t.to_string(),
        None => return json!({ "error": "missing 'text' param" }),
    };

    query_ui(move |tab| {
        if let Some(editor) = tab.main_split.active_editor.get_untracked() {
            let doc = editor.doc();
            let buf_len = doc.buffer.with_untracked(|b| {
                use lapce_core::buffer::rope_text::RopeText;
                b.len()
            });
            // Use do_raw_edit with a Selection covering the entire buffer
            use lapce_core::editor::EditType;
            use lapce_core::selection::Selection;
            let sel = Selection::region(0, buf_len);
            doc.do_raw_edit(&[(&sel, text.as_str())], EditType::Other);
            json!({ "status": "ok" })
        } else {
            json!({ "error": "no active editor" })
        }
    })
}

fn cmd_save(_params: Value) -> Value {
    query_ui(|tab| {
        if let Some(editor) = tab.main_split.active_editor.get_untracked() {
            editor.save(false, || {});
            json!({ "status": "ok" })
        } else {
            json!({ "error": "no active editor" })
        }
    })
}

fn cmd_undo(_params: Value) -> Value {
    query_ui(|tab| {
        use crate::command::{CommandKind, LapceCommand};
        use lapce_core::command::EditCommand;
        tab.run_lapce_command(LapceCommand {
            kind: CommandKind::Edit(EditCommand::Undo),
            data: None,
        });
        json!({ "status": "ok" })
    })
}

fn cmd_redo(_params: Value) -> Value {
    query_ui(|tab| {
        use crate::command::{CommandKind, LapceCommand};
        use lapce_core::command::EditCommand;
        tab.run_lapce_command(LapceCommand {
            kind: CommandKind::Edit(EditCommand::Redo),
            data: None,
        });
        json!({ "status": "ok" })
    })
}

fn cmd_find_replace(params: Value) -> Value {
    let find = match params.get("find").and_then(Value::as_str) {
        Some(f) => f.to_string(),
        None => return json!({ "error": "missing 'find' param" }),
    };
    let replace = match params.get("replace").and_then(Value::as_str) {
        Some(r) => r.to_string(),
        None => return json!({ "error": "missing 'replace' param" }),
    };

    query_ui(move |tab| {
        if let Some(editor) = tab.main_split.active_editor.get_untracked() {
            let doc = editor.doc();
            let (content, buf_len) = doc.buffer.with_untracked(|b| {
                use lapce_core::buffer::rope_text::RopeText;
                (b.to_string(), b.len())
            });
            let new_content = content.replace(&find, &replace);
            if new_content == content {
                return json!({ "status": "ok", "replacements": 0 });
            }
            let count = content.matches(&find).count();
            use lapce_core::editor::EditType;
            use lapce_core::selection::Selection;
            let sel = Selection::region(0, buf_len);
            doc.do_raw_edit(&[(&sel, new_content.as_str())], EditType::Other);
            json!({ "status": "ok", "replacements": count })
        } else {
            json!({ "error": "no active editor" })
        }
    })
}

fn cmd_run_command(params: Value) -> Value {
    let name = match params.get("name").and_then(Value::as_str) {
        Some(n) => n.to_string(),
        None => return json!({ "error": "missing 'name' param" }),
    };
    query_ui(move |tab| {
        use crate::command::LapceWorkbenchCommand;
        use std::str::FromStr;
        match LapceWorkbenchCommand::from_str(&name) {
            Ok(cmd) => {
                tab.run_workbench_command(cmd, None);
                json!({ "status": "ok" })
            }
            Err(_) => {
                json!({ "error": format!("unknown workbench command: {name}") })
            }
        }
    })
}

fn cmd_get_state(_params: Value) -> Value {
    query_ui(|tab| {
        let workspace_path =
            tab.workspace.path.as_ref().map(|p| p.display().to_string());

        let active_file = tab
            .main_split
            .active_editor
            .get_untracked()
            .map(|e| {
                let doc = e.doc();
                match doc.content.get_untracked() {
                    DocContent::File { path, .. } => path.display().to_string(),
                    DocContent::Scratch { name, .. } => format!("scratch:{name}"),
                    DocContent::Local => "local".to_string(),
                    DocContent::History(_) => "history".to_string(),
                }
            });

        let is_dirty = tab
            .main_split
            .active_editor
            .get_untracked()
            .map(|e| !e.doc().is_pristine())
            .unwrap_or(false);

        // Collect open editor paths
        let mut open_files = Vec::new();
        tab.main_split.docs.with_untracked(|docs| {
            for (path, _) in docs.iter() {
                open_files.push(path.display().to_string());
            }
        });

        // Explorer entries from file_explorer root
        let root = tab.file_explorer.root.get_untracked();
        let explorer_entries: Vec<String> = root
            .children
            .keys()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();

        json!({
            "workspace": workspace_path,
            "active_file": active_file,
            "is_dirty": is_dirty,
            "open_files": open_files,
            "explorer_entries": explorer_entries,
        })
    })
}

fn cmd_get_diagnostics(_params: Value) -> Value {
    query_ui(|tab| {
        let mut result: HashMap<String, Vec<Value>> = HashMap::new();
        tab.main_split.diagnostics.with_untracked(|diags| {
            for (path, diag_data) in diags.iter() {
                let items: Vec<Value> = diag_data
                    .diagnostics
                    .get_untracked()
                    .iter()
                    .map(|d| {
                        json!({
                            "message": d.message,
                            "severity": d.severity.map(|s| format!("{s:?}")),
                            "range": format!(
                                "{}:{}-{}:{}",
                                d.range.start.line,
                                d.range.start.character,
                                d.range.end.line,
                                d.range.end.character
                            ),
                        })
                    })
                    .collect();
                result.insert(path.display().to_string(), items);
            }
        });
        json!({ "diagnostics": result })
    })
}

fn cmd_get_editor_text(_params: Value) -> Value {
    query_ui(|tab| {
        if let Some(editor) = tab.main_split.active_editor.get_untracked() {
            let doc = editor.doc();
            let text = doc.buffer.with_untracked(|b| b.to_string());
            let path =
                if let DocContent::File { path, .. } = doc.content.get_untracked() {
                    Some(path.display().to_string())
                } else {
                    None
                };
            json!({ "text": text, "path": path })
        } else {
            json!({ "error": "no active editor" })
        }
    })
}

fn cmd_wait_idle(params: Value) -> Value {
    let timeout_ms = params
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(5000);
    let start = Instant::now();
    let deadline = start + Duration::from_millis(timeout_ms);

    let mut last_state = String::new();
    let mut stable_count = 0;

    while Instant::now() < deadline {
        let state = query_ui(|tab| {
            let active = tab
                .main_split
                .active_editor
                .get_untracked()
                .map(|e| {
                    let doc = e.doc();
                    let pristine = doc.is_pristine();
                    let loaded = doc.loaded.get_untracked();
                    format!("p={pristine},l={loaded}")
                })
                .unwrap_or_default();
            json!({ "state_hash": active })
        });

        let state_str = state.to_string();
        if state_str == last_state {
            stable_count += 1;
            if stable_count >= 3 {
                let elapsed = start.elapsed().as_millis();
                return json!({ "status": "idle", "elapsed_ms": elapsed });
            }
        } else {
            stable_count = 0;
        }
        last_state = state_str;
        thread::sleep(Duration::from_millis(100));
    }

    json!({ "status": "timeout", "message": "IDE did not reach idle within timeout" })
}

fn cmd_screenshot(params: Value) -> Value {
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("/tmp/e2e_screenshot.png");

    if path.trim().is_empty() {
        return json!({ "error": "path must not be empty" });
    }

    let xwd_output = match std::process::Command::new("xwd")
        .args(["-root", "-silent"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return json!({ "error": format!("xwd failed: {stderr}") });
        }
        Err(e) => {
            return json!({ "error": format!("failed to launch xwd: {e}") });
        }
    };

    let mut convert = match std::process::Command::new("convert")
        .arg("xwd:-")
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            return json!({ "error": format!("failed to launch convert: {e}") });
        }
    };

    if let Some(stdin) = convert.stdin.as_mut() {
        if let Err(e) = stdin.write_all(&xwd_output.stdout) {
            return json!({ "error": format!("failed to pipe xwd output to convert: {e}") });
        }
    } else {
        return json!({ "error": "failed to open convert stdin" });
    }

    match convert.wait_with_output() {
        Ok(output) if output.status.success() => {
            json!({ "status": "ok", "path": path })
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            json!({ "error": format!("convert failed: {stderr}") })
        }
        Err(e) => json!({ "error": format!("failed waiting for convert: {e}") }),
    }
}
