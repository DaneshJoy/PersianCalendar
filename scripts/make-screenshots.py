"""Compose transparent WebView screenshots onto a neutral backdrop for the docs.

Usage: python scripts/make-screenshots.py
Reads .artifacts/cap-*.png and writes docs/screenshots/*.png.
"""
from __future__ import annotations

import os
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
ARTIFACTS = ROOT / ".artifacts"
OUT = ROOT / "docs" / "screenshots"


def backdrop(width: int, height: int) -> Image.Image:
    image = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    pixels = image.load()
    for y in range(height):
        for x in range(width):
            t = (x / width) * 0.55 + (y / height) * 0.45
            pixels[x, y] = (
                int(32 + 55 * t),
                int(40 + 22 * t),
                int(72 + 90 * t),
                255,
            )
    return image


def compose(filename: str, pad: int = 40) -> None:
    source = ARTIFACTS / filename
    if not source.exists():
        print("skip (missing):", source)
        return
    shot = Image.open(source).convert("RGBA")
    canvas = backdrop(shot.width + pad * 2, shot.height + pad * 2)
    shadow = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(shadow)
    draw.rectangle(
        (pad + 6, pad + 10, pad + shot.width + 6, pad + shot.height + 10),
        fill=(0, 0, 0, 90),
    )
    canvas.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(18)))
    canvas.alpha_composite(shot, (pad, pad))
    target = OUT / filename.replace("cap-", "")
    canvas.convert("RGB").save(target, quality=95)
    print("wrote", target, canvas.size)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name in (
        "cap-popup.png",
        "cap-settings.png",
        "cap-widget-small.png",
        "cap-widget-full.png",
    ):
        compose(name)


if __name__ == "__main__":
    main()
