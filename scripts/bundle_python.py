#!/usr/bin/env python3
"""
OwnStack Python Bundling Configuration

This script prepares the Python runtime and dependencies for bundling
with the OwnStack IDE distribution. It creates a self-contained Python
environment that ownstack-bridge can spawn.

Usage:
    python bundle_python.py --output dist/python_bundle
"""

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path


def bundle_python(output_dir: str) -> None:
    """Create a bundled Python environment with all dependencies."""
    output = Path(output_dir)
    output.mkdir(parents=True, exist_ok=True)
    
    venv_dir = output / "venv"
    
    print(f"[1/4] Creating virtual environment at {venv_dir}")
    subprocess.run(
        [sys.executable, "-m", "venv", str(venv_dir)],
        check=True,
    )
    
    # Determine pip path
    if os.name == "nt":
        pip = venv_dir / "Scripts" / "pip.exe"
        python = venv_dir / "Scripts" / "python.exe"
    else:
        pip = venv_dir / "bin" / "pip"
        python = venv_dir / "bin" / "python"
    
    print("[2/4] Installing dependencies")
    requirements = Path(__file__).parent.parent / "ownstack-python" / "requirements.txt"
    if requirements.exists():
        subprocess.run(
            [str(pip), "install", "-r", str(requirements)],
            check=True,
        )
    else:
        print(f"  WARNING: {requirements} not found, skipping deps")
    
    print("[3/4] Copying ownstack-python source")
    src = Path(__file__).parent.parent / "ownstack-python"
    dst = output / "ownstack-python"
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(src, dst, ignore=shutil.ignore_patterns(
        "__pycache__", "*.pyc", ".pytest_cache", ".venv"
    ))
    
    print("[4/4] Creating launch script")
    launch_script = output / "start_bridge.py"
    launch_script.write_text(
        '#!/usr/bin/env python3\n'
        'import sys, os\n'
        'sys.path.insert(0, os.path.join(os.path.dirname(__file__), "ownstack-python"))\n'
        'from app.bridge_rpc import main\n'
        'main()\n'
    )
    
    print(f"\n✅ Bundle created at: {output}")
    print(f"   Python: {python}")
    print(f"   Launch: python {launch_script}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Bundle Python for OwnStack IDE")
    parser.add_argument(
        "--output", "-o",
        default="dist/python_bundle",
        help="Output directory for the bundle",
    )
    args = parser.parse_args()
    bundle_python(args.output)


if __name__ == "__main__":
    main()
