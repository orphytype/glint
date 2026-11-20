"""Writes Glint's icon files from the mark's outlines (the same functions as crates/glint/src/mark.rs):
the light pebble with its glint cut out, on a dark rounded tile.

    pip install resvg-py pillow fonttools
    python scripts/icons.py

Writes brand/svg/app-icon.svg, docs/glint.png (the README's picture), crates/glint/assets/glint.ico (16 to 128 px as 32-bit bitmaps, 256 px
as PNG, which the app cuts out for its window and tray), packaging/linux/icons/*.png and
packaging/macos/glint.iconset/*.png. Change the numbers here and in mark.rs together.

It also writes crates/glint/src/wordmark.rs: the word "glint" from brand/svg/wordmark-ink.svg as
flat outlines, which the launch screen draws.
"""
import io
import math
import os
import re
import struct

import resvg_py
from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TILE, PAPER = "#121212", "#f4f2ee"
GRID, EDGE, SEED = 48.0, 2.0, 1.9
GLINT_AT, GLINT_REACH = (19.0, 17.0), 20.0 * (44.0 / 48.0) * 0.5


def pebble_r(a):
    w = 0.045 * math.sin(3 * a + SEED) + 0.030 * math.sin(5 * a + SEED * 1.7) + 0.018 * math.sin(7 * a + SEED * 2.3)
    return 1 + 0.45 * w


def glint_r(a):
    tri = abs((((a + math.pi / 4) / (math.pi / 2)) % 1.0) * 2 - 1)
    return 0.30 + 0.70 * (1 - tri) ** 2.4


def pebble(n):
    pts = [(pebble_r(i / 1024 * math.tau) * math.cos(i / 1024 * math.tau), pebble_r(i / 1024 * math.tau) * math.sin(i / 1024 * math.tau)) for i in range(1024)]
    x0, x1 = min(p[0] for p in pts), max(p[0] for p in pts)
    y0, y1 = min(p[1] for p in pts), max(p[1] for p in pts)
    c, k = ((x0 + x1) / 2, (y0 + y1) / 2), (GRID - 2 * EDGE) / max(x1 - x0, y1 - y0)
    out = []
    for i in range(n):
        a = i / n * math.tau
        r = pebble_r(a)
        out.append(((r * math.cos(a) - c[0]) * k + GRID / 2, (r * math.sin(a) - c[1]) * k + GRID / 2))
    return out


def glint(n):
    return [(GLINT_AT[0] + glint_r(i / n * math.tau) * GLINT_REACH * math.cos(i / n * math.tau),
             GLINT_AT[1] + glint_r(i / n * math.tau) * GLINT_REACH * math.sin(i / n * math.tau)) for i in range(n)]


def tile(n, half=0.82, power=5):
    """A superellipse in a box of side 2 centred on 0."""
    out = []
    for i in range(n):
        t = i / n * math.tau
        c, s = math.cos(t), math.sin(t)
        out.append((half * math.copysign(abs(c) ** (2 / power), c), half * math.copysign(abs(s) ** (2 / power), s)))
    return out


def path(pts):
    return "M" + "L".join(f"{x:.3f},{y:.3f}" for x, y in pts) + "Z"


def svg():
    """The icon on a 1024 box: the tile, then the mark at 64% of the tile's box, in the middle."""
    s = 1024
    k, at = s * 0.64 / GRID, s * 0.18
    t = [((x + 1) * s / 2, (y + 1) * s / 2) for x, y in tile(256)]
    place = lambda pts: [(at + x * k, at + y * k) for x, y in pts]
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {s} {s}">'
            f'<path fill="{TILE}" d="{path(t)}"/>'
            f'<path fill="{PAPER}" d="{path(place(pebble(512)))}"/>'
            f'<path fill="{TILE}" d="{path(place(glint(512)))}"/></svg>\n')


def render(src, px):
    return Image.open(io.BytesIO(bytes(resvg_py.svg_to_bytes(svg_string=src, width=px, height=px)))).convert("RGBA")


