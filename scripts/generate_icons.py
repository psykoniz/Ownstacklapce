#!/usr/bin/env python3
"""Generate OwnStack IDE icon files from logo_app.svg.

Produces:
  - extra/macos/Lapce.app/Contents/Resources/lapce.icns  (macOS)
  - extra/windows/lapce.ico                                (Windows)
  - extra/images/logo.png                                  (512x512 PNG)
"""

import io
import struct
import subprocess
import sys
from pathlib import Path

import cairosvg
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SVG_PATH = ROOT / "extra" / "images" / "logo_app.svg"

ICNS_OUT = ROOT / "extra" / "macos" / "Lapce.app" / "Contents" / "Resources" / "lapce.icns"
ICO_OUT = ROOT / "extra" / "windows" / "lapce.ico"
PNG_OUT = ROOT / "extra" / "images" / "logo.png"


def svg_to_png(svg_path: Path, size: int) -> Image.Image:
    """Render SVG to a square PNG at the given size."""
    png_data = cairosvg.svg2png(
        url=str(svg_path),
        output_width=size,
        output_height=size,
    )
    return Image.open(io.BytesIO(png_data)).convert("RGBA")


def make_icns(images: dict[int, Image.Image], out_path: Path):
    """Create a minimal .icns file with the standard icon sizes."""
    # macOS icns type codes for PNG-encoded icons
    type_map = {
        16: b"icp4",    # 16x16
        32: b"icp5",    # 32x32
        64: b"icp6",    # 64x64
        128: b"ic07",   # 128x128
        256: b"ic08",   # 256x256
        512: b"ic09",   # 512x512
        1024: b"ic10",  # 1024x1024
    }

    entries = []
    for size, type_code in type_map.items():
        if size in images:
            buf = io.BytesIO()
            images[size].save(buf, format="PNG")
            png_bytes = buf.getvalue()
            # Each entry: 4-byte type + 4-byte length (includes header) + data
            entry_data = type_code + struct.pack(">I", len(png_bytes) + 8) + png_bytes
            entries.append(entry_data)

    body = b"".join(entries)
    icns_data = b"icns" + struct.pack(">I", len(body) + 8) + body

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(icns_data)
    print(f"  Created {out_path} ({len(icns_data)} bytes)")


def make_ico(images: dict[int, Image.Image], out_path: Path):
    """Create a .ico file with standard Windows icon sizes."""
    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    ico_images = []
    for s in ico_sizes:
        if s in images:
            ico_images.append(images[s])
        else:
            # Resize from nearest larger
            for candidate in sorted(images.keys()):
                if candidate >= s:
                    ico_images.append(images[candidate].resize((s, s), Image.LANCZOS))
                    break

    out_path.parent.mkdir(parents=True, exist_ok=True)
    ico_images[0].save(
        str(out_path),
        format="ICO",
        sizes=[(img.width, img.height) for img in ico_images],
        append_images=ico_images[1:],
    )
    print(f"  Created {out_path} ({out_path.stat().st_size} bytes)")


def main():
    if not SVG_PATH.exists():
        print(f"ERROR: SVG not found at {SVG_PATH}", file=sys.stderr)
        sys.exit(1)

    print(f"Source: {SVG_PATH}")
    print("Rendering SVG at multiple sizes...")

    sizes = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
    images = {}
    for s in sizes:
        images[s] = svg_to_png(SVG_PATH, s)
        print(f"  {s}x{s} OK")

    # 1. macOS .icns
    print("\nGenerating macOS .icns...")
    make_icns(images, ICNS_OUT)

    # 2. Windows .ico
    print("Generating Windows .ico...")
    make_ico(images, ICO_OUT)

    # 3. Main logo PNG (512x512)
    print("Generating logo.png (512x512)...")
    images[512].save(str(PNG_OUT), format="PNG")
    print(f"  Created {PNG_OUT} ({PNG_OUT.stat().st_size} bytes)")

    print("\nDone! All icons generated from OwnStack logo.")


if __name__ == "__main__":
    main()
