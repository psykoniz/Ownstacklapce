# OwnStack Native IDE - Architecture

Version: 1.2  
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

Current implemented guards include:

- `max_consecutive_failures` default: `10`
- Tool args size hard-limit: `16 KB` (`TOOL_ARGS_MAX_BYTES`)
- Repeated parse error guard: abort after `3` repeated signatures (`REPEATED_PARSE_ERROR_LIMIT`)

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
- `PolicyPrompt` / `PolicyResponse` (with `correlation_id`)
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
- Onboarding polish: `lapce-app/src/ownstack_onboarding.rs`
- Empty states: `lapce-app/src/ownstack_empty_state.rs`
- MCP panel UX/config integration: `lapce-app/src/ownstack_mcp.rs`
- Empty editor placeholder overlay: `lapce-app/src/editor/view.rs`

## 8. MCP and plugin surfaces

### 8.1 MCP servers

Runtime config path:

- `.ownstack/mcp_servers.json`

Load path:

- `ownstack-agent/src/main.rs` (`load_mcp_server_configs`)

### 8.2 WASI/plugins

Plugin host and toolkit integration live under:

- `ownstack-agent/src/plugins/mod.rs`

## 9. Testing and validation baseline

Primary validation commands used for this architecture:

- `cargo check --workspace --all-targets`
- `cargo test -p ownstack-agent`
- `cargo test -p ownstack-engine`
- `cargo test -p lapce-app ownstack_tests`
- `python scripts/healthcheck.py`

Healthcheck scripts include agent spawn, sandbox, policy approval, MCP handshake,
packaging smoke, WASI plugin, and optional complex mission E2E tests.

## 10. Current constraints and explicit non-goals

- No rewrite of Lapce editor core (`lapce-core/src/buffer.rs`, `lapce-core/src/syntax.rs` remain protected)
- Security flow cannot be bypassed
- Rust native stack only (no Electron/Tauri runtime path)

## 11. Next architecture focus

With Phase 12 marked complete, the next architecture work is stabilization:

1. Keep agent-first runtime semantics consistent across all UI surfaces
2. Expand E2E coverage (complex missions and release/install checks)
3. Reduce warning noise in key crates and keep CI gates strict
4. Harden release pipeline validation on real signed/notarized artifacts

