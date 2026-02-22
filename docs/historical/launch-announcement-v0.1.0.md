# OwnStack IDE v0.1.0 Launch Announcement (Draft)

## Headline
OwnStack IDE v0.1.0 is live: native Rust editor performance with secure built-in AI operations.

## What is new

- First-launch onboarding wizard for provider and mode setup.
- Secure API key storage via OS-native keychain/credential stores.
- Agent status surface in bottom bar (`Ask/Auto/Plan`, `running/idle/disconnected`).
- Cross-platform release packaging:
  - Windows: MSI + portable build
  - macOS: signed/notarized DMG
  - Linux: deb/rpm/AppImage/Flatpak artifacts
- Release workflow validation in CI before publish.

## Security message

OwnStack operations are guarded by an explicit chain:

`PolicyEngine -> PathValidator -> Sandbox -> AuditLog`

In `Ask` mode, risky actions require explicit user approval.

## Suggested channels

- GitHub Release notes (`v0.1.0`)
- Project README and docs update
- Discord announcement post
- X/LinkedIn short launch post

## Short social post

OwnStack IDE v0.1.0 is out.
Native Rust code editing + embedded AI agent workflows, with policy checks, sandboxed execution, audit trails, secure key storage, and cross-platform installers.
