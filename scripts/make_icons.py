"""Generate Agora's icons with the standard library only (no Pillow).

Two overlapping discs — Claude (clay) and GPT (teal) — on an ink tile; the lens where they
overlap is the warm "agora", the common ground. Writes:

  web/icon.svg, web/icon-192.png, web/icon-512.png
  src-tauri/icons/{32x32,128x128,128x128@2x,icon}.png, src-tauri/icons/icon.ico

Run:  python -I scripts/make_icons.py   (from the repo root)
"""

import math
import struct
import sys
import zlib
from pathlib import Path

INK = (17, 17, 17)
CLAY = (181, 100, 60)
TEAL = (62, 110, 96)
WARM = (219, 212, 205)

# Geometry in a 64-unit box.
RX = 13.0
C1 = (25.0, 32.0)
C2 = (39.0, 32.0)
R = 14.0


def inside_round_rect(x: float, y: float) -> bool:
    if x < 0 or y < 0 or x > 64 or y > 64:
        return False
    cx = min(max(x, RX), 64 - RX)
    cy = min(max(y, RX), 64 - RX)
    return (x - cx) ** 2 + (y - cy) ** 2 <= RX * RX


def color_at(x: float, y: float):
    if not inside_round_rect(x, y):
        return None
    a = (x - C1[0]) ** 2 + (y - C1[1]) ** 2 <= R * R
    b = (x - C2[0]) ** 2 + (y - C2[1]) ** 2 <= R * R
    if a and b:
        return WARM
    if a:
        return CLAY
    if b:
        return TEAL
    return INK


def render(size: int, ss: int = 4) -> bytes:
    rows = []
    step = 64.0 / size
    sub = step / ss
    for py in range(size):
        row = bytearray([0])  # filter: none
        for px in range(size):
            r = g = b = a = 0
            for sy in range(ss):
                for sx in range(ss):
                    c = color_at(px * step + (sx + 0.5) * sub, py * step + (sy + 0.5) * sub)
                    if c is not None:
                        r += c[0]
                        g += c[1]
                        b += c[2]
                        a += 1
            n = ss * ss
            if a:
                row += bytes((r // a, g // a, b // a, round(255 * a / n)))
            else:
                row += b"\x00\x00\x00\x00"
        rows.append(bytes(row))
    raw = b"".join(rows)

    def chunk(tag: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)  # 8-bit RGBA
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def ico(pngs: dict) -> bytes:
    sizes = sorted(pngs)
    header = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries = b""
    data = b""
    for s in sizes:
        blob = pngs[s]
        w = 0 if s >= 256 else s
        entries += struct.pack("<BBBBHHII", w, w, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    return header + entries + data


SVG = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <rect width="64" height="64" rx="{RX:g}" fill="#111111"/>
  <circle cx="{C1[0]:g}" cy="{C1[1]:g}" r="{R:g}" fill="#b5643c"/>
  <circle cx="{C2[0]:g}" cy="{C2[1]:g}" r="{R:g}" fill="#3e6e60"/>
  <path d="M32 {32 - math.sqrt(R * R - 49):.3f} A{R:g} {R:g} 0 0 1 32 {32 + math.sqrt(R * R - 49):.3f} A{R:g} {R:g} 0 0 1 32 {32 - math.sqrt(R * R - 49):.3f}Z" fill="#dbd4cd"/>
</svg>
"""


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    web = root / "web"
    icons = root / "src-tauri" / "icons"
    icons.mkdir(parents=True, exist_ok=True)
    (web / "icon.svg").write_text(SVG, encoding="utf-8")
    cache = {}

    def png(size: int) -> bytes:
        if size not in cache:
            cache[size] = render(size, 4 if size <= 256 else 2)
        return cache[size]

    (web / "icon-192.png").write_bytes(png(192))
    (web / "icon-512.png").write_bytes(png(512))
    (icons / "32x32.png").write_bytes(png(32))
    (icons / "128x128.png").write_bytes(png(128))
    (icons / "128x128@2x.png").write_bytes(png(256))
    (icons / "icon.png").write_bytes(png(512))
    (icons / "icon.ico").write_bytes(ico({s: png(s) for s in (16, 24, 32, 48, 64, 128, 256)}))
    print("icons written to", web, "and", icons)
    return 0


if __name__ == "__main__":
    sys.exit(main())
