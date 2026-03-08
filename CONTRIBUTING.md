# Contributing to OwnStack IDE

Thanks for contributing.

OwnStack IDE is a Lapce-based Rust workspace with additional OwnStack runtime crates.
Please keep contributions aligned with the project directives and phase constraints.

## Before opening a PR

1. Read `GEMINI.md` (full agent/project directives).
2. Read `AGENTS.md` (condensed operational constraints).
3. Read `docs/ARCHITECTURE.md` and confirm your change matches current architecture.
4. Check `.ownstack/current_phase.json` and avoid future-phase implementation.

## Where to discuss

- Discord: https://discord.gg/n8tGJ6Rn6D
- Issues (this repository): https://github.com/psykoniz/Ownstacklapce/issues

## Contribution scope

Typical accepted contributions:
- Bug fixes
- Tests and reliability improvements
- Documentation improvements
- Performance and DX improvements
- Phase-appropriate features only

Out of scope without prior discussion:
- Architecture rewrites
- Security-flow bypasses
- Protected file edits listed in `AGENTS.md`

## Branch and commit guidance

- Keep one logical change per commit.
- Use descriptive commit messages.
- Avoid `WIP` commit titles.
- Do not commit secrets, local caches, build artifacts.

Suggested commit format:
- `feat(scope): summary`
- `fix(scope): summary`
- `docs(scope): summary`
- `test(scope): summary`
- `security(scope): summary`

## Required local checks

Run before pushing:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo check --workspace --all-targets
```

For OwnStack runtime changes, also run:

```bash
python scripts/healthcheck.py
```

## Security and quality rules

- No `unwrap()` in production paths.
- No `println!()` in production paths (use `tracing`).
- No `unsafe` without a `// SAFETY:` explanation.
- Keep security chain intact:
  `Policy -> PathValidator -> Sandbox -> ToolResult -> AuditLog`

## Documentation policy

If behavior changes, update the relevant docs in the same PR:
- `README.md`
- `docs/ARCHITECTURE.md`
- `docs/OPERATIONS.md`
- `docs/ROADMAP.md`
- feature-specific docs under `docs/`
