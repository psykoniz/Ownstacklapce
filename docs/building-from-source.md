## Building OwnStack IDE from source

OwnStack IDE is a Rust workspace.
Builds are performed with Cargo.

## 1. Prerequisites

- Rust toolchain (recommended via https://rustup.rs)
- Git
- Platform dependencies below

### Ubuntu / Debian
```sh
sudo apt install clang libxkbcommon-x11-dev pkg-config libvulkan-dev libwayland-dev xorg-dev libxcb-shape0-dev libxcb-xfixes0-dev libssl-dev
```

### Fedora
```sh
sudo dnf install clang libxkbcommon-x11-devel libxcb-devel vulkan-loader-devel wayland-devel openssl-devel pkgconf
```

### macOS
```sh
xcode-select --install
```

### Windows
- Visual Studio Build Tools with C++ workload
- Latest stable Rust toolchain

## 2. Clone repository

```sh
git clone https://github.com/psykoniz/Ownstacklapce.git
cd Ownstacklapce
```

## 3. Build binaries

### Release editor binary
```sh
cargo build --release -p lapce-app --bin ownstack-ide
```

### Release agent binary (optional explicit build)
```sh
cargo build --release -p ownstack-agent --bin ownstack-agent
```

## 4. Run

### Linux/macOS
```sh
./target/release/ownstack-ide
```

### Windows (PowerShell)
```powershell
.\target\release\ownstack-ide.exe
```

## 5. Development workflow

### Fast debug run
```sh
cargo run -p lapce-app --bin ownstack-ide
```

### Workspace checks
```sh
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo check --workspace --all-targets
cargo test --workspace
```

## 6. E2E and health checks

### Python script healthcheck
```sh
python scripts/healthcheck.py
```

### Rust E2E harness compile check
```sh
cargo test -p ownstack-e2e --no-run
```

Headless Linux note:
- use `xvfb-run` for UI-driven E2E tests when no display server is available.

## 7. Packaging builds

Release packaging is orchestrated by:
- `.github/workflows/release.yml`
- platform scripts under `scripts/`

For local packaging attempts, review:
- `scripts/build_windows_installer.ps1`
- `scripts/build_appimage.sh`
- `scripts/build_flatpak_bundle.sh`