def bmp_frame(im):
    """A 32-bit icon bitmap: the header with the height doubled, BGRA rows bottom up, an empty mask."""
    w, h = im.size
    r, g, b, a = im.split()
    rows = Image.merge("RGBA", (b, g, r, a)).transpose(Image.Transpose.FLIP_TOP_BOTTOM).tobytes()
    mask = b"\0" * (((w + 31) // 32) * 4 * h)
    head = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, len(rows) + len(mask), 0, 0, 0, 0)
    return head + rows + mask


def ico(src, sizes):
    frames = []
    for px in sizes:
        im = render(src, px)
        if px >= 256:
            b = io.BytesIO()
            im.save(b, "PNG")
            frames.append((px, b.getvalue()))
        else:
            frames.append((px, bmp_frame(im)))
    out = struct.pack("<HHH", 0, 1, len(frames))
    at = 6 + 16 * len(frames)
    for px, data in frames:
        out += struct.pack("<BBBBHHII", px % 256, px % 256, 0, 0, 1, 32, len(data), at)
        at += len(data)
    return out + b"".join(d for _, d in frames)


def wordmark():
    """The wordmark's outlines in cap heights (Fraunces' cap is 1400 units), baseline at 0, y down.
    Each curve becomes 6 straight pieces. A contour inside another is a counter."""
    from fontTools.pens.recordingPen import RecordingPen
    from fontTools.svgLib.path import parse_path

    with open(os.path.join(ROOT, "brand/svg/wordmark-ink.svg"), encoding="utf-8") as f:
        d = re.search(r' d="([^"]+)"', f.read()).group(1)
    pen = RecordingPen()
    parse_path(d, pen)
    cap, contours, at = 1400.0, [], (0.0, 0.0)
    for op, args in pen.value:
        if op == "moveTo":
            contours.append([args[0]])
        elif op == "lineTo":
            contours[-1].append(args[0])
        elif op == "qCurveTo":
            # Implied on-curve points sit halfway between two control points.
            ctrl, end = list(args[:-1]), args[-1]
            ons = [((ctrl[i][0] + ctrl[i + 1][0]) / 2, (ctrl[i][1] + ctrl[i + 1][1]) / 2) for i in range(len(ctrl) - 1)] + [end]
            for c, b in zip(ctrl, ons):
                for k in range(1, 7):
                    t = k / 6
                    contours[-1].append(((1 - t) ** 2 * at[0] + 2 * (1 - t) * t * c[0] + t * t * b[0], (1 - t) ** 2 * at[1] + 2 * (1 - t) * t * c[1] + t * t * b[1]))
                at = b
            continue
        elif op == "closePath":
            continue
        at = args[-1]
    # A contour that closes on its start point drops the repeat.
    contours = [[(x / cap, y / cap) for x, y in (c[:-1] if c[0] == c[-1] else c)] for c in contours]

    def inside(p, poly):
        hit = False
        for (x0, y0), (x1, y1) in zip(poly, poly[1:] + poly[:1]):
            if (y0 > p[1]) != (y1 > p[1]) and p[0] < x0 + (p[1] - y0) * (x1 - x0) / (y1 - y0):
                hit = not hit
        return hit

    holes = [any(j != i and inside(c[0], o) for j, o in enumerate(contours)) for i, c in enumerate(contours)]
    xs = [x for c in contours for x, _ in c]
    ys = [y for c in contours for _, y in c]
    rows = lambda keep: "\n".join("    &[" + ", ".join(f"[{x:.4f}, {y:.4f}]" for x, y in c) + "]," for c, h in zip(contours, holes) if h == keep)
    return (
        "//! The word \"glint\" in Fraunces 800, as `brand/svg/wordmark-ink.svg` has it: flat outlines in cap\n"
        "//! heights, the baseline at 0 and y down. Written by `scripts/icons.py`; change it there.\n"
        "// Some coordinates come out close to well-known constants; they are only coordinates.\n#![allow(clippy::approx_constant)]\n\n"
        "/// The outlines to fill.\n#[rustfmt::skip]\npub const BODY: &[&[[f32; 2]]] = &[\n" + rows(False) + "\n];\n\n"
        "/// The counters, painted over the letters in the background's colour.\n#[rustfmt::skip]\npub const HOLES: &[&[[f32; 2]]] = &[\n" + rows(True) + "\n];\n\n"
        f"/// The word's box: left, top, right, bottom.\npub const BOX: [f32; 4] = [{min(xs):.4f}, {min(ys):.4f}, {max(xs):.4f}, {max(ys):.4f}];\n"
    )


def main():
    with open(os.path.join(ROOT, "crates/glint/src/wordmark.rs"), "w", encoding="utf-8", newline="\n") as f:
        f.write(wordmark())
    src = svg()
    with open(os.path.join(ROOT, "brand/svg/app-icon.svg"), "w", encoding="utf-8") as f:
        f.write(src)
    with open(os.path.join(ROOT, "crates/glint/assets/glint.ico"), "wb") as f:
        f.write(ico(src, [16, 20, 24, 32, 40, 48, 64, 128, 256]))
    render(src, 160).save(os.path.join(ROOT, "docs/glint.png"))
    for px in (32, 48, 64, 128, 256):
        render(src, px).save(os.path.join(ROOT, f"packaging/linux/icons/{px}.png"))
    for px in (16, 32, 128, 256, 512):
        render(src, px).save(os.path.join(ROOT, f"packaging/macos/glint.iconset/icon_{px}x{px}.png"))
        render(src, px * 2).save(os.path.join(ROOT, f"packaging/macos/glint.iconset/icon_{px}x{px}@2x.png"))
    print("wrote the app icon, the README picture, glint.ico, the Linux icons, the macOS icon set and wordmark.rs")


if __name__ == "__main__":
    main()
