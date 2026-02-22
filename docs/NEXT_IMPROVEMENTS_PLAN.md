# NEXT IMPROVEMENTS PLAN

Date: 2026-02-21
Branch baseline: claude/verify-branch-functionality-dlozo (7eabfb1)

## Status

Execution status: COMPLETED (2026-02-21)

Delivered:
- P1.1 Audit file locking (`audit.jsonl`) with concurrency-safe JSONL writes.
- P1.2 Shell-aware policy parser + compatibility behavior/tests.
- P2 Tool namespacing with canonical ids, provider-safe encoding, and legacy alias compatibility.
- P3.1 Healer LLM fallback with strict JSON suggestions and policy-safe execution (no auto-exec for `Ask`).
- P3.2 Multivers parallel execution via `tokio` tasks + bounded concurrency + deterministic tie-break.
- P3.3 SemanticIndex persistence (manifest/version/checksum + payload integrity + rebuild on load).

## 0) Immediate fixes done

- Fixed E2E policy-block assertion path (`stderr`/`error`) in `tests/e2e/verify_escape_mitigation.py`.
- Added `verify_escape_mitigation.py` to `scripts/healthcheck.py`.
- Added per-script timeout override in `scripts/healthcheck.py` (`test_wasi_plugin.py` -> 240s).
- Replaced hardcoded `session_id: "system"` with per-session UUID in `ownstack-engine/src/security.rs`.

Validation executed:
- `python tests/e2e/verify_escape_mitigation.py` -> PASS
- `python scripts/healthcheck.py` -> PASS
- `cargo test -p ownstack-engine --quiet` -> PASS

## 1) Priority roadmap

### P1 - Security and correctness (must ship first)

#### 1.1 Audit log file locking (`audit.jsonl`)
Goal: guarantee safe concurrent writes across threads/processes.

Implementation:
- Add cross-process lock before append in `ownstack-engine/src/audit.rs`.
- Keep lock scope minimal (open -> lock -> write -> flush -> unlock).
- Fallback behavior: explicit error in `ToolResult.stderr` and tracing warning.

Files:
- `ownstack-engine/src/audit.rs`
- `ownstack-engine/src/security.rs` (if propagation needed)
- `ownstack-engine/tests/*` (new concurrency tests)

Acceptance:
- No truncated/interleaved JSON lines under concurrent stress.
- Existing audit tests remain green.

#### 1.2 Policy parser upgrade (shell-aware)
Goal: replace fragile `contains()` checks with tokenized parsing.

Implementation:
- Parse shell command into argv-like tokens (quoted strings, escapes, pipes, redirects).
- Run policy rules on parsed command graph (commands + pipe chain + redirects).
- Keep existing blocklist semantics as compatibility tests.

Files:
- `ownstack-engine/src/policy.rs`
- `ownstack-engine/tests/*` (new parser edge-case tests)

Acceptance:
- Same behavior for current blocked cases.
- Better precision for quoted/escaped inputs.

### P2 - Tool execution model hardening

#### 2.1 Tool namespacing (`toolkit:tool`)
Goal: avoid collisions between tool names exposed by different toolkits.

Implementation:
- Introduce canonical tool id format: `namespace:tool`.
- Maintain backward alias support for one release cycle (`exec` -> `core:exec`).
- Update orchestrator/tool-call routing and tool listing payloads.

Files:
- `ownstack-agent/src/toolkits/mod.rs`
- `ownstack-agent/src/orchestrator.rs`
- `ownstack-agent/src/provider.rs` (tool schema text)
- `ownstack-agent/tests/*` and E2E tool-call tests

Acceptance:
- No ambiguous dispatch.
- Legacy prompts still work during compatibility window.

### P3 - Intelligence features

#### 3.1 Healer LLM-backed fallback
Goal: keep rule-based fast path, add LLM fallback for complex failures.

Implementation:
- Keep existing pattern analyzer as stage 1.
- If confidence is low or no deterministic fix found, call planner model with a strict JSON schema.
- Enforce safe output contract (no direct execution without policy approval).

Files:
- `ownstack-agent/src/toolkits/healer.rs`
- `ownstack-agent/src/orchestrator.rs`
- `ownstack-agent/src/provider.rs` (schema/tooling)

Acceptance:
- Deterministic cases unchanged.
- Complex stack traces return structured actionable fixes.

#### 3.2 Multivers parallelization (`tokio::spawn`)
Goal: reduce latency for variant evaluation.

Implementation:
- Parallelize variant generation/evaluation with bounded concurrency.
- Preserve deterministic ranking and cancellation on best score threshold.

Files:
- `ownstack-agent/src/toolkits/multivers.rs`
- `ownstack-agent/tests/*` stress tests

Acceptance:
- Lower wall-clock time on N variants.
- No race-induced nondeterministic output in tests.

#### 3.3 SemanticIndex persistence (real HNSW persistence)
Goal: replace placeholder persistence with durable index storage.

Implementation:
- Persist vectors + metadata + versioned manifest.
- Add load-time integrity checks (dimension, model id, checksum).
- Add migration guard for schema version changes.

Files:
- `ownstack-agent/src/index.rs`
- `ownstack-agent/tests/*` persistence/load tests

Acceptance:
- Rebuild not required between agent restarts.
- Corrupted index detected with explicit recovery path.

## 2) Delivery plan by waves

Wave A (1-2 days):
- Audit file locking
- Policy parser skeleton + parity tests

Wave B (2-3 days):
- Tool namespacing + compatibility aliases
- Regression pass on tool-call E2E

Wave C (3-5 days):
- Healer LLM fallback
- Multivers parallel execution

Wave D (3-5 days):
- SemanticIndex persistence + migration checks

## 3) Mandatory validation gates per wave

- `cargo check --workspace --all-targets --frozen`
- `cargo test --workspace`
- `python scripts/healthcheck.py`
- Targeted E2E:
  - `python tests/e2e/test_mcp_handshake.py`
  - `python tests/e2e/verify_escape_mitigation.py`
  - `python tests/e2e/verify_llm_e2e.py` (with API key)

## 4) Risk management

- Tool namespacing and policy parser are behavioral changes: ship behind feature flags first.
- Healer LLM fallback must never bypass PolicyEngine/Sandbox/Audit flow.
- Semantic index persistence must include versioning from day 1.
