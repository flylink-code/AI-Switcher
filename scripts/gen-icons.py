#!/usr/bin/env python3
"""Generate the AI-Switcher icon set from the approved high-resolution mark."""
import sys
from pathlib import Path

try:
    from PIL import Image, ImageDraw
except ImportError:
    print("Pillow is required: pip install Pillow", file=sys.stderr)
    sys.exit(1)

OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("src-tauri/icons")
OUT.mkdir(parents=True, exist_ok=True)
SOURCE = OUT / "brand-mark.png"


def make(size: int) -> Image.Image:
    """Resize the canonical mark and apply transparent rounded corners."""
    if not SOURCE.is_file():
        raise FileNotFoundError(f"Missing source icon: {SOURCE}")
    img = Image.open(SOURCE).convert("RGBA").resize((size, size), Image.Resampling.LANCZOS)
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, size - 1, size - 1], radius=round(size * 0.18), fill=255
    )
    img.putalpha(mask)
    return img


# PNGs at the sizes Tauri's config references + extra app-store sizes.
png_specs = {
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 512,
    "Square30x30Logo.png": 30,
    "Square44x44Logo.png": 44,
    "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89,
    "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142,
    "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284,
    "Square310x310Logo.png": 310,
    "StoreLogo.png": 50,
}
for name, size in png_specs.items():
    make(size).save(OUT / name, "PNG")
    print(f"wrote {name} ({size}x{size})")

# ICO (multi-size) and ICNS via Pillow's savers.
ico_sizes = [(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
base = make(256)
base.save(OUT / "icon.ico", sizes=ico_sizes)
print("wrote icon.ico")

# ICNS is only supported on macOS Pillow builds; fall back to copying the PNG
# under the icns name so the config reference resolves on all platforms.
# (Tauri only needs icns when bundling for macOS, which P0 does not target.)
try:
    base.save(OUT / "icon.icns")
    print("wrote icon.icns")
except Exception as e:  # noqa: BLE001
    make(512).save(OUT / "icon.icns", "PNG")
    print(f"wrote icon.icns (PNG fallback: {e})")

print("done")
