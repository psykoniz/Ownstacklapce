## Installing OwnStack IDE from packages

OwnStack IDE artifacts are published through repository releases.

Release page:
- https://github.com/psykoniz/Ownstacklapce/releases

## Artifact types

Current release pipeline targets:
- Windows: MSI + portable package
- macOS: DMG
- Linux: tar/deb/rpm/AppImage/Flatpak variants

Common artifact names include:
- `OwnStack-linux-x86_64.AppImage`
- `OwnStack-linux-x86_64.flatpak`
- `OwnStack-macos.dmg`
- `OwnStack-windows-x86_64.msi`

## Linux install examples

### Debian/Ubuntu from `.deb`
```bash
sudo apt install ./ownstack-ide*.deb
```

### Fedora/RHEL from `.rpm`
```bash
sudo dnf install ./ownstack-ide*.rpm
```

### AppImage
```bash
chmod +x OwnStack-linux-x86_64.AppImage
./OwnStack-linux-x86_64.AppImage
```

### Flatpak bundle
```bash
flatpak install --user OwnStack-linux-x86_64.flatpak
flatpak run io.ownstack.ownstackide
```

## Windows install examples

### MSI
- Run the downloaded `.msi` installer and follow wizard steps.

### Portable package
- Extract the portable zip and launch `ownstack-ide.exe`.

## macOS install example

1. Open `OwnStack-macos.dmg`
2. Drag `OwnStack.app` to `Applications`
3. Launch from Applications

## Notes

- Package naming may vary slightly by release channel and architecture.
- For reproducible validation, pair package install with `tests/e2e/test_packaging_install_run.py`.
