# OwnStack E2E testing

This folder and the `ownstack-e2e` crate provide end-to-end validation for the OwnStack runtime stack.

Two E2E layers are available:
1. Python script suite (`tests/e2e/*.py`) integrated by `scripts/healthcheck.py`
2. Rust harness crate (`e2e/`) with JSON-RPC golden-path tests

## 1. Python E2E scripts

Available scripts include:
- `verify_agent_spawn.py`
- `verify_sandbox_exec.py`
- `verify_escape_mitigation.py`
- `verify_llm_e2e.py`
- `test_policy_approval.py`
- `test_wasi_plugin.py`
- `test_agent_rpc.py`
- `test_mcp_handshake.py`
- `test_mini_project_mission.py`
- `test_scraper_bot_mission.py`
- `test_packaging_install_run.py`

Run from repository root:

```bash
python scripts/healthcheck.py
```

## 2. Rust E2E harness (`ownstack-e2e`)

The Rust harness launches the real IDE in E2E mode and drives it through JSON-RPC.

Build/compile tests:

```bash
cargo test -p ownstack-e2e --no-run
```

Run tests serially (recommended for deterministic runs):

```bash
cargo test -p ownstack-e2e -- --test-threads=1
```

Headless Linux:

```bash
xvfb-run -a cargo test -p ownstack-e2e -- --test-threads=1
```

Note:
- The test file contains a global mutex lock to serialize fixture-mutating tests.

## 3. E2E runtime flags

App E2E control server flags:
- `--e2e` / `OWNSTACK_E2E=1`
- `--e2e-port` / `OWNSTACK_E2E_PORT`
- `--window-size WIDTHxHEIGHT` / `OWNSTACK_WINDOW_SIZE`

The app prints `E2E_READY:<port>` when control server is available.

## 4. Packaging smoke env vars

Linux:
- `OWNSTACK_LINUX_TARBALL`
- `OWNSTACK_LINUX_APPIMAGE`
- `OWNSTACK_LINUX_FLATPAK`

Windows:
- `OWNSTACK_WINDOWS_PORTABLE_ZIP`
- `OWNSTACK_WINDOWS_MSI`
- `OWNSTACK_WINDOWS_PYTHON_BUNDLE` (optional)

macOS:
- `OWNSTACK_MACOS_DMG`
- `OWNSTACK_MACOS_APP_BIN`

## 5. Complex mission toggle

To include heavy mission scripts in healthcheck:

```bash
export OWNSTACK_RUN_COMPLEX_MISSIONS=1
python scripts/healthcheck.py
```
