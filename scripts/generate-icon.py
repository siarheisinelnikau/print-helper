"""Rasterize our rect/circle SVG with the standard library; no GUI or SVG runtime.

Small ICO entries use 32-bit DIBs with transparency masks; the 256px entry is PNG.
Run with --check to detect stale assets without writing them.
"""
from pathlib import Path
import struct
import sys
import xml.etree.ElementTree as ET
import zlib

ROOT = Path(__file__).resolve().parent.parent
SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)


def rasterize(size):
    shapes = []
    for node in ET.parse(ROOT / "assets/app-icon.svg").getroot():
        tag = node.tag.rsplit("}", 1)[-1]
        assert tag in ("rect", "circle"), f"Unsupported SVG element: {tag}"
        color = bytes.fromhex(node.attrib["fill"].removeprefix("#"))
        shapes.append((tag, {k: float(v) for k, v in node.attrib.items()
                             if k != "fill"}, color))
    rgba = bytearray()
    samples = 4
    for y in range(size):
        for x in range(size):
            sums = [0, 0, 0]
            covered = 0
            for sy in range(samples):
                py = (y + (sy + 0.5) / samples) * 32 / size
                for sx in range(samples):
                    px = (x + (sx + 0.5) / samples) * 32 / size
                    color = None
                    for tag, a, fill in shapes:
                        if tag == "circle":
                            inside = (px - a["cx"])**2 + (py - a["cy"])**2 <= a["r"]**2
                        else:
                            rx = a.get("rx", 0)
                            dx = max(a["x"] + rx - px, 0, px - (a["x"] + a["width"] - rx))
                            dy = max(a["y"] + rx - py, 0, py - (a["y"] + a["height"] - rx))
                            inside = (a["x"] <= px < a["x"] + a["width"]
                                      and a["y"] <= py < a["y"] + a["height"]
                                      and dx*dx + dy*dy <= rx*rx)
                        if inside:
                            color = fill
                    if color is not None:
                        covered += 1
                        sums = [s + c for s, c in zip(sums, color)]
            rgba.extend(round(s / covered) if covered else 0 for s in sums)
            rgba.append(round(255 * covered / samples**2))
    return bytes(rgba)


def png(size, rgba):
    def chunk(kind, payload):
        return (struct.pack(">I", len(payload)) + kind + payload
                + struct.pack(">I", zlib.crc32(kind + payload)))
    rows = b"".join(b"\0" + rgba[y*size*4:(y+1)*size*4] for y in range(size))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


def dib(size, rgba):
    pixels = bytearray()
    mask = bytearray()
    stride = (size + 31) // 32 * 4
    for y in reversed(range(size)):
        row_mask = bytearray(stride)
        for x in range(size):
            r, g, b, a = rgba[(y*size+x)*4:(y*size+x+1)*4]
            pixels.extend((b, g, r, a))
            if a == 0:
                row_mask[x // 8] |= 1 << (7 - x % 8)
        mask.extend(row_mask)
    return (struct.pack("<IiiHHIIiiII", 40, size, size*2, 1, 32, 0,
                        len(pixels) + len(mask), 0, 0, 0, 0) + pixels + mask)


def main():
    entries = []
    payloads = []
    offset = 6 + 16 * len(SIZES)
    for size in SIZES:
        rgba = rasterize(size)
        payload = png(size, rgba) if size == 256 else dib(size, rgba)
        entries.append(struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0,
                                   1, 32, len(payload), offset))
        payloads.append(payload)
        offset += len(payload)
    outputs = {"app-icon.ico": struct.pack("<HHH", 0, 1, len(SIZES))
               + b"".join(entries + payloads), "app-icon.png": payloads[-1]}
    for name, content in outputs.items():
        path = ROOT / "assets" / name
        if "--check" in sys.argv:
            assert path.read_bytes() == content, f"Regenerate {path}"
        else:
            path.write_bytes(content)
    print("Verified icon assets." if "--check" in sys.argv else "Generated icon assets.")


if __name__ == "__main__":
    main()
