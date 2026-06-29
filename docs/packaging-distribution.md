# Packaging & Distribution (Windows)

How OwnStack IDE is built into a shippable Windows distribution, and the
performance profile used for it.

## What ships

Every Windows artifact must contain **both** executables:

| File | Role |
|------|------|
| `ownstack-ide.exe`   | The editor (Lapce fork + Floem GUI). |
| `ownstack-agent.exe` | The embedded AI agent. The IDE spawns it from **its own directory** (`current_exe().parent()`), so an artifact without it ships a dead AI bridge. |

> ⚠️ This was a real packaging bug: the MSI and the portable zip originally
> bundled only `ownstack-ide.exe`. Installed users had a working editor with no
> AI. Both are now fixed (see `extra/windows/wix/lapce.wxs` `OwnstackAgentExe`
> component, and the `Create portable` step in `.github/workflows/release.yml`).

`lapce-proxy.exe` ships separately as a gzip (the IDE downloads/uses it as the
workspace proxy).

## Build profiles

Defined in the workspace `Cargo.toml`:

| Profile | Use | Settings |
|---------|-----|----------|
| `dev` | local dev | `opt-level=0`, incremental, but deps built at `opt-level=3` (snappy debug runs). |
| `release` | fast optimized build for local perf checks | cargo defaults (`opt-level=3`, no LTO). |
| `release-lto` | **the distribution profile** | `inherits = release` + `lto = true` + `codegen-units = 1` + `strip = "symbols"`. Slowest to compile, smallest/fastest binary. Output in `target/release-lto/`. |

`panic` strategy is deliberately left at `unwind`: Floem/winit and the proxy rely
on `catch_unwind`, so `panic = "abort"` is **not** used despite its size win.

## One-command local packaging

```powershell
pwsh scripts/package-windows.ps1                  # release-lto (official, slow)
pwsh scripts/package-windows.ps1 -Profile release # faster, still optimized
pwsh scripts/package-windows.ps1 -SkipBuild       # package already-built binaries
```

It produces under `dist/`:

- `OwnStack/` — staged folder (ide + agent + icon)
- `OwnStack-windows-portable.zip` — complete portable package
- `OwnStack-windows.msi` — installer (only if WiX is present)

The script auto-detects WiX (`C:\Program Files (x86)\WiX Toolset*\bin`). When
packaging a non-`release-lto` profile it copies the binaries into
`target/release-lto/` first, because the `.wxs` reads from that path.

## MSI (WiX)

`extra/windows/wix/lapce.wxs` (WiX v3) defines:

- `LapceExe` + `OwnstackAgentExe` components (both binaries in
  `%ProgramFiles%\OwnStack IDE`)
- Start-menu shortcut, "Open OwnStack IDE here" context menu
- Adds the install dir to the system `PATH`

Build manually:

```powershell
candle.exe -arch x64 -ext WixUIExtension -ext WixUtilExtension -out dist/ownstack-ide.wixobj extra/windows/wix/lapce.wxs
light.exe  -ext WixUIExtension -ext WixUtilExtension -sice:ICE61 -sice:ICE91 -out dist/OwnStack-windows.msi dist/ownstack-ide.wixobj
```

## CI release

`.github/workflows/release.yml` (Windows job):

1. `cargo build --frozen --profile release-lto`
2. Code-sign `ownstack-ide.exe` **and** `ownstack-agent.exe` (gated on
   `WINDOWS_CODESIGN_ENABLED`)
3. Build + sign the MSI
4. Build the portable variant (`--features lapce-app/portable`) and zip **both**
   exes
5. gzip `lapce-proxy.exe`
6. Upload `OwnStack-windows-portable.zip` + `lapce-proxy-windows-*.gz`

## Runtime config the artifacts rely on

- API keys live in the **OS keyring** (Windows Credential Manager), never in any
  shipped file.
- `~/.ownstack/provider.json` holds **non-secret** provider config (base_url,
  model, wire_api).
- The agent process is bound to the IDE via a Windows **job object**
  (`KILL_ON_JOB_CLOSE`) so a crash/force-kill of the IDE cannot leave orphaned
  `ownstack-agent.exe` processes.
