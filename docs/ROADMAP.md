# OwnStack Native IDE — Roadmap

Date: 2026-03-02

This roadmap is aligned with:

- `GEMINI.md`
- `AGENTS.md`
- `.ownstack/current_phase.json`

## 1. Current baseline

From `.ownstack/current_phase.json`:

- `current_phase = 12`
- `phase_0_complete` through `phase_12_complete` are `true`

The project is in **post-phase stabilization**, not in feature bootstrap.

## 2. Completed milestones

- Agent-first runtime mode/state loop (`Ask/Auto/Plan`, run-state deltas via `UiStateDelta` RPC)
- Security path enforced: `Policy → Path → Sandbox → ToolResult → Audit`
- Policy E2E with `correlation_id`, `timeout_secs`, `cwd` and modal UI (`window_tab.rs`)
- JSON parse error hardening: `validate_and_parse_tool_args` (`orchestrator.rs:120`) returns structured errors with `raw_arguments` metadata — no silent null fallback
- Anti-loop guard (`max_consecutive_failures = 10`) and tool-args size hard-limit (`16 KB`)
- MCP runtime config + integration tests (reads `.ownstack/mcp_servers.json` and Claude Desktop config)
- Workspace sandbox path-safety hardening
- Onboarding wizard (5 steps, mounted at startup): provider selection, keyring key storage, mode selection, workspace config
- OS-native keyring integration (Windows Credential Manager / macOS Keychain / Linux Secret Service) via `secret_store.rs`
- 12 E2E test scripts in `tests/e2e/` + `scripts/healthcheck.py` (auto-skip without keys)
- UI surfaces: status bar budgets, chat streaming/diffs/tool results, kill-switch, MCP panel (dynamic config), empty states
- Windows installer with Python sidecar bundling (`scripts/build_windows_installer.ps1` + `scripts/bundle_python.py`)

## 3. Active priorities (next execution window)

### P0 — CI and release hardening

1. Run `cargo check --workspace --all-targets` as CI gate (no compile errors).
2. Run `scripts/healthcheck.py` as baseline smoke gate.
3. Validate E2E scripts with a real LLM API key in a controlled environment (`test_scraper_bot_mission.py`, `test_mini_project_mission.py`).
4. Cross-platform packaging: automate macOS DMG and Linux package signing in the release pipeline.

### P1 — E2E mission reliability

1. Track per-model/provider JSON parse failure rates using the structured metadata already in place (`raw_arguments`, `error_kind`).
2. Maintain no-regression behavior for policy, MCP, sandbox, and keyring scripts.

### P1 — Agent-first UI consistency

1. Ensure all UI mode/status surfaces derive exclusively from runtime `UiStateDelta`.
2. Keep chat/status/panel rendering thin (no duplicated business logic in UI).
3. Continue UX polish only when it does not bypass runtime/security invariants.

## 4. Deferred items (not blocking baseline)

- Settings Modal UI for key management (keyring works end-to-end; UI management panel is deferred)
- AI model dropdown selector in chat UI
- Per-message feedback buttons (thumbs up/down)
- Further specialist toolkit depth (v2+ logic quality)
- Extended docs and go-live communication material

## 5. Definition of done for this cycle

1. Branch builds with no compile errors on workspace.
2. No regression in core test suites and healthcheck scripts.
3. At least one complex E2E mission validated successfully with a real API key.
4. Updated docs reflect current runtime contract and phase status.
5. Release branch is push-ready with reproducible validation commands.
