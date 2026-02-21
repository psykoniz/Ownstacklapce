# OwnStackLapce Roadmap (Executable)

Date: 2026-02-14

This roadmap is written to match `GEMINI.md` security requirements and the
current phase in `.ownstack/current_phase.json`.

## Current State

- Phase file: `.ownstack/current_phase.json` reports `current_phase = 4`.
- Core wiring gaps were present (chat/palette prompts, OwnStack message routing,
  Ask-mode approvals, budgets, Windows sandbox parsing). These are being closed
  incrementally with tests.

## Milestone 0 (P0): End-to-End Prompt Works

Goal: user prompt -> proxy -> agent -> streaming response visible in IDE.

- [x] Send `OwnStackRpc::AiPrompt` from chat panel via `proxy.ownstack(...)`
  - file: `lapce-app/src/ownstack_chat.rs`
- [x] Send `OwnStackRpc::AiPrompt` from palette via `proxy.ownstack(...)`
  - file: `lapce-app/src/ownstack_palette.rs`
- [x] Route OwnStack notifications in the UI (stream chunk + mission + tool/audit/policy)
  - file: `lapce-app/src/window_tab.rs`

Acceptance:
- Chat prompt triggers agent spawn and streams tokens back into the chat panel.

## Milestone 1 (P0 Security): Ask-Mode + Audit

Goal: `PolicyDecision::Ask` never executes silently; everything is auditable.

- [x] Implement a Policy approval handshake agent <-> IDE UI
  - new: `ownstack-agent/src/policy_approval.rs`
  - agent: `ownstack-agent/src/main.rs` (stdin reader + work queue; avoids deadlock)
  - UI: `lapce-app/src/window_tab.rs` (Approve/Deny modal -> `PolicyResponse`)
- [x] Enforce Ask-mode in toolkits:
  - `ownstack-agent/src/toolkits/core.rs`
  - `ownstack-agent/src/toolkits/git.rs`
- [x] Write JSONL audit entries for core + git tool execution
  - files: `ownstack-agent/src/toolkits/core.rs`, `ownstack-agent/src/toolkits/git.rs`

Acceptance:
- A command classified as Ask triggers an approval modal.
- Deny => tool fails, and an audit entry is written.
- Approve => tool runs, and an audit entry is written.

## Milestone 2 (P0 Anti-Runaway): Budgets + Duration

Goal: load budgets and stop runaway agent loops.

- [x] Load budgets from `.ownstack/budgets.json` at agent startup (subset mapped)
  - file: `ownstack-agent/src/main.rs`
- [x] Enforce `max_duration_minutes` during agent loop
  - file: `ownstack-agent/src/orchestrator.rs`

Acceptance:
- If runtime exceeds `max_duration_minutes`, the agent stops with a budget error.

## Milestone 2b (P0 Safety): Kill-Switch

Goal: user can stop the agent immediately; proxy kills agent + bridge processes.

- [x] Add `OwnStackRpc::KillSwitch`
  - file: `lapce-rpc/src/ownstack.rs`
- [x] Chat "Stop" button triggers `KillSwitch`
  - file: `lapce-app/src/ownstack_chat.rs`
- [x] Proxy handles `KillSwitch` by killing child processes and not auto-restarting
  - file: `lapce-proxy/src/dispatch.rs`

Acceptance:
- While the agent is generating or running a tool, press Stop => agent is killed and
  the IDE returns to an idle state.

## Milestone 3 (P0 Windows Reliability): Sandbox Parsing

Goal: Windows sandbox tests pass; quoting works.

- [x] Replace whitespace splitting with a quote-aware command splitter
  - file: `ownstack-engine/src/sandbox/process.rs`
- [x] Update stress tests to be deterministic on Windows environments
  - file: `ownstack-engine/tests/sandbox_stress.rs`

Acceptance:
- `cargo test -p ownstack-engine` passes on Windows.

## Next (Planned)

- [x] Kill-Switch (stop agent + kill subprocesses) wired from UI -> proxy -> agent.
- [x] Replace `grep | head` search tool with a Rust-native search (cross-platform).
- [ ] OwnStack status bar integration (mode + running state) + better tool/audit panels.
- [ ] Phase 4 distribution:
  - bundle `ownstack-agent.exe` and optional Python backend
  - MSI pipeline validation
  - first-launch onboarding polish + provider key storage (no secrets in logs)
