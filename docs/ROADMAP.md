# OwnStack Native IDE - Roadmap

Date: 2026-03-08

This roadmap is aligned with:
- `GEMINI.md`
- `AGENTS.md`
- `.ownstack/current_phase.json`

## 1. Current baseline

From `.ownstack/current_phase.json`:
- `current_phase = 12`
- `phase_0_complete` through `phase_12_complete` are `true`

The project is in post-phase stabilization, not bootstrap mode.

## 2. Completed milestones

- Agent-first runtime mode/state loop (`ask`, `auto`, `plan`) through `UiStateDelta`
- Security chain enforcement (`Policy -> Path -> Sandbox -> ToolResult -> Audit`)
- Policy approval E2E with correlation IDs and timeout handling
- Anti-loop and argument hard limits in orchestrator
- MCP runtime config and integration tests
- Onboarding wizard + OS keyring API key storage
- Python-sidecar bridge path and native-agent path coexistence
- OwnStack UI surfaces (chat, status, audit, MCP, palette, onboarding)
- Python E2E script suite with healthcheck integration
- Rust E2E harness crate (`ownstack-e2e`) with golden-path coverage

Recent stabilization additions:
- E2E screenshot execution hardened (no shell command interpolation)
- E2E launcher pipe deadlock mitigation
- Missing-LSP detection wired to UI notification (`LspNotInstalled`)
- Rust E2E fixture mutation serialization to reduce flaky parallel runs

## 3. Active priorities

### P0 - CI and release hardening

1. Keep `cargo check --workspace --all-targets` green.
2. Keep baseline healthcheck scripts green.
3. Validate complex mission E2E with real API keys in controlled CI.
4. Continue release workflow hardening for signing/notarization and artifact verification.

### P1 - E2E reliability and observability

1. Track and reduce JSON parse/tool-call failure loops in agent orchestration.
2. Keep deterministic E2E startup/runtime behavior (`OWNSTACK_E2E`, window sizing, snapshot tooling).
3. Improve signal-to-noise in E2E logs and artifact retention.

### P1 - Runtime/UI consistency

1. Keep UI state driven from runtime `UiStateDelta`.
2. Keep proxy as lifecycle owner for bridge/agent processes.
3. Prevent local UI logic drift from runtime truth.

## 4. Deferred (non-blocking)

- Expanded settings UX for key management and model selection
- Additional specialist toolkit depth and mission quality improvements
- Broader public release documentation and onboarding material

## 5. Definition of done for current cycle

1. Workspace builds and tests without regression.
2. Core E2E suites remain stable.
3. Security chain remains enforced with no bypass path.
4. Documentation reflects runtime reality and validation commands.
5. Branch remains release-candidate ready for integration/push.
