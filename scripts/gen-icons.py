#!/usr/bin/env python3
"""Generate placeholder icons for Claude Switcher.

Creates a simple branded square (gradient + "CS" monogram) and emits the full
Tauri icon set. Run once during scaffolding; output is committed.
"""
import sys
from pathlib import Path

try:
    from PIL import Image, ImageDraw, ImageFont
except ImportError:
    print("Pillow is required: pip install Pillow", file=sys.stderr)
    sys.exit(1)

OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("src-tauri/icons")
OUT.mkdir(parents=True, exist_ok=True)


def make(size: int) -> Image.Image:
    """Render a square icon of the given size with a CS monogram."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # Rounded square background — a calm indigo gradient look (solid for simplicity).
    bg = (88, 101, 242)  # Discord-blurple-ish indigo
    radius = int(size * 0.18)
    draw.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=bg)

    # "CS" monogram centered.
    font = None
    for candidate in ["arial.ttf", "DejaVuSans-Bold.ttf", "Arial.ttf"]:
        try:
            font = ImageFont.truetype(candidate, int(size * 0.5))
            break
        except OSError:
            continue
    if font is None:
        font = ImageFont.load_default()

    text = "CS"
    bbox = draw.textbbox((0, 0), text, font=font)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    x = (size - tw) / 2 - bbox[0]
    y = (size - th) / 2 - bbox[1]
    draw.text((x, y), text, fill=(255, 255, 255, 255), font=font)
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
