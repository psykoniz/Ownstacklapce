# OwnStack Native IDE - Architecture

Version: 1.4
Last updated: 2026-03-08
Runtime status: Phase 12 complete (`.ownstack/current_phase.json`)

## 1. Scope and source of truth

This document describes the currently implemented architecture in this repository.

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
- `e2e` (`ownstack-e2e` Rust E2E client/tests)

Non-workspace runtime component:
- `ownstack-python` (sidecar integration path used through `ownstack-bridge`)

## 3. Runtime topology

Primary runtime stack:
1. `lapce-app` (Floem UI)
2. `lapce-proxy` (dispatch/process orchestration)
3. `ownstack-agent` (native agent runtime, tool orchestration, provider calls)
4. Optional `ownstack-bridge -> ownstack-python` path

Key wiring:
- UI sends/receives `OwnStackRpc` through proxy
- Proxy owns lifecycle (agent/bridge start, stop, kill-switch)
- Agent emits semantic UI deltas consumed by UI reducer

## 4. Mandatory security execution chain

Every command/tool execution must follow:

`Command -> PolicyEngine -> PathValidator -> Sandbox -> ToolResult -> AuditLog`

Core implementations:
- Policy: `ownstack-engine/src/policy.rs`
- Path safety: `ownstack-engine/src/path_safety.rs`
- Sandbox: `ownstack-engine/src/sandbox/process.rs`
- Audit: `ownstack-engine/src/audit.rs`
- Security composition: `ownstack-engine/src/security.rs`

## 5. Agent runtime model

Entry points:
- `ownstack-agent/src/main.rs`
- `ownstack-agent/src/orchestrator.rs`

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

UI reflects runtime mode/state from RPC (not local-only toggles).

### 5.2 Anti-runaway guards

Implemented guards include:
- `max_consecutive_failures` default: `10`
- Tool args max size: `16 KB` (`TOOL_ARGS_MAX_BYTES`)
- Repeated parse error abort: `3` (`REPEATED_PARSE_ERROR_LIMIT`)
- Structured parse-error feedback via `validate_and_parse_tool_args`

## 6. RPC contract highlights

Contract file: `lapce-rpc/src/ownstack.rs`

Important variants in active use:
- `AiPrompt`
- `SetAgentMode`
- `KillSwitch`
- `AiStreamChunk`
- `PolicyPrompt` / `PolicyResponse`
- `MissionUpdate`
- `BudgetUpdate`
- `ContextUpdate`
- `UiStateDelta`
- `ToolResultMsg`
- `AuditEvent`
- `UiSnapshot`, `UiSnapshotRequest`, `CaptureScreenshot`
- `LspNotInstalled`

`UiStateDelta` fields currently shipped:
- `mode`
- `run_state`
- `budget`
- `context`
- `mission`
- `pending_approval`
- `tool_event`
- `alert`

## 7. UI architecture

Reducer entry:
- `lapce-app/src/window_tab.rs` (`apply_ownstack_ui_delta`)

UI responsibilities:
- Render runtime state (chat/status/panels)
- Trigger user actions (prompt, mode change, approval, kill-switch)
- Stay thin: avoid duplicating agent business logic in UI layer

Current OwnStack UI modules include:
- Chat/status/audit/palette/onboarding/MCP panels under `lapce-app/src/ownstack_*`
- Empty-state UI: `lapce-app/src/ownstack_empty_state.rs`
- E2E control driver: `lapce-app/src/e2e_driver.rs`

## 8. API key management

Onboarding path:
- `lapce-app/src/ownstack_onboarding.rs`

Key storage:
- OS-native keyring (`OwnStack IDE` service)
- Keys: OpenRouter and Anthropic entries

Proxy-side secret hydration:
- `lapce-proxy/src/dispatch.rs`

## 9. MCP and plugin surfaces

MCP config:
- `.ownstack/mcp_servers.json`

Runtime load path:
- `ownstack-agent/src/main.rs` (`load_mcp_server_configs`)

UI panel path:
- `lapce-app/src/ownstack_mcp.rs`

WASI/plugin host:
- `ownstack-agent/src/plugins/mod.rs`

## 10. Testing architecture

### 10.1 Python E2E scripts

Located in `tests/e2e/` (healthcheck-compatible), including:
- policy approval
- MCP handshake
- WASI plugin check
- packaging smoke checks
- optional complex mission runs

### 10.2 Rust E2E crate

Workspace crate:
- `e2e` (`ownstack-e2e`)

Contains:
- IDE launcher client (`e2e/src/lib.rs`)
- JSON-RPC golden path tests (`e2e/tests/e2e_golden_path.rs`)

### 10.3 Baseline validation commands

- `cargo check --workspace --all-targets`
- `cargo test -p ownstack-agent`
- `cargo test -p ownstack-engine`
- `cargo test -p lapce-app ownstack_tests`
- `cargo test -p lapce-proxy lsp_discovery`
- `cargo test -p ownstack-e2e --no-run`
- `python scripts/healthcheck.py`

## 11. Packaging architecture

Release pipeline:
- `.github/workflows/release.yml`

Target artifact families:
- Windows MSI + portable package
- macOS DMG/sign/notarization path
- Linux tar/deb/rpm/AppImage/Flatpak variants

## 12. Constraints and non-goals

- No rewrite of Lapce editor core (`lapce-core/src/buffer.rs`, `lapce-core/src/syntax.rs` protected)
- Security execution chain cannot be bypassed
- Rust-native runtime only (no Electron/Tauri runtime path)

## 13. Current stabilization focus

1. Keep E2E paths deterministic and secure (driver + harness)
2. Keep CI/release automation reproducible across platforms
3. Reduce warning noise in core crates and hold strict gates
4. Continue phase-appropriate hardening without architectural bypasses
