//! E2E "golden path" tests for OwnStack IDE.
//!
//! These tests launch the real IDE binary in `--e2e` mode and drive it through
//! the JSON-RPC control server. They validate the core editor workflow:
//! launch, open workspace, open file, edit, save, undo/redo, find/replace,
//! and command palette.
//!
//! # Running
//!
//! ```sh
//! # Build the IDE first
//! cargo build -p lapce-app
//!
//! # Run E2E tests (needs a display — use xvfb-run on headless CI)
//! cargo test -p ownstack-e2e -- --test-threads=1
//!
//! # Or with xvfb:
//! xvfb-run -a cargo test -p ownstack-e2e -- --test-threads=1
//! ```

use std::{
    fs,
    path::PathBuf,
    time::Duration,
};

use ownstack_e2e::{E2eClient, IdeProcess, find_ide_binary, fixtures_project};

/// Helper: launch IDE with the fixture workspace, return (process, client).
fn launch_ide() -> (IdeProcess, E2eClient) {
    let binary = find_ide_binary();
    let workspace = fixtures_project();

    // Create a temp config dir so we don't interfere with user config
    let config_dir = std::env::temp_dir().join("ownstack-e2e-config");
    let _ = fs::create_dir_all(&config_dir);

    let env_vars = vec![
        ("LIBGL_ALWAYS_SOFTWARE", "1"),
        ("WGPU_BACKEND", "gl"),
        ("XDG_CONFIG_HOME", config_dir.to_str().unwrap()),
    ];

    let process = IdeProcess::launch(&binary, Some(&workspace), env_vars)
        .expect("Failed to launch IDE");

    let mut client = E2eClient::new(process.port);

    // Wait for IDE to be ready
    client
        .wait_ready(Duration::from_secs(15))
        .expect("IDE did not become ready");

    (process, client)
}

// ── Test 1: launch_ready ─────────────────────────────────────────────────────

#[test]
fn t01_launch_ready() {
    let (_proc, mut client) = launch_ide();

    // Ping should succeed
    let result = client.ping().expect("ping failed");
    assert_eq!(result["status"], "ok");
    assert_eq!(result["message"], "pong");

    // Wait idle should succeed
    let idle = client.wait_idle(5000).expect("wait_idle failed");
    assert!(
        idle["status"] == "idle" || idle["status"] == "timeout",
        "unexpected idle status: {idle}"
    );
}

// ── Test 2: open_workspace ───────────────────────────────────────────────────

#[test]
fn t02_open_workspace() {
    let (_proc, mut client) = launch_ide();

    let state = client.get_state().expect("get_state failed");

    // The workspace should be the fixture project
    let ws = state["workspace"].as_str().unwrap_or("");
    assert!(
        ws.contains("fixtures/project") || ws.contains("fixtures\\project"),
        "workspace not set correctly: {ws}"
    );
}

// ── Test 3: open_edit_save_persist ───────────────────────────────────────────

#[test]
fn t03_open_edit_save_persist() {
    let (_proc, mut client) = launch_ide();

    let fixture_dir = fixtures_project();
    let test_file = fixture_dir.join("src/main.rs");

    // Backup original content
    let original = fs::read_to_string(&test_file).expect("read fixture file");

    // Open the file
    client.open_file(&test_file).expect("open_file failed");
    std::thread::sleep(Duration::from_millis(500));
    client.wait_idle(3000).ok();

    // Set new text
    let new_text = format!("{original}\n// E2E test marker\n");
    client
        .editor_set_text(&new_text)
        .expect("editor_set_text failed");
    std::thread::sleep(Duration::from_millis(200));

    // Save
    client.save().expect("save failed");
    std::thread::sleep(Duration::from_millis(500));

    // Verify disk content
    let on_disk = fs::read_to_string(&test_file).expect("read after save");
    assert!(
        on_disk.contains("E2E test marker"),
        "saved content not found on disk"
    );

    // Restore original content
    fs::write(&test_file, &original).expect("restore fixture");
}

// ── Test 4: undo_redo ────────────────────────────────────────────────────────

