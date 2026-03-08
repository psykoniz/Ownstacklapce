# OwnStack Operations Runbook

Last updated: 2026-03-08

## 1. Scope

This runbook covers runtime behavior for:
- Agent execution and UI interaction
- Security chain enforcement
- Secret handling
- E2E and CI validation
- Release checks

## 2. Mandatory security execution flow

Every tool action must follow:

`Command -> PolicyEngine -> PathValidator -> Sandbox -> ToolResult -> AuditLog`

Hard requirements:
- No direct tool execution from UI bypassing proxy/agent flow
- `ask` decisions require explicit approve/deny path
- Blocked command classes remain blocked by policy defaults
- Audit entries are required for execution outcomes

## 3. Runtime process wiring

Primary process path:
1. `lapce-app` sends `OwnStackRpc`
2. `lapce-proxy` dispatches and manages lifecycle
3. `ownstack-agent` executes orchestration/tool calls

Optional bridge path:
- `ownstack-bridge` to `ownstack-python`

Kill-switch behavior:
- `OwnStackRpc::KillSwitch` is handled in proxy dispatch
- agent and bridge subprocesses are stopped immediately

## 4. Secret and onboarding handling

Secrets use OS keyring:
- Service: `OwnStack IDE`
- Keys: `openrouter_api_key`, `anthropic_api_key`

Relevant paths:
- Onboarding state file: `ownstack-onboarding.json`
- Onboarding UI: `lapce-app/src/ownstack_onboarding.rs`
- Secret read/env hydration before process spawn: `lapce-proxy/src/dispatch.rs`

Rules:
- Never log secret values
- Never store API keys in plaintext project config
- Keep onboarding state non-secret

## 5. Agent mode and runtime state

Mode:
- `ask`
- `auto`
- `plan`

Run state:
- `disconnected`
- `idle`
- `running`
- `awaiting_approval`
- `stopped`
- `error`

UI must reflect runtime state from RPC, not local-only toggles.

## 6. Policy approval operations

Policy approval manager behavior:
- Correlation ID required
- Timeout default: 15s
- Mismatched responses ignored safely
- Timeout treated as deny

RPC variants:
- `PolicyPrompt { command, reason, cwd, correlation_id, timeout_secs }`
- `PolicyResponse { approved, correlation_id }`

## 7. Runtime delta and reducer model

Agent emits semantic deltas through `UiStateDelta`:
- `mode`, `run_state`
- `budget`, `context`
- `mission`
- `pending_approval`
- `tool_event`
- `alert`

Reducer path:
- `lapce-app/src/window_tab.rs` (`apply_ownstack_ui_delta`)

Compatibility updates still accepted:
- `BudgetUpdate`
- `ContextUpdate`

## 8. LSP discovery operations

Proxy module:
- `lapce-proxy/src/lsp_discovery.rs`

Behavior:
- On buffer open, proxy checks known server availability for detected language
- If missing, emits `OwnStackRpc::LspNotInstalled { language_id, install_hint }`
- UI displays informational install guidance

## 9. E2E control server operations

App-side E2E driver:
- `lapce-app/src/e2e_driver.rs`

Startup knobs:
- `--e2e` / `OWNSTACK_E2E`
- `--e2e-port` / `OWNSTACK_E2E_PORT`
- `--window-size` / `OWNSTACK_WINDOW_SIZE`

Security note:
- Screenshot capture no longer uses interpolated `sh -c` command strings

Rust E2E harness:
- `e2e/src/lib.rs`
- `e2e/tests/e2e_golden_path.rs`

## 10. MCP operations

MCP runtime config:
- `.ownstack/mcp_servers.json`

Load path:
- `ownstack-agent/src/main.rs` (`load_mcp_server_configs`)

UI panel:
- `lapce-app/src/ownstack_mcp.rs`

## 11. Validation commands

Core local checks:

```bash
cargo check --workspace --all-targets
cargo test -p ownstack-agent
cargo test -p ownstack-engine
cargo test -p lapce-app ownstack_tests
cargo test -p lapce-proxy lsp_discovery
cargo test -p ownstack-e2e --no-run
python scripts/healthcheck.py
```

Optional heavy missions (enable with `OWNSTACK_RUN_COMPLEX_MISSIONS=1`):
- `test_mini_project_mission.py`
- `test_scraper_bot_mission.py`

## 12. Release operations

Release workflow:
- `.github/workflows/release.yml`

Expected artifact families:
- Windows: MSI + portable package
- macOS: DMG (sign/notarization path)
- Linux: tar/deb/rpm/AppImage/Flatpak variants

Recommended gate before publish:
- run workspace checks and healthcheck
- validate release workflow artifact steps

## 13. Common incident notes

- Windows `os error 5` usually means stale running binaries during rebuild
- Cargo lock waits usually mean concurrent cargo processes
- E2E startup issues: verify fresh binaries and `E2E_READY` line visibility
