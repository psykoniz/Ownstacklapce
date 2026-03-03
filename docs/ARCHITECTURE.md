# OwnStack Native IDE - Architecture

Version: 1.3  
Last updated: 2026-03-02  
Runtime status: Phase 12 complete (`.ownstack/current_phase.json`)

## 1. Scope and source of truth

This document describes the current implemented architecture in this repository.

Authoritative references:

- `GEMINI.md`
- `AGENTS.md`
- `.ownstack/current_phase.json`
- Workspace `Cargo.toml`

## 2. Monorepo layout

Rust workspace members (current):

- `lapce-app`
- `lapce-core`
- `lapce-proxy`
- `lapce-rpc`
- `ownstack-engine`
- `ownstack-bridge`
- `ownstack-agent`

Non-workspace runtime component:

- `ownstack-python` (sidecar integration path used through `ownstack-bridge`)

## 3. Runtime topology

The product runs as a multi-process native stack:

1. `lapce-app` (Floem UI)
2. `lapce-proxy` (dispatch/process orchestration)
3. `ownstack-agent` (native agent runtime, tool orchestration, provider calls)
4. Optional `ownstack-bridge` -> `ownstack-python` sidecar path for bridge-backed RPC

Key wiring:

- UI sends/receives `OwnStackRpc` via proxy
- Proxy manages native agent lifecycle and kill-switch semantics
- Agent owns runtime mode/state and emits UI deltas

## 4. Security execution chain (mandatory)

Every command/tool execution must follow:

`Command -> PolicyEngine -> PathValidator -> Sandbox -> ToolResult -> AuditLog`

Implemented core locations:

- Policy: `ownstack-engine/src/policy.rs`
- Path safety: `ownstack-engine/src/path_safety.rs`
- Sandbox: `ownstack-engine/src/sandbox/process.rs`
- Audit: `ownstack-engine/src/audit.rs`
- Security context composition: `ownstack-engine/src/security.rs`

## 5. Agent runtime model

Agent runtime is centered in `ownstack-agent/src/main.rs` and `ownstack-agent/src/orchestrator.rs`.

### 5.1 Runtime mode and run state

Runtime mode (agent-owned):

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

UI reflects mode/state from runtime RPC, not local-only toggles.

### 5.2 Budgets and anti-runaway guards

Current implemented guards:

- `max_consecutive_failures` default: `10`
- Tool args size hard-limit: `16 KB` (`TOOL_ARGS_MAX_BYTES`)
- Repeated parse error guard: abort after `3` repeated signatures (`REPEATED_PARSE_ERROR_LIMIT`)
- JSON parse errors are **never silently dropped**: `validate_and_parse_tool_args` (`orchestrator.rs:120`) returns structured `OrchestratorError` with `error_kind`, `args_prefix`, and `raw_arguments` metadata injected into the LLM tool response for self-correction.

These controls are enforced in the orchestrator loop and tested in unit tests.

### 5.3 Policy approval flow

Policy approval manager:

- `ownstack-agent/src/policy_approval.rs`

Behavior:

- Correlated request/response via `correlation_id`
- Default timeout: `15s` (`POLICY_PROMPT_TIMEOUT_SECS`)
- Mismatched IDs are rejected safely
- No pending approval fallback resolves to deny

## 6. RPC contract (current)

`lapce-rpc/src/ownstack.rs` is the RPC contract for OwnStack.

Important variants in active use:

- `AiPrompt`
- `SetAgentMode`
- `KillSwitch`
- `AiStreamChunk`
- `PolicyPrompt` / `PolicyResponse` (with `correlation_id`, `timeout_secs`, `cwd`)
- `MissionUpdate`
- `BudgetUpdate`
- `ContextUpdate`
- `UiStateDelta` (semantic runtime delta)
- `ToolResultMsg`
- `AuditEvent`
- `UiSnapshot`, `UiSnapshotRequest`, `CaptureScreenshot`

`UiStateDelta` fields currently shipped:

- `mode`
- `run_state`
- `budget`
- `context`
- `mission`
- `pending_approval`
- `tool_event`
- `alert`

## 7. UI architecture (agent-first thin-client direction)

Core reducer point:

- `lapce-app/src/window_tab.rs` (`apply_ownstack_ui_delta`)

UI responsibilities:

- Render runtime state (chat/status/panels)
- Trigger user actions (prompt, mode change, policy approve/deny, kill-switch)
- Avoid owning agent business logic

Recent UI layers:

