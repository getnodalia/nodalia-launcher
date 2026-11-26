#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Draws a stand-in app icon: the app's accent on a rounded 128 px tile, with its initial.

Only for an app that has no icon of its own yet. Once the app ships its portrait, copy that icon
into assets/app-icons/<id>.png over the stand-in (never redraw it here).

    packaging/placeholder-icon.py <id> <initial> <R> <G> <B>
"""
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
SCALE = 4
SIZE = 128


def main() -> None:
    app, initial, *rgb = sys.argv[1:]
    color = tuple(int(c) for c in rgb)
    big = SIZE * SCALE
    tile = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(tile)
    draw.rounded_rectangle((0, 0, big - 1, big - 1), radius=int(big * 112 / 512), fill=color + (255,))
    font = ImageFont.truetype(str(ROOT / "assets/fonts/Sora-Variable.ttf"), int(big * 0.58))
    font.set_variation_by_name("Bold")
    draw.text((big / 2, big / 2), initial, font=font, fill=(255, 255, 255, 255), anchor="mm")
    tile.resize((SIZE, SIZE), Image.LANCZOS).save(ROOT / f"assets/app-icons/{app}.png", optimize=True)


if __name__ == "__main__":
    main()
