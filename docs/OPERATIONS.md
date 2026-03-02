# OwnStack Operations Runbook

Last updated: 2026-03-02

## 1. Scope

This runbook covers runtime behavior for:

- Agent execution and UI interaction
- Security chain enforcement
- Secret handling
- E2E/CI validation and release checks

## 2. Mandatory security execution flow

Every tool action must follow:

`Command -> PolicyEngine -> PathValidator -> Sandbox -> ToolResult -> AuditLog`

Hard requirements:

- No direct tool execution from UI bypassing proxy/agent flow
- `Ask` decisions must require explicit approve/deny handling
- Blocked command classes remain blocked by policy defaults
- Audit entries must be produced for execution outcomes

## 3. Runtime process wiring

Primary process path:

1. `lapce-app` sends `OwnStackRpc`
2. `lapce-proxy` dispatches and manages process lifecycle
3. `ownstack-agent` executes orchestration/tool calls

Bridge path is available via `ownstack-bridge` and `ownstack-python` when enabled.

Kill-switch behavior:

- `OwnStackRpc::KillSwitch` is handled in proxy dispatch
- Agent and bridge subprocesses are stopped immediately

## 4. Secret and onboarding handling

Secrets use OS keyring:

- Service: `OwnStack IDE`
- Keys: `openrouter_api_key`, `anthropic_api_key`

Relevant paths:

- Onboarding state file: `ownstack-onboarding.json`
- Onboarding UI and save: `lapce-app/src/ownstack_onboarding.rs`
- Secret read and env hydration into agent process: `lapce-proxy/src/dispatch.rs`

Operational rules:

- Never log secret values
- Never persist API keys in plain JSON project config
- Keep onboarding state non-secret

## 5. Agent mode and runtime state

Agent mode is runtime-owned:

- `ask`
- `auto`
- `plan`

Run states:

- `disconnected`
- `idle`
- `running`
- `awaiting_approval`
- `stopped`
- `error`

UI must reflect runtime state from RPC, not local-only toggles.

## 6. Policy approval operations

Policy approval is handled by `PolicyApprovalManager`:

- Correlation ID enforced on prompt/response matching
- Timeout constant: `15s`
- Mismatched responses are ignored safely
- Timeout resolves as deny

RPC variants used:

- `PolicyPrompt { command, reason, cwd, correlation_id, timeout_secs }`
- `PolicyResponse { approved, correlation_id }`

## 7. Runtime deltas and UI reducer

Agent emits semantic UI deltas through `UiStateDelta`:

- `mode`, `run_state`
- `budget`, `context`
- `mission`
- `pending_approval`
- `tool_event`
- `alert`

Reducer path:

- `lapce-app/src/window_tab.rs` (`apply_ownstack_ui_delta`)

Legacy budget/context update variants are still consumed for compatibility:

- `BudgetUpdate`
- `ContextUpdate`

## 8. MCP operations

MCP config file:

- `.ownstack/mcp_servers.json`

Load path:

- `ownstack-agent/src/main.rs` (`load_mcp_server_configs`)

UI panel path:

- `lapce-app/src/ownstack_mcp.rs`

## 9. Vision/snapshot operations

UI snapshot and capture messages:

- `UiSnapshotRequest`
- `UiSnapshot`
- `CaptureScreenshot`

Toolkit and engine paths:

- `ownstack-agent/src/toolkits/vision.rs`
- `ownstack-engine/src/vision.rs`

## 10. Validation commands

Core local checks:

```bash
cargo check --workspace --all-targets
cargo test -p ownstack-agent
cargo test -p ownstack-engine
cargo test -p lapce-app ownstack_tests
python scripts/healthcheck.py
```

Healthcheck script set currently includes:

- `verify_agent_spawn.py`
- `verify_sandbox_exec.py`
- `verify_escape_mitigation.py`
- `test_wasi_plugin.py`
- `test_policy_approval.py`
- `test_agent_rpc.py`
- `test_mcp_handshake.py`
- `test_packaging_install_run.py`
- `verify_llm_e2e.py`

Optional heavy missions (enabled by `OWNSTACK_RUN_COMPLEX_MISSIONS=1`):

- `test_mini_project_mission.py`
- `test_scraper_bot_mission.py`

## 11. Release operations

Release workflow:

- `.github/workflows/release.yml`

Expected artifact families:

- Windows: MSI + portable zip
- macOS: DMG (signed/notarized path)
- Linux: tar/deb/rpm/AppImage/Flatpak variants

Recommended gate before publish:

- run CI checks and healthcheck locally
- confirm artifact validation job passes

## 12. Incident notes (Windows-heavy)

- Build can fail with `os error 5` when `ownstack-agent.exe` remains running
- Cargo file-lock waits usually indicate concurrent or stale cargo processes
- If E2E appears silent, verify script points to the correct fresh agent binary

