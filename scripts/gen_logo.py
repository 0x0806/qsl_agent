#!/usr/bin/env python3
"""Rasterise the QSL "Split state" mark (see qsl/BRAND.md) to half-block cells.

Geometry is the brand kit's: two half-annuli (radius 20, stroke 8) on a 64 grid, the left half
(ink) moved up 4, the right half (accent) down 4, the pair rotated -25 degrees about (32,32).
Prints a Rust const of pixel rows ('i' ink, 'a' accent, ' ' empty); src/brand.rs pairs rows into half blocks.
Usage: python3 scripts/gen_logo.py [width_px]
"""
import math, sys

W = int(sys.argv[1]) if len(sys.argv) > 1 else 22
BOX = (6.0, 6.0, 52.0)
SS = 4  # subsamples per pixel axis

def cls(x, y):
    # inverse rotation of the -25 degree tilt
    a = math.radians(25)
    dx, dy = x - 32, y - 32
    qx = 32 + dx * math.cos(a) - dy * math.sin(a)
    qy = 32 + dx * math.sin(a) + dy * math.cos(a)
    d_l = math.hypot(qx - 32, qy - 28)
    if qx <= 32 and 16 <= d_l <= 24 and 8 <= qy <= 48:
        return "i"
    d_r = math.hypot(qx - 32, qy - 36)
    if qx >= 32 and 16 <= d_r <= 24 and 16 <= qy <= 56:
        return "a"
    return " "

def pixel(px, py):
    s = BOX[2] / W
    n = {"i": 0, "a": 0, " ": 0}
    for sy in range(SS):
        for sx in range(SS):
            n[cls(BOX[0] + (px + (sx + .5) / SS) * s, BOX[1] + (py + (sy + .5) / SS) * s)] += 1
    best = max("ia", key=lambda k: n[k])
    return best if n[best] >= SS * SS * 0.45 else " "

print("/// Pixel rows of the mark: 'i' ink, 'a' accent, ' ' empty. Two pixel rows make one terminal row (half blocks).")
print("pub const MARK_PX: &[&str] = &[")
for py in range(W):
    print('    "' + "".join(pixel(px, py) for px in range(W)) + '",')
print("];")
