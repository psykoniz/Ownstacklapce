# OwnStack E2E Scripts

This directory contains interactive/regression scripts used to validate the full OwnStack stack during development.

## Scripts

- `verify_agent_spawn.py`: verifies proxy -> native agent startup and basic RPC path.
- `verify_sandbox_exec.py`: validates sandbox command execution path.
- `verify_escape_mitigation.py`: checks policy/path escape protections.
- `verify_llm_e2e.py`: validates LLM streaming path end-to-end.
- `test_policy_approval.py`: validates approval prompt and response routing.
- `test_wasi_plugin.py`: validates WASI plugin execution and JSON output.
- `test_agent_rpc.py`: validates raw agent RPC contract behavior.
- `test_mcp_handshake.py`: validates MCP initialize/tools handshake with a mock server.
- `test_mini_project_mission.py`: validates a complex mission that generates a Rust mini project and runs tests.
- `test_scraper_bot_mission.py`: validates a complex mission that generates an offline scraper-style Rust project and runs tests.
- `test_packaging_install_run.py`: validates packaging artifacts install/run smoke checks per OS.

## Packaging Smoke Env Vars

- Linux:
  - `OWNSTACK_LINUX_TARBALL`
  - `OWNSTACK_LINUX_APPIMAGE`
  - `OWNSTACK_LINUX_FLATPAK`
- Windows:
  - `OWNSTACK_WINDOWS_PORTABLE_ZIP`
  - `OWNSTACK_WINDOWS_MSI`
  - `OWNSTACK_WINDOWS_PYTHON_BUNDLE` (optional, `python_bundle.zip` or extracted dir)
- macOS:
  - `OWNSTACK_MACOS_DMG`
  - `OWNSTACK_MACOS_APP_BIN`

## Run

From repository root:

```bash
python scripts/healthcheck.py
```

Run the scraper mission E2E directly:

```bash
python tests/e2e/test_scraper_bot_mission.py --workspace .
```