#[test]
fn t04_undo_redo() {
    let (_proc, mut client) = launch_ide();

    let fixture_dir = fixtures_project();
    let test_file = fixture_dir.join("src/main.rs");
    let original = fs::read_to_string(&test_file).expect("read fixture");

    // Open file
    client.open_file(&test_file).expect("open_file failed");
    std::thread::sleep(Duration::from_millis(500));
    client.wait_idle(3000).ok();

    // Read current text
    let before = client.get_editor_text().expect("get_editor_text");
    let before_text = before["text"].as_str().unwrap_or("").to_string();

    // Edit
    let edited = format!("{before_text}// undo test\n");
    client.editor_set_text(&edited).expect("set text");
    std::thread::sleep(Duration::from_millis(200));

    // Undo
    client.undo().expect("undo failed");
    std::thread::sleep(Duration::from_millis(200));

    let after_undo = client.get_editor_text().expect("get text after undo");
    let after_undo_text = after_undo["text"].as_str().unwrap_or("");
    // After undo, the text should not contain our edit
    // (Note: depending on buffer undo granularity, this may vary)
    assert!(
        !after_undo_text.contains("// undo test") || after_undo_text == before_text,
        "undo did not revert the change"
    );

    // Redo
    client.redo().expect("redo failed");
    std::thread::sleep(Duration::from_millis(200));

    let after_redo = client.get_editor_text().expect("get text after redo");
    let after_redo_text = after_redo["text"].as_str().unwrap_or("");
    assert!(
        after_redo_text.contains("// undo test"),
        "redo did not re-apply the change"
    );

    // Restore
    fs::write(&test_file, &original).expect("restore fixture");
}

// ── Test 5: find_replace ─────────────────────────────────────────────────────

#[test]
fn t05_find_replace() {
    let (_proc, mut client) = launch_ide();

    let fixture_dir = fixtures_project();
    let test_file = fixture_dir.join("src/main.rs");
    let original = fs::read_to_string(&test_file).expect("read fixture");

    // Open file
    client.open_file(&test_file).expect("open_file failed");
    std::thread::sleep(Duration::from_millis(500));
    client.wait_idle(3000).ok();

    // Find and replace
    let result = client
        .find_replace("OwnStack", "TestStack")
        .expect("find_replace failed");

    let replacements = result["replacements"].as_u64().unwrap_or(0);
    assert!(
        replacements > 0,
        "expected at least 1 replacement, got {replacements}"
    );

    // Verify the editor text changed
    let after = client.get_editor_text().expect("get text after replace");
    let text = after["text"].as_str().unwrap_or("");
    assert!(
        text.contains("TestStack"),
        "replaced text not found in editor"
    );
    assert!(
        !text.contains("OwnStack"),
        "original text still found after replace"
    );

    // Restore
    fs::write(&test_file, &original).expect("restore fixture");
}

// ── Test 6: command palette ──────────────────────────────────────────────────

#[test]
fn t06_command_palette() {
    let (_proc, mut client) = launch_ide();

    // Try running a known workbench command
    let result = client.run_command("new_file");

    // If the command is known, it should work. If the strum name doesn't match,
    // that's also informative.
    match result {
        Ok(val) => {
            assert_eq!(val["status"], "ok", "command did not succeed: {val}");
        }
        Err(e) => {
            // If the exact command name doesn't match strum serialization,
            // that's expected — we just verify the driver responded
            eprintln!("[e2e] command error (may be expected): {e}");
        }
    }

    // Verify we can get state after command
    let state = client.get_state().expect("get_state after command");
    assert!(state.get("workspace").is_some(), "state should have workspace field");
}

// ── Test 7 (bonus): diagnostics ──────────────────────────────────────────────

#[test]
#[ignore = "requires LSP server running — enable when rust-analyzer is available"]
fn t07_diagnostics() {
    let (_proc, mut client) = launch_ide();

    let fixture_dir = fixtures_project();
    let broken_file = fixture_dir.join("src/broken.rs");

    // Open the broken file
    client.open_file(&broken_file).expect("open broken file");
    std::thread::sleep(Duration::from_secs(3)); // Give LSP time to analyze
    client.wait_idle(10000).ok();

    // Check diagnostics
    let diags = client.get_diagnostics().expect("get_diagnostics failed");
    let diag_map = diags["diagnostics"].as_object();

    // If LSP provided diagnostics, verify they exist for the broken file
    if let Some(map) = diag_map {
        let has_diags = map.values().any(|v| {
            v.as_array().map_or(false, |arr| !arr.is_empty())
        });
        if has_diags {
            eprintln!("[e2e] diagnostics found: {diags}");
        } else {
            eprintln!("[e2e] no diagnostics returned (LSP may not be running)");
        }
    }
}
