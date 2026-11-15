"""Generate the application icon set for PersianCalendar.

Design: a minimal flat rounded square in the app accent colour (indigo/blue)
with the Persian day number "۲۲" centred in white Vazirmatn, plus a tiny
month band. Outputs:
  - assets/app-icon.png  (1024x1024 source, kept for regeneration)
  - icons/32x32.png, icons/128x128.png, icons/128x128@2x.png, icons/icon.ico

Run with the bundled Python (Pillow required):
  python scripts/make-icon.py
"""
from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
FONT = ROOT / "src-tauri" / "assets" / "Vazirmatn-FD-Bold.ttf"
ICONS = ROOT / "icons"

ACCENT_TOP = (74, 124, 240)
ACCENT_BOTTOM = (37, 78, 190)
WHITE = (255, 255, 255, 255)
BAND = (255, 255, 255, 46)


def rounded_mask(size: int, radius: int) -> Image.Image:
    mask = Image.new("L", (size * 4, size * 4), 0)
    draw = ImageDraw.Draw(mask)
    rr = radius * 4
    draw.rounded_rectangle((0, 0, size * 4 - 1, size * 4 - 1), radius=rr, fill=255)
    return mask.resize((size, size), Image.LANCZOS)


def vertical_gradient(size: int, top: tuple[int, int, int], bottom: tuple[int, int, int]) -> Image.Image:
    img = Image.new("RGBA", (size, size))
    px = img.load()
    for y in range(size):
        t = y / max(1, size - 1)
        r = round(top[0] + (bottom[0] - top[0]) * t)
        g = round(top[1] + (bottom[1] - top[1]) * t)
        b = round(top[2] + (bottom[2] - top[2]) * t)
        for x in range(size):
            px[x, y] = (r, g, b, 255)
    return img


def build(size: int = 1024) -> Image.Image:
    scale = size / 1024.0
    radius = round(220 * scale)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))

    body = vertical_gradient(size, ACCENT_TOP, ACCENT_BOTTOM)
    body.putalpha(rounded_mask(size, radius))
    canvas.alpha_composite(body)

    draw = ImageDraw.Draw(canvas)
    # subtle top band, like the header of a calendar sheet
    band_top = round(112 * scale)
    band_h = round(96 * scale)
    band = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    bd = ImageDraw.Draw(band)
    bd.rounded_rectangle(
        (round(150 * scale), band_top, size - round(150 * scale), band_top + band_h),
        radius=round(34 * scale),
        fill=(255, 255, 255, 40),
    )
    canvas.alpha_composite(band)

    # two white dots = binding rings of a calendar page (minimal hint)
    for cx in (round(360 * scale), round(664 * scale)):
        dot_r = round(17 * scale)
        draw.ellipse(
            (cx - dot_r, band_top - dot_r, cx + dot_r, band_top + dot_r),
            fill=(255, 255, 255, 235),
        )

    # Persian day number (Vazirmatn FD maps ASCII digits to Persian glyphs)
    font_size = round(430 * scale)
    font = ImageFont.truetype(str(FONT), font_size)
    text = "22"
    bbox = draw.textbbox((0, 0), text, font=font)
    tw = bbox[2] - bbox[0]
    th = bbox[3] - bbox[1]
    tx = (size - tw) / 2 - bbox[0]
    ty = (size - th) / 2 - bbox[1] + round(58 * scale)
    draw.text((tx, ty), text, font=font, fill=WHITE)

    return canvas


def main() -> None:
    ICONS.mkdir(parents=True, exist_ok=True)
    master = build(1024)
    master.save(ROOT / "assets" / "app-icon.png")

    sizes = {"32x32.png": 32, "128x128.png": 128, "128x128@2x.png": 256}
    for name, target in sizes.items():
        master.resize((target, target), Image.LANCZOS).save(ICONS / name)

    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    master.resize((256, 256), Image.LANCZOS).save(
        ICONS / "icon.ico",
        format="ICO",
        sizes=[(s, s) for s in ico_sizes],
    )
    print("icon set written to", ICONS)


if __name__ == "__main__":
    main()
