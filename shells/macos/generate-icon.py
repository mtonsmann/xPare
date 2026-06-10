#!/usr/bin/env python3
"""Generate the xPare .iconset with no external image dependencies.

Pure stdlib (struct/zlib/math) so it runs anywhere Python 3 is present — no Pillow.
Writes the ten PNG sizes Apple's `iconutil` expects into the given iconset dir;
`package-app.sh` then runs `iconutil -c icns` to produce `AppIcon.icns`.
"""

from __future__ import annotations

import argparse
import math
import struct
import zlib
from pathlib import Path


ICON_FILES = {
    "icon_16x16.png": 16,
    "icon_16x16@2x.png": 32,
    "icon_32x32.png": 32,
    "icon_32x32@2x.png": 64,
    "icon_128x128.png": 128,
    "icon_128x128@2x.png": 256,
    "icon_256x256.png": 256,
    "icon_256x256@2x.png": 512,
    "icon_512x512.png": 512,
    "icon_512x512@2x.png": 1024,
}


def chunk(kind: bytes, data: bytes) -> bytes:
    checksum = zlib.crc32(kind)
    checksum = zlib.crc32(data, checksum)
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", checksum & 0xFFFFFFFF)


def write_png(path: Path, size: int) -> None:
    rows = bytearray()
    for y in range(size):
        rows.append(0)
        for x in range(size):
            rows.extend(pixel(size, x + 0.5, y + 0.5))

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    data = b"\x89PNG\r\n\x1a\n"
    data += chunk(b"IHDR", header)
    data += chunk(b"IDAT", zlib.compress(bytes(rows), level=9))
    data += chunk(b"IEND", b"")
    path.write_bytes(data)


def pixel(size: int, x: float, y: float) -> bytes:
    scale = size / 1024.0
    center = size / 2.0
    coverage = squircle_coverage(size, x, y)
    if coverage <= 0.0:
        return bytes((0, 0, 0, 0))

    t = y / max(size - 1, 1)
    bg = mix((23, 41, 50), (10, 92, 95), t)

    # A diagonal "strip" mark plus an accent line — evokes stripping rich text to plain.
    stripe = abs((x - y) - 40 * scale)
    stripe_alpha = smooth(210 * scale, 0, stripe)
    accent = abs((x + y) - size * 0.98)
    accent_alpha = smooth(34 * scale, 0, accent)

    r, g, b = bg
    r, g, b = over((240, 248, 246), (r, g, b), stripe_alpha * 0.96)
    r, g, b = over((36, 185, 170), (r, g, b), accent_alpha * 0.9)

    distance = math.hypot(x - center, y - center)
    shade = 0.92 + 0.08 * max(0.0, 1.0 - distance / center)
    return bytes((clamp(r * shade), clamp(g * shade), clamp(b * shade), clamp(255 * coverage)))


def squircle_coverage(size: int, x: float, y: float) -> float:
    """Coverage (0..1) of the macOS Big Sur+ icon shape at a sample point.

    Apple's icon grid: the shape leaves a ~10% transparent margin on every side
    (the system renders no mask, so a full-bleed icon looks oversized next to
    every other app), and the corners are rounded at ~22.37% of the shape's
    width — the 824 px shape / ~185 px corner figure from Apple's 1024 px
    template. A true superellipse corner differs imperceptibly at these sizes
    and would cost the stdlib-only simplicity, so this is a rounded rect with a
    ~1 px anti-aliased edge to keep the small sizes clean.
    """
    center = size / 2.0
    margin = size * 0.10
    half = center - margin
    radius = 2.0 * half * 0.2237
    dx = max(abs(x - center) - (half - radius), 0.0)
    dy = max(abs(y - center) - (half - radius), 0.0)
    outside = math.hypot(dx, dy) - radius
    return smooth(-0.5, 0.5, outside)


def smooth(edge0: float, edge1: float, value: float) -> float:
    if edge0 == edge1:
        return 1.0 if value <= edge0 else 0.0
    t = (value - edge1) / (edge0 - edge1)
    t = max(0.0, min(1.0, t))
    return t * t * (3.0 - 2.0 * t)


def mix(a: tuple[int, int, int], b: tuple[int, int, int], t: float) -> tuple[int, int, int]:
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def over(top: tuple[int, int, int], bottom: tuple[int, int, int], alpha: float) -> tuple[int, int, int]:
    alpha = max(0.0, min(1.0, alpha))
    return tuple(round(top[i] * alpha + bottom[i] * (1.0 - alpha)) for i in range(3))


def clamp(value: float) -> int:
    return max(0, min(255, round(value)))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("iconset", type=Path)
    args = parser.parse_args()

    args.iconset.mkdir(parents=True, exist_ok=True)
    for name, size in ICON_FILES.items():
        write_png(args.iconset / name, size)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
