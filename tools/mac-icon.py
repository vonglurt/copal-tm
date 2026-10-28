#!/usr/bin/env python3
"""mac-icon.py OUT.png -- copal-tm's icon, 1024 pixels square, RGBA.

The panel in miniature: a dark tile, the CPU history's green trace over its
grid, and four ladder meters below in the program's own hues (green, cyan,
orange, yellow -- src/tty/color.rs's tests name them).  Drawn by distance,
so every edge is anti-aliased without supersampling, and written as a PNG by
hand: zlib is the only library.  tools/mac-app.sh makes the .icns from it.
"""
import math
import struct
import sys
import zlib

N = 1024
GROUND = (0x18, 0x18, 0x18)
PANEL = (0x22, 0x24, 0x22)
GRID = (0x3A, 0x3E, 0x3A)
GREEN = (0x7C, 0xF0, 0x5A)
CYAN = (0x4F, 0xE3, 0xE8)
ORANGE = (0xF0, 0xA0, 0x3A)
YELLOW = (0xF2, 0xE3, 0x4A)
EDGE = (0x55, 0x5A, 0x55)


def cover(d):
    return min(1.0, max(0.0, 0.5 - d))


def box(x, y, x0, y0, x1, y1, r=0.0):
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    hx, hy = (x1 - x0) / 2 - r, (y1 - y0) / 2 - r
    qx, qy = abs(x - cx) - hx, abs(y - cy) - hy
    return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - r


def seg(x, y, ax, ay, bx, by):
    px, py, dx, dy = x - ax, y - ay, bx - ax, by - ay
    t = max(0.0, min(1.0, (px * dx + py * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - dx * t, py - dy * t)


# The trace: a busy machine's last minute, as the history draws it.
CHART = (190, 230, 834, 560)
LOAD = [0.30, 0.42, 0.38, 0.65, 0.58, 0.80, 0.72, 0.55, 0.62, 0.90, 0.84, 0.70, 0.78]
PTS = [
    (CHART[0] + (CHART[2] - CHART[0]) * i / (len(LOAD) - 1), CHART[3] - (CHART[3] - CHART[1]) * v)
    for i, v in enumerate(LOAD)
]
METERS = [(GREEN, 0.72), (CYAN, 0.45), (ORANGE, 0.88), (YELLOW, 0.30)]


def render():
    rows = []
    for j in range(N):
        y = j + 0.5
        row = bytearray()
        for i in range(N):
            x = i + 0.5
            d = box(x, y, 100, 100, 924, 924, 185)
            alpha = cover(d)
            if alpha == 0:
                row += b"\0\0\0\0"
                continue
            col = list(GROUND)

            def paint(rgb, a):
                for k in range(3):
                    col[k] = col[k] + (rgb[k] - col[k]) * a

            paint(EDGE, cover(abs(d + 5) - 5))
            # The history: its panel, grid, the area under the trace, the trace.
            paint(PANEL, cover(box(x, y, 160, 200, 864, 590, 28)))
            if CHART[0] <= x <= CHART[2] and CHART[1] <= y <= CHART[3]:
                for g in range(1, 4):
                    gy = CHART[1] + (CHART[3] - CHART[1]) * g / 4
                    paint(GRID, cover(abs(y - gy) - 1.5))
                k = min(int((x - CHART[0]) / (CHART[2] - CHART[0]) * (len(PTS) - 1)), len(PTS) - 2)
                (ax, ay), (bx, by) = PTS[k], PTS[k + 1]
                top = ay + (by - ay) * (x - ax) / (bx - ax)
                if y > top:
                    paint(GREEN, 0.22)
            dl = min(seg(x, y, *PTS[k], *PTS[k + 1]) for k in range(len(PTS) - 1))
            paint(GREEN, cover(dl - 9))
            # Four ladder meters, rungs and all.
            for m, (hue, v) in enumerate(METERS):
                x0 = 190 + m * 170
                paint(PANEL, cover(box(x, y, x0 - 14, 636, x0 + 134, 852, 18)))
                for r in range(6):
                    y1 = 834 - r * 33
                    lit = (r + 0.5) / 6 <= v
                    paint(hue if lit else GRID, cover(box(x, y, x0, y1 - 24, x0 + 120, y1, 5)))
            row += bytes(int(round(v)) for v in col) + bytes([int(round(alpha * 255))])
        rows.append(bytes(row))
    return rows


def png(rows, path):
    raw = b"".join(b"\0" + r for r in rows)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 9)))
        f.write(chunk(b"IEND", b""))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: mac-icon.py OUT.png")
    png(render(), sys.argv[1])
