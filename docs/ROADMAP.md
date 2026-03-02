# OwnStackLapce Roadmap

Date: 2026-03-02

This roadmap is aligned with:

- `GEMINI.md`
- `AGENTS.md`
- `.ownstack/current_phase.json`

## 1. Current baseline

From `.ownstack/current_phase.json`:

- `current_phase = 12`
- `phase_0_complete` through `phase_12_complete` are `true`

The project is in post-phase stabilization, not in feature bootstrap.

## 2. Completed milestones (high level)

Delivered in the current baseline:

- Agent-first runtime mode/state loop (`Ask/Auto/Plan`, run-state deltas)
- Security path enforced (`Policy -> Path -> Sandbox -> ToolResult -> Audit`)
- Policy E2E with correlation IDs and timeout behavior
- Anti-loop guard and tool-args size hard-limit in orchestrator
- MCP runtime config + integration tests
- Workspace sandbox path-safety hardening
- UI state surfaces updated (status/chat/onboarding/empty states)

## 3. Active priorities (next execution window)

### P0 - Stabilize and release hardening

1. Keep CI gates strict and reproducible:
   - `cargo check --workspace --all-targets`
   - key crate tests
   - `scripts/healthcheck.py`
2. Validate release pipeline with real signing/notarization secrets.
3. Keep cross-platform executable path handling stable (`EXE_SUFFIX` path logic).

### P1 - E2E depth and mission reliability

1. Run complex mission E2E in controlled environments:
   - `tests/e2e/test_mini_project_mission.py`
   - `tests/e2e/test_scraper_bot_mission.py`
2. Track failures by model/provider class and keep parser feedback actionable.
3. Preserve no-regression behavior for policy, MCP, and sandbox scripts.

### P1 - Agent-first UI consistency

1. Ensure all UI mode/status surfaces derive from runtime deltas.
2. Keep chat/status/panel rendering thin (no duplicated business logic in UI).
3. Continue UX polish only when it does not bypass runtime/security invariants.

## 4. Deferred items (not blocking baseline)

- Further specialist toolkit depth (v2+ logic quality)
- Additional packaging polish for all distro channels
- Extended docs and go-live communication material

## 5. Definition of done for this cycle

1. Branch builds with no compile errors on workspace.
2. No regression in core test suites and healthcheck scripts.
3. Updated docs reflect current runtime contract and phase status.
4. Release branch is push-ready with reproducible validation commands.

