#!/usr/bin/env python3
"""Draw the Finder window background of Nodalia Launcher's DMG, as an SVG.

    pip install fonttools
    python3 packaging/macos/dmg/draw_background.py <nodalia-design checkout> background.svg

The layout, colours and outlined Sora text come from nodalia-design's
tools/dmg-background/draw_background.py, the same file every Nodalia app's window is drawn with,
so the family reads as one; only the launcher's motif and arrow live here: the grid of the
Nodalia apps with one of them being installed.
"""
import importlib.util
import sys
from pathlib import Path

if len(sys.argv) != 3:
    sys.exit(__doc__)
design = Path(sys.argv[1])
spec = importlib.util.spec_from_file_location("nodalia_dmg", design / "tools/dmg-background/draw_background.py")
dmg = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dmg)

AW, AH, INK, AL, AR, AYc = dmg.AW, dmg.AH, dmg.INK, dmg.AL, dmg.AR, dmg.AYc


def motif(c, d):
    o = [f'<rect width="{AW}" height="{AH}" fill="{c}"/>']
    cols, rows, size, gap = 4, 5, 40, 10
    x0 = (AW - (cols * size + (cols - 1) * gap)) / 2
    y0 = 34
    for i in range(cols * rows):
        x = x0 + (i % cols) * (size + gap)
        y = y0 + (i // cols) * (size + gap)
        if i == 6:
            # the app being installed: filled, with its progress underneath
            o.append(f'<rect x="{x}" y="{y}" width="{size}" height="{size}" rx="11" fill="#FFFFFF"/>')
            o.append(f'<rect x="{x + 6}" y="{y + size - 9}" width="{size - 12}" height="4" rx="2" fill="{d}" opacity=".3"/>')
            o.append(f'<rect x="{x + 6}" y="{y + size - 9}" width="{(size - 12) * 0.6:.1f}" height="4" rx="2" fill="{d}"/>')
        else:
            o.append(f'<rect x="{x}" y="{y}" width="{size}" height="{size}" rx="11" fill="{d}" opacity="{0.55 if i % 3 else 0.8}"/>')
    # a release strip: the version it fetches and a button to update everything
    o.append(f'<rect x="{x0}" y="320" width="{AW - 2 * x0}" height="34" rx="17" fill="{INK}"/>')
    o.append(f'<path d="M{x0 + 20},{327} L{x0 + 20},{343} M{x0 + 13},{337} L{x0 + 20},{344} L{x0 + 27},{337}" fill="none" stroke="#FFFFFF" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>')
    o.append(f'<rect x="{x0 + 40}" y="333" width="{AW - 2 * x0 - 60}" height="6" rx="3" fill="#FFFFFF" opacity=".85"/>')
    return "".join(o)


def arrow(c):
    # a download arrow into Applications
    return (f'<line x1="{AL}" y1="{AYc}" x2="{AR - 12}" y2="{AYc}" stroke="{c}" stroke-width="2.4" stroke-linecap="round"/>'
            + dmg.arrowhead(AR, AYc, c))


dmg.APPS["launcher"] = ("Nodalia Launcher", "Instala y actualiza las apps de Nodalia", "#2CC5D3", "#168E9B", motif, arrow)
Path(sys.argv[2]).write_text(dmg.svg("launcher"))
