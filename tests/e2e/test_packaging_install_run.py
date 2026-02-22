import os
import platform
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path


def run_checked(cmd, cwd=None):
    result = subprocess.run(
        cmd,
        cwd=cwd,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"Command failed: {' '.join(cmd)}\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    return result


def run_version(binary_path: Path):
    commands = [
        [str(binary_path), "--version"],
        [str(binary_path), "-V"],
    ]
    last_error = None
    for cmd in commands:
        try:
            run_checked(cmd)
            return
        except Exception as err:  # pragma: no cover
            last_error = err
    raise RuntimeError(f"Could not run version for {binary_path}: {last_error}")


def first_existing(paths):
    for path in paths:
        if path.exists():
            return path
    return None


def check_windows():
    performed = False

    portable_zip = os.getenv("OWNSTACK_WINDOWS_PORTABLE_ZIP")
    if portable_zip:
        zip_path = Path(portable_zip)
        if not zip_path.exists():
            raise FileNotFoundError(f"Portable ZIP not found: {zip_path}")
        with tempfile.TemporaryDirectory() as tmp:
            with zipfile.ZipFile(zip_path) as zf:
                zf.extractall(tmp)
            exe = first_existing(
                [
                    Path(tmp) / "ownstack-ide.exe",
                    Path(tmp) / "OwnStack" / "ownstack-ide.exe",
                ]
            )
            if exe is None:
                candidates = list(Path(tmp).rglob("ownstack-ide.exe"))
                if candidates:
                    exe = candidates[0]
            if exe is None:
                raise FileNotFoundError(
                    f"Could not find ownstack-ide.exe after extracting {zip_path}"
                )
            run_version(exe)
        print(f"[PASS] Portable ZIP executable smoke test: {zip_path}")
        performed = True

    msi_path = os.getenv("OWNSTACK_WINDOWS_MSI")
    if msi_path:
        msi = Path(msi_path)
        if not msi.exists():
            raise FileNotFoundError(f"MSI not found: {msi}")
        if msi.suffix.lower() != ".msi":
            raise RuntimeError(f"Expected .msi artifact, got: {msi}")
        print(f"[PASS] MSI artifact presence check: {msi}")
        performed = True

    python_bundle = os.getenv("OWNSTACK_WINDOWS_PYTHON_BUNDLE")
    if python_bundle:
        bundle_path = Path(python_bundle)
        if not bundle_path.exists():
            raise FileNotFoundError(
                f"Python runtime bundle not found: {bundle_path}"
            )

        if bundle_path.suffix.lower() == ".zip":
            with zipfile.ZipFile(bundle_path) as zf:
                names = set(zf.namelist())
                required = {
                    "start_bridge.py",
                    "ownstack-python/app/bridge_rpc.py",
                }
                missing = [name for name in required if name not in names]
                if missing:
                    raise FileNotFoundError(
                        f"Python bundle missing required files: {missing}"
                    )
        else:
            launch = bundle_path / "start_bridge.py"
            bridge_rpc = bundle_path / "ownstack-python" / "app" / "bridge_rpc.py"
            if not launch.exists():
                raise FileNotFoundError(
                    f"Python bundle launch script missing: {launch}"
                )
            if not bridge_rpc.exists():
                raise FileNotFoundError(
                    f"Python bundle bridge entrypoint missing: {bridge_rpc}"
                )

        print(f"[PASS] Python runtime bundle check: {bundle_path}")
        performed = True

    return performed


def check_linux():
    performed = False

    tarball_path = os.getenv("OWNSTACK_LINUX_TARBALL")
    if tarball_path:
        tarball = Path(tarball_path)
        if not tarball.exists():
            raise FileNotFoundError(f"Linux tarball not found: {tarball}")
        with tempfile.TemporaryDirectory() as tmp:
            with tarfile.open(tarball) as tf:
                tf.extractall(tmp)
            binary = first_existing(
                [
                    Path(tmp) / "OwnStack" / "ownstack-ide",
                    Path(tmp) / "ownstack-ide",
                ]
            )
            if binary is None:
                candidates = list(Path(tmp).rglob("ownstack-ide"))
                if candidates:
                    binary = candidates[0]
            if binary is None:
                raise FileNotFoundError(
                    f"Could not find ownstack-ide in extracted tarball {tarball}"
                )
            binary.chmod(binary.stat().st_mode | 0o111)
            run_version(binary)
        print(f"[PASS] Linux tarball executable smoke test: {tarball}")
        performed = True

    appimage_path = os.getenv("OWNSTACK_LINUX_APPIMAGE")
    if appimage_path:
        appimage = Path(appimage_path)
        if not appimage.exists():
            raise FileNotFoundError(f"AppImage not found: {appimage}")
        appimage.chmod(appimage.stat().st_mode | 0o111)
        try:
            run_checked([str(appimage), "--appimage-version"])
        except Exception:
            run_version(appimage)
        print(f"[PASS] AppImage smoke test: {appimage}")
        performed = True

    flatpak_path = os.getenv("OWNSTACK_LINUX_FLATPAK")
    if flatpak_path:
        flatpak = Path(flatpak_path)
        if not flatpak.exists():
            raise FileNotFoundError(f"Flatpak bundle not found: {flatpak}")
        if flatpak.suffix.lower() != ".flatpak":
            raise RuntimeError(f"Expected .flatpak artifact, got: {flatpak}")
        print(f"[PASS] Flatpak artifact presence check: {flatpak}")
        performed = True

    return performed


def check_macos():
    performed = False

    dmg_path = os.getenv("OWNSTACK_MACOS_DMG")
    if dmg_path:
        dmg = Path(dmg_path)
        if not dmg.exists():
            raise FileNotFoundError(f"DMG not found: {dmg}")
        if dmg.suffix.lower() != ".dmg":
            raise RuntimeError(f"Expected .dmg artifact, got: {dmg}")
        print(f"[PASS] DMG artifact presence check: {dmg}")
        performed = True

    app_bin = os.getenv("OWNSTACK_MACOS_APP_BIN")
    if app_bin:
        app_binary = Path(app_bin)
        if not app_binary.exists():
            raise FileNotFoundError(f"macOS app binary not found: {app_binary}")
        run_version(app_binary)
        print(f"[PASS] macOS app binary smoke test: {app_binary}")
        performed = True

    return performed


def main():
    print("=== PACKAGING INSTALL/RUN E2E TEST ===")
    system = platform.system().lower()
    print(f"Detected platform: {system}")

    if system == "windows":
        performed = check_windows()
    elif system == "linux":
        performed = check_linux()
    elif system == "darwin":
        performed = check_macos()
    else:
        print(f"[SKIP] Unsupported platform: {system}")
        return 0

    if not performed:
        print("[SKIP] No packaging artifact environment variables provided.")
        return 0

    print("SUCCESS: Packaging smoke checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
