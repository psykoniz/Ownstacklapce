# Updating versioning for package managers and whatnot

- App metainfo: `extra/linux/io.ownstack.ownstackide.metainfo.xml`
- macOS plist (`CFBundleShortVersionString`): `extra/macos/Lapce.app/Contents/Info.plist`
- Rust: `Cargo.toml`
- Obviously changelog: `CHANGELOG.md`
- RPM spec: `lapce.spec`
- Windows wix spec (`<Product [...] Version=X.X.X>`): `extra/windows/wix/lapce.wxs`

## CI/CD release workflow

Release pipeline source:

- `.github/workflows/release.yml`

Main jobs:

1. `windows`: build `ownstack-ide.exe`, produce `OwnStack-windows.msi`, optional code signing.
2. `macos`: produce `OwnStack-macos.dmg`, notarize, staple, validate.
3. `linux`: produce binary tarballs + vendored dependencies.
4. `deb` / `rpm`: distro packaging matrix.
5. `appimage` / `flatpak`: Linux portable package artifacts.
6. `validate-artifacts`: hard gate that required artifacts exist before publish.
7. `publish`: uploads all artifacts to GitHub Releases.

## Expected release artifacts

- `OwnStack-windows.msi`
- `OwnStack-windows-portable.zip`
- `OwnStack-macos.dmg`
- `ownstack-linux-amd64.tar.gz`
- `ownstack-linux-arm64.tar.gz`
- `OwnStack-linux-x86_64.AppImage`
- `OwnStack-linux-x86_64.flatpak`
- `.deb` outputs (Debian/Ubuntu matrix)
- `.rpm` outputs (Fedora matrix)

## Manual validation checklist

Before publishing `vX.Y.Z`:

1. Trigger `release.yml` from tag push or `workflow_dispatch`.
2. Ensure `validate-artifacts` is green.
3. On macOS job, confirm notarization + staple validation completed.
4. On Windows job, confirm signing verification step passes when cert secrets are configured.
5. Verify release notes and `CHANGELOG.md` match the tag contents.