- Status bar improvements: `lapce-app/src/status.rs`
- Onboarding wizard (5 steps, mounted at startup): `lapce-app/src/ownstack_onboarding.rs`, wired in `lapce-app/src/app.rs`
- Empty states: `lapce-app/src/ownstack_empty_state.rs`
- MCP panel UX/config integration: `lapce-app/src/ownstack_mcp.rs`
- Empty editor placeholder overlay: `lapce-app/src/editor/view.rs`
- Policy Engine approval modal (with `correlation_id` + timer): `lapce-app/src/window_tab.rs`

## 8. API key management

### 8.1 Onboarding wizard

Implemented in `lapce-app/src/ownstack_onboarding.rs`. Mounted at startup in `lapce-app/src/app.rs`.

Steps: Welcome → Provider Setup → Mode Selection → Workspace Config → Finish.

Supported providers: OpenRouter, Anthropic, Local (Ollama).

### 8.2 Keyring storage

Keys are stored in the OS-native keyring (not in plaintext files or env vars):

| Platform | Backend |
|----------|---------|
| Windows | Windows Credential Manager |
| macOS | macOS Keychain |
| Linux | Secret Service (libsecret) |

Service name: `"OwnStack IDE"` (constant in `ownstack_onboarding.rs` and `secret_store.rs`).

### 8.3 Key synchronization (agent side)

`ownstack-agent/src/secret_store.rs` implements `sync_env_and_keyring()`:

- If env var present but keyring missing/different → writes to keyring.
- If env var absent but keyring has key → hydrates env var from keyring.
- Bidirectional sync at agent startup; no manual env var configuration required.

## 9. MCP and plugin surfaces

### 9.1 MCP servers

Runtime config path:

- `.ownstack/mcp_servers.json`

Additional config source (auto-detected if present):

- Claude Desktop `claude_desktop_config.json` (platform path resolved at runtime)

Load path:

- `ownstack-agent/src/main.rs` (`load_mcp_server_configs`)
- UI panel reads config via `ownstack-app/src/ownstack_mcp.rs` (`load_mcp_servers`)

### 9.2 WASI/plugins

Plugin host and toolkit integration live under:

- `ownstack-agent/src/plugins/mod.rs`

## 10. Testing and validation baseline

### 10.1 E2E test scripts (12 total)

Located in `tests/e2e/`:

| Script | Coverage |
|--------|----------|
| `test_agent_rpc.py` | Agent spawn, basic RPC |
| `test_mcp_handshake.py` | MCP server connection |
| `test_mini_project_mission.py` | Simple mission |
| `test_scraper_bot_mission.py` | Complex multi-file mission |
| `test_policy_approval.py` | Policy Ask/Allow/Deny E2E |
| `test_packaging_install_run.py` | Installer smoke test |
| `test_python_debt_report.py` | Python sidecar |
| `test_wasi_plugin.py` | WASI plugin host |
| `verify_agent_spawn.py` | Agent process spawn |
| `verify_escape_mitigation.py` | Path safety/sandbox escape |
| `verify_llm_e2e.py` | LLM provider round-trip |
| `verify_sandbox_exec.py` | Sandbox exec isolation |

### 10.2 Healthcheck

`scripts/healthcheck.py` executes a subset of the above. Tests requiring LLM API keys are **auto-skipped** when no key is present — no manual configuration needed.

### 10.3 Primary validation commands

- `cargo check --workspace --all-targets`
- `cargo test -p ownstack-agent`
- `cargo test -p ownstack-engine`
- `cargo test -p lapce-app ownstack_tests`
- `python scripts/healthcheck.py`

## 11. Packaging

| Platform | Status | Script |
|----------|--------|--------|
| Windows | Functional | `scripts/build_windows_installer.ps1` + `scripts/bundle_python.py` |
| Linux AppImage | Scripted | `scripts/build_appimage.sh` |
| Linux Flatpak | Scripted | `scripts/build_flatpak_bundle.sh` |
| macOS | Not yet automated in release pipeline | — |

Python sidecar bundling is part of the Windows installer flow. Cross-platform release pipeline is not yet fully industrialized.

## 12. Current constraints and explicit non-goals

- No rewrite of Lapce editor core (`lapce-core/src/buffer.rs`, `lapce-core/src/syntax.rs` remain protected)
- Security flow cannot be bypassed
- Rust native stack only (no Electron/Tauri runtime path)

## 13. Next architecture focus

With Phase 12 marked complete, the next architecture work is stabilization:

1. Validate E2E scripts in CI with a real LLM API key
2. Finalize cross-platform release pipeline (macOS packaging, Linux notarization)
3. Reduce warning noise in key crates and keep CI gates strict
4. Harden release pipeline validation on real signed/notarized artifacts
