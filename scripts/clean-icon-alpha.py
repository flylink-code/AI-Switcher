"""Strip white fringe / opaque corners from app icons for Linux docks."""

from __future__ import annotations

import math
import sys
from pathlib import Path

from PIL import Image, ImageDraw


def rounded_mask(size: int, radius: float) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(mask)
    # Slight inset so AA fringe outside the squircle becomes fully transparent.
    inset = max(1, size // 256)
    draw.rounded_rectangle(
        (inset, inset, size - 1 - inset, size - 1 - inset),
        radius=radius,
        fill=255,
    )
    return mask


def clean_icon(src: Path, dst: Path) -> None:
    image = Image.open(src).convert("RGBA")
    size = image.width
    if image.height != size:
        raise SystemExit(f"expected square icon, got {image.size}")

    # App icons use a fairly large corner radius (~22% of side).
    radius = size * 0.22
    mask = rounded_mask(size, radius)

    pixels = image.load()
    mask_px = mask.load()
    for y in range(size):
        for x in range(size):
            r, g, b, a = pixels[x, y]
            m = mask_px[x, y]
            if m == 0:
                pixels[x, y] = (0, 0, 0, 0)
                continue
            # Kill near-white fringe that survived anti-aliasing against a white export bg.
            brightness = (r + g + b) / 3.0
            if brightness >= 230 and a < 255:
                pixels[x, y] = (0, 0, 0, 0)
                continue
            if brightness >= 245 and min(r, g, b) >= 230:
                # Fully opaque white-ish edge pixels also go transparent.
                dist_edge = min(x, y, size - 1 - x, size - 1 - y)
                if dist_edge <= max(2, size // 64):
                    pixels[x, y] = (0, 0, 0, 0)
                    continue
            if m < 255:
                # Soften mask edge: keep content color, scale alpha.
                pixels[x, y] = (r, g, b, int(a * (m / 255.0)))

    image.save(dst)
    # Verify corners are transparent.
    for point in ((0, 0), (size - 1, 0), (0, size - 1), (size - 1, size - 1)):
        if image.getpixel(point)[3] != 0:
            raise SystemExit(f"corner still opaque at {point}: {image.getpixel(point)}")
    print(f"cleaned {src.name} -> {dst} ({size}x{size})")


def main() -> None:
    root = Path(__file__).resolve().parents[1] / "src-tauri" / "icons"
    src = root / "icon.png"
    backup = root / "icon.png.bak"
    if not backup.exists():
        backup.write_bytes(src.read_bytes())
    clean_icon(src, src)
    for name in ("32x32.png", "128x128.png", "128x128@2x.png"):
        path = root / name
        if path.exists():
            clean_icon(path, path)


if __name__ == "__main__":
    main()
