# AGENTS.md — Codex/AI Agent Directives

> **See GEMINI.md for the complete source of truth.**  
> This file is a condensed version for quick reference.

---

## Quick Reference

### Current Phase: See `.ownstack/current_phase.json`

**Source of truth**: `.ownstack/current_phase.json`  
**Currently**: Phase 12 (Team Orchestration & Ecosystem)

**ALLOWED**: Phase-appropriate work per `GEMINI.md` (multi-agent, model routing, WASI plugins, toolkit migration), docs, bug fixes  
**NOT ALLOWED**: Skipping phases, touching protected files, bypassing the security flow

---

## Before Any Action

1. Read `GEMINI.md` (full directives)
2. Read `docs/ARCHITECTURE.md` (structure)
3. Check `.ownstack/current_phase.json`

---

## Protected Files (NEVER modify)

- `LICENSE` (Apache 2.0)
- `lapce-core/src/buffer.rs`
- `lapce-core/src/syntax.rs`
- `.rustfmt.toml`, `.taplo.toml`, `deny.toml`

---

## Security Flow (Mandatory)

```
Command → PolicyEngine → PathValidator → Sandbox → ToolResult → AuditLog
```

All steps required. No bypassing.

---

## Blocked Commands

`rm -rf /`, `sudo *`, `curl | sh`, `chmod 777`, `mkfs`, `dd if=`, `shutdown`, `reboot`

---

## Code Standards

- No `unwrap()` in production
- No `println!()` — use `tracing`
- No `unsafe` without `// SAFETY:` comment
- All new files need tests
- `cargo test` must pass before commit

---

## When Blocked

1. Re-read GEMINI.md
2. Check if task matches current phase
3. If phase future → STOP
4. If unclear → ASK human

---

*For complete directives, see GEMINI.md*
