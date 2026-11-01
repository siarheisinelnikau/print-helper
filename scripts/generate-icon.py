"""Rasterize our rect/circle SVG with the standard library; no GUI or SVG runtime.

Small ICO entries use 32-bit DIBs with transparency masks; the 256px entry is PNG.
Run with --check to detect stale/corrupt pixels and metadata without writing them.
Lossless PNG compression bytes can differ between zlib implementations.
"""
from pathlib import Path
import struct
import sys
import xml.etree.ElementTree as ET
import zlib

ROOT = Path(__file__).resolve().parent.parent
SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def rasterize(size):
    shapes = []
    for node in ET.parse(ROOT / "assets/app-icon.svg").getroot():
        tag = node.tag.rsplit("}", 1)[-1]
        require(tag in ("rect", "circle"), f"Unsupported SVG element: {tag}")
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
    return (PNG_SIGNATURE
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


def build_assets():
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
    return {"app-icon.ico": struct.pack("<HHH", 0, 1, len(SIZES))
            + b"".join(entries + payloads), "app-icon.png": payloads[-1]}


def png_rgba(content, size, label):
    """Decode our bounded 8-bit RGBA PNG format, validating CRCs and all filters."""
    require(content.startswith(PNG_SIGNATURE), f"{label}: invalid PNG signature")
    position = len(PNG_SIGNATURE)
    compressed = bytearray()
    header = False
    ended = False
    image_chunks = 0
    while position < len(content):
        require(position + 12 <= len(content), f"{label}: truncated PNG chunk")
        length = struct.unpack_from(">I", content, position)[0]
        end = position + 12 + length
        require(end <= len(content), f"{label}: PNG chunk exceeds file")
        kind = content[position + 4:position + 8]
        payload = content[position + 8:end - 4]
        crc = struct.unpack_from(">I", content, end - 4)[0]
        require(zlib.crc32(kind + payload) == crc, f"{label}: PNG CRC mismatch")
        if kind == b"IHDR":
            require(not header and position == 8, f"{label}: misplaced PNG header")
            require(length == 13, f"{label}: invalid PNG header length")
            require(struct.unpack(">IIBBBBB", payload) == (size, size, 8, 6, 0, 0, 0),
                    f"{label}: unexpected PNG dimensions or RGBA format")
            header = True
        elif kind == b"IDAT":
            require(header and not ended, f"{label}: misplaced PNG image data")
            compressed.extend(payload)
            image_chunks += 1
        elif kind == b"IEND":
            require(header and image_chunks and length == 0 and end == len(content),
                    f"{label}: invalid PNG end or trailing data")
            ended = True
        else:
            # Profiles/gamma can change appearance despite equal channel bytes.
            require(False, f"{label}: unexpected PNG chunk {kind!r}")
        position = end
    require(ended, f"{label}: missing PNG end")
    stride = size * 4
    expected_length = (stride + 1) * size
    decoder = zlib.decompressobj()
    try:
        rows = decoder.decompress(compressed, expected_length + 1)
    except zlib.error as error:
        raise ValueError(f"{label}: invalid compressed PNG data") from error
    require(decoder.eof and not decoder.unused_data and not decoder.unconsumed_tail
            and len(rows) == expected_length, f"{label}: invalid PNG image length")
    rgba = bytearray()
    previous = bytearray(stride)
    for y in range(size):
        start = y * (stride + 1)
        method = rows[start]
        require(method <= 4, f"{label}: invalid PNG filter")
        row = bytearray(stride)
        for x, value in enumerate(rows[start + 1:start + 1 + stride]):
            left = row[x - 4] if x >= 4 else 0
            above = previous[x]
            upper_left = previous[x - 4] if x >= 4 else 0
            if method == 0:
                prediction = 0
            elif method == 1:
                prediction = left
            elif method == 2:
                prediction = above
            elif method == 3:
                prediction = (left + above) // 2
            else:
                estimate = left + above - upper_left
                distances = [abs(estimate - n) for n in (left, above, upper_left)]
                prediction = (left, above, upper_left)[distances.index(min(distances))]
            row[x] = (value + prediction) & 255
        rgba.extend(row)
        previous = row
    return bytes(rgba)


def icon_entries(content, label):
    require(len(content) >= 6, f"{label}: truncated ICO header")
    require(struct.unpack_from("<HHH", content) == (0, 1, len(SIZES)),
            f"{label}: invalid ICO type or variant count")
    cursor = 6 + 16 * len(SIZES)
    require(len(content) >= cursor, f"{label}: truncated ICO directory")
    entries = []
    for index, size in enumerate(SIZES):
        width, height, colors, reserved, planes, depth, length, offset = struct.unpack_from(
            "<BBBBHHII", content, 6 + 16 * index)
        require((width or 256, height or 256, colors, reserved, planes, depth)
                == (size, size, 0, 0, 1, 32),
                f"{label}: invalid metadata for {size}px ICO variant")
        require(offset == cursor and length > 0 and offset + length <= len(content),
                f"{label}: invalid ICO variant offset/length")
        entries.append((size, content[offset:offset + length]))
        cursor = offset + length
    require(cursor == len(content), f"{label}: trailing ICO data")
    return entries


def verify_asset(name, actual, expected):
    if name == "app-icon.png":
        require(png_rgba(actual, 256, name) == png_rgba(expected, 256, "SVG reference"),
                f"{name}: decoded RGBA pixels differ from the SVG; regenerate assets")
    else:
        require(name == "app-icon.ico", f"Unknown icon asset: {name}")
        actual_entries = icon_entries(actual, name)
        expected_entries = icon_entries(expected, "SVG reference")
        for (size, payload), (_, reference) in zip(actual_entries, expected_entries):
            label = f"{name}: {size}px variant"
            if size == 256:
                require(png_rgba(payload, size, label) == png_rgba(reference, size, "SVG reference"),
                        f"{label}: decoded RGBA pixels differ from the SVG; regenerate assets")
            else:
                # DIB pixels, alpha masks and headers are uncompressed and deterministic.
                require(payload == reference,
                        f"{label}: DIB pixels/mask/metadata differ from the SVG; regenerate assets")


def main():
    print(f"Icon rasterizer: Python {sys.version.split()[0]}, "
          f"zlib {zlib.ZLIB_VERSION} (runtime {zlib.ZLIB_RUNTIME_VERSION}); standard library")
    outputs = build_assets()
    for name, content in outputs.items():
        path = ROOT / "assets" / name
        if "--check" in sys.argv:
            verify_asset(name, path.read_bytes(), content)
        else:
            path.write_bytes(content)
    print("Verified icon assets." if "--check" in sys.argv else "Generated icon assets.")


if __name__ == "__main__":
    main()
