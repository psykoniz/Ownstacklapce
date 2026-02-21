# OwnStack Operations Runbook

## Scope
This document defines the production execution path, security checks, credential handling, and release validation for OwnStack IDE.

## Mandatory Security Execution Flow
Every tool execution must follow this chain, in order:

`Command -> PolicyEngine -> PathValidator -> Sandbox -> ToolResult -> AuditLog`

Operational rules:

- Never bypass policy approval in `Ask` mode.
- Never execute a tool directly from UI without proxy and policy checks.
- Always write resulting tool/audit events back to the RPC stream.
- Keep dangerous commands blocked by policy defaults.

## Credential and Secret Handling
OwnStack credentials are stored in the OS-native secure store through `keyring`:

- Windows: Credential Manager
- macOS: Keychain
- Linux: Secret Service / Keyring backend

Current entries:

- Service: `OwnStack IDE`
- Username/key: `openrouter_api_key`
- Username/key: `anthropic_api_key`

Storage and load path:

- UI wizard save path: `lapce-app/src/ownstack_onboarding.rs`
- Runtime secret load path: `lapce-proxy/src/dispatch.rs`

Security requirements:

- Do not log secret values.
- Do not persist API keys in JSON config files.
- Keep onboarding state (`ownstack-onboarding.json`) non-secret.

## First-Launch Onboarding
On first launch, onboarding is displayed from `lapce-app/src/app.rs`.

Steps:

1. Provider setup (OpenRouter / Anthropic / Local Ollama).
2. Secure key input for cloud providers.
3. Default mode selection (`Ask`, `Auto`, `Plan`).
4. Workspace policy/budget guidance.
5. Persist runtime state and keyring secrets on `Finish`.

## Status Bar and Agent State
The status bar is the operator signal surface:

- Mode: `Ask | Auto | Plan`
- Runtime state: `running | idle | disconnected`
- Optional detail: stream/mission/audit/policy states

Code paths:

- State model: `lapce-app/src/ownstack_status.rs`
- UI rendering: `lapce-app/src/status.rs`
- Event updates: `lapce-app/src/window_tab.rs`

## E2E and Regression Scripts
E2E scripts live in `tests/e2e/`.

Primary checks:

- `tests/e2e/verify_agent_spawn.py`
- `tests/e2e/verify_sandbox_exec.py`
- `tests/e2e/verify_escape_mitigation.py`
- `tests/e2e/verify_llm_e2e.py`
- `tests/e2e/test_policy_approval.py`
- `tests/e2e/test_wasi_plugin.py`
- `tests/e2e/test_agent_rpc.py`

Quick run:

```bash
python scripts/healthcheck.py
```

## MCP Client Runtime Config
When present, the agent auto-loads MCP servers from:

- `.ownstack/mcp_servers.json`

Example:

```json
{
  "servers": [
    {
      "name": "filesystem",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "."],
      "env": {},
      "enabled": true
    }
  ]
}
```

## Release Validation (Cross-Platform)
Release orchestration is in `.github/workflows/release.yml`.

Validation targets:

- Windows: `.msi` + portable zip (+ optional code signing verification)
- macOS: signed/notarized `.dmg` + staple validation
- Linux: `.deb`, `.rpm`, `.AppImage`, `.flatpak`
- Artifact presence gate: `validate-artifacts` job

## Incident Notes
Windows-specific operational caveats:

- If `ownstack-agent.exe` is running, builds may fail with access denied (`os error 5`).
- If Cargo lock files are stale, terminate orphan `cargo` processes before retry.
- If E2E output is silent, verify the script is executing `target/debug/ownstack-agent(.exe)` and not a stale `target/debug/deps` binary.
