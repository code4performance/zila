#!/usr/bin/env python3
"""Draws the icon of the language as a PNG from the same geometry as docs/assets/zila.svg.

    python tools/make-icon.py                          # editors/vscode/icon.png, 128 x 128
    python tools/make-icon.py out.png --size 256

Standard library only: the shapes are filled at 4x supersampling (even-odd rule for the Z, a rounded
square for the background) and written as an RGBA PNG with zlib. The SVG is the source of truth; if
its numbers change, change them here too (tests/docs.rs checks that the files exist and the size).
"""

import argparse
import struct
import sys
import zlib

SIDE = 128.0  # the geometry of the SVG is drawn in a 128 x 128 box
RADIUS = 28.0
ACCENT = (0x5B, 0x5B, 0xD6)
WHITE = (0xFF, 0xFF, 0xFF)
Z = [(36, 32), (92, 32), (92, 46), (58, 82), (92, 82), (92, 96), (36, 96), (36, 82), (70, 46), (36, 46)]


def inside_polygon(x, y, poly):
    inside = False
    n = len(poly)
    j = n - 1
    for i in range(n):
        xi, yi = poly[i]
        xj, yj = poly[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
            inside = not inside
        j = i
    return inside


def inside_rounded_square(x, y):
    if x < 0 or y < 0 or x > SIDE or y > SIDE:
        return False
    cx = min(max(x, RADIUS), SIDE - RADIUS)
    cy = min(max(y, RADIUS), SIDE - RADIUS)
    return (x - cx) ** 2 + (y - cy) ** 2 <= RADIUS ** 2


def render(size):
    ss = 4
    rows = []
    scale = SIDE / (size * ss)
    for py in range(size):
        row = bytearray([0])  # filter type 0
        for px in range(size):
            bg = fg = 0
            for sy in range(ss):
                y = (py * ss + sy + 0.5) * scale
                for sx in range(ss):
                    x = (px * ss + sx + 0.5) * scale
                    if inside_rounded_square(x, y):
                        bg += 1
                        if inside_polygon(x, y, Z):
                            fg += 1
            total = ss * ss
            if bg == 0:
                row += bytes([0, 0, 0, 0])
                continue
            mix = fg / bg
            r = round(ACCENT[0] * (1 - mix) + WHITE[0] * mix)
            g = round(ACCENT[1] * (1 - mix) + WHITE[1] * mix)
            b = round(ACCENT[2] * (1 - mix) + WHITE[2] * mix)
            row += bytes([r, g, b, round(255 * bg / total)])
        rows.append(bytes(row))
    return b"".join(rows)


def chunk(tag, data):
    body = tag + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def png(size):
    raw = render(size)
    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)  # 8 bit RGBA
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("out", nargs="?", default="editors/vscode/icon.png")
    ap.add_argument("--size", type=int, default=128)
    a = ap.parse_args()
    data = png(a.size)
    with open(a.out, "wb") as f:
        f.write(data)
    print("wrote %s (%d x %d, %d bytes)" % (a.out, a.size, a.size, len(data)))


if __name__ == "__main__":
    sys.exit(main())
