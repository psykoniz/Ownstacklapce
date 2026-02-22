# Obsolete Files Audit (Codebase Explorer)

Date: 2026-02-22  
Scope: docs + workflow scripts + root artifacts

## Method Used

1. Ran codebase-explorer workflow tooling:
   - `.agent/workflows/scripts/quick-scan.ps1` (failed)
   - `.agent/workflows/scripts/check-rust-modules.ps1` (failed)
   - `.agent/workflows/scripts/check-python-modules.ps1` (failed)
   - `.agent/workflows/scripts/analyze_workspace.py` (passed)
2. Fallback manual scan (equivalent checks):
   - file inventory (`docs/`, `.agent/workflows/`, `.ownstack/`)
   - stale/legacy keyword grep
   - cross-reference scan of docs from repo
   - root artifact inspection

## Findings

## 1) Broken Workflow Scripts (Windows PowerShell) - High

The PowerShell versions of codebase-explorer scripts are currently not executable on Windows due to parsing/encoding corruption:

- `.agent/workflows/scripts/quick-scan.ps1`
- `.agent/workflows/scripts/check-rust-modules.ps1`
- `.agent/workflows/scripts/check-python-modules.ps1`

Observed errors include:
- unexpected tokens
- missing string terminator
- malformed braces/regex parsing

Impact:
- workflow cannot be reliably run from PowerShell on this repo.

Recommendation:
- regenerate `.ps1` scripts from clean UTF-8 sources, or
- keep only `.sh` scripts and document Git Bash requirement.

## 2) Phase Documentation Drift - High

Current runtime state:
- `.ownstack/current_phase.json` -> `current_phase = 12`

Conflicting documentation:
- `AGENTS.md` states "Currently: Phase 4 (Distribution)"
- `docs/ROADMAP.md` states "current_phase = 4"
- `docs/ARCHITECTURE.md` roadmap section still says "Phase 0 ... EN COURS"

Impact:
- agent/operator guidance is contradictory.
- increases risk of wrong phase gating decisions.

Recommendation:
- align `AGENTS.md`, `docs/ROADMAP.md`, and roadmap statuses in `docs/ARCHITECTURE.md` with `.ownstack/current_phase.json`.

## 3) Versioned Build Output Artifacts in Repo Root - High

The following files appear to be command-output snapshots and are tracked in Git:

- `build_final_check_2.txt`
- `build_final_check_3.txt`
- `build_warnings.txt`
- `build_warnings_latest.txt`
- `check_lapce_proxy_latest.txt`
- `check_ownstack_agent_latest.txt`

Why considered obsolete:
- content is ephemeral command output (PowerShell stderr/stdout captures),
- not referenced by docs/README,
- newer checks are reproducible via CI/`scripts/healthcheck.py`.

Recommendation:
- remove from version control and keep generated output ignored only.

## 4) Unlinked/Working Docs (Archive Candidates) - Medium

Several docs are not linked from `README.md` and appear to be one-off operational snapshots:

- `docs/CODEBASE_ANALYSIS_REPORT.md`
- `docs/NEXT_IMPROVEMENTS_PLAN.md`
- `docs/PHASE_AUDIT_0_12.md`
- `docs/TOP_PYTHON_DEBTS.md`
- `docs/launch-announcement-v0.1.0.md`
- `docs/new-release.md`

Note:
- these may still be useful, but they read as "working artifacts" rather than stable product docs.

Recommendation:
- move to `docs/archive/` or `docs/reports/` and link explicitly from an index.

## 5) Local Debug Logs Present (Ignored) - Low

Local root logs found:
- `wasi_debug.log`
- `wasi_debug_utf8.log`
- `wasi_final.log`
- `wasi_final_utf8.log`

Status:
- ignored by `.gitignore` (`*.log`), so they are local noise only.

Recommendation:
- optional cleanup from working directory; no repo action required.

## Proposed Cleanup Order

1. Fix/replace broken `.ps1` workflow scripts.
2. Reconcile phase docs with `.ownstack/current_phase.json`.
3. Remove tracked build output `.txt` artifacts.
4. Archive unlinked historical reports under `docs/archive/`.

