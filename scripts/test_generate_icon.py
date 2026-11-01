"""Headless regressions for portable, strict icon-asset verification."""
import importlib.util
from pathlib import Path
import struct
import unittest
import zlib

SPEC = importlib.util.spec_from_file_location(
    "generate_icon", Path(__file__).with_name("generate-icon.py"))
ICON = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ICON)


def chunk(kind, payload):
    return (struct.pack(">I", len(payload)) + kind + payload
            + struct.pack(">I", zlib.crc32(kind + payload)))


def encoded_png(size, compressed, split=False):
    data = [compressed]
    if split:
        middle = len(compressed) // 2
        data = [compressed[:middle], compressed[middle:]]
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + b"".join(chunk(b"IDAT", part) for part in data)
            + chunk(b"IEND", b""))


def unfiltered_rows(size, rgba):
    return b"".join(b"\0" + rgba[y*size*4:(y+1)*size*4] for y in range(size))


def replace_256_variant(content, payload):
    entry = 6 + 16 * (len(ICON.SIZES) - 1)
    offset = struct.unpack_from("<I", content, entry + 12)[0]
    result = bytearray(content[:offset])
    struct.pack_into("<I", result, entry + 8, len(payload))
    return bytes(result) + payload


class IconVerificationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.reference = ICON.build_assets()
        cls.rgba = ICON.png_rgba(cls.reference["app-icon.png"], 256, "test reference")
        cls.rows = unfiltered_rows(256, cls.rgba)

    def test_lossless_compression_and_chunk_boundaries_do_not_make_icons_stale(self):
        # Reproduce zlib/zlib-ng encoding differences without depending on either library version.
        alternatives = [zlib.compress(self.rows, 0)]
        compressor = zlib.compressobj(9, zlib.DEFLATED, zlib.MAX_WBITS,
                                     zlib.DEF_MEM_LEVEL, zlib.Z_HUFFMAN_ONLY)
        alternatives.append(compressor.compress(self.rows) + compressor.flush())
        for compressed in alternatives:
            with self.subTest(length=len(compressed)):
                png = encoded_png(256, compressed, split=True)
                self.assertNotEqual(png, self.reference["app-icon.png"])
                ICON.verify_asset("app-icon.png", png, self.reference["app-icon.png"])
                ico = replace_256_variant(self.reference["app-icon.ico"], png)
                self.assertNotEqual(ico, self.reference["app-icon.ico"])
                ICON.verify_asset("app-icon.ico", ico, self.reference["app-icon.ico"])

    def test_changed_pixels_fail_for_standalone_png_and_every_ico_size(self):
        rgba = bytearray(self.rgba)
        rgba[(128 * 256 + 128) * 4] ^= 1
        png = encoded_png(256, zlib.compress(unfiltered_rows(256, rgba)))
        with self.assertRaisesRegex(ValueError, "pixels differ"):
            ICON.verify_asset("app-icon.png", png, self.reference["app-icon.png"])
        ico = replace_256_variant(self.reference["app-icon.ico"], png)
        with self.assertRaisesRegex(ValueError, "pixels differ"):
            ICON.verify_asset("app-icon.ico", ico, self.reference["app-icon.ico"])
        for index, size in enumerate(ICON.SIZES[:-1]):
            with self.subTest(size=size):
                ico = bytearray(self.reference["app-icon.ico"])
                offset = struct.unpack_from("<I", ico, 6 + 16 * index + 12)[0]
                pixel = 40 + ((size - 1 - size // 2) * size + size // 2) * 4
                ico[offset + pixel] ^= 1
                with self.assertRaisesRegex(ValueError, "DIB pixels/mask/metadata differ"):
                    ICON.verify_asset("app-icon.ico", ico, self.reference["app-icon.ico"])

    def test_corrupt_crc_stream_filter_and_truncation_fail(self):
        png = bytearray(self.reference["app-icon.png"])
        png[41] ^= 1  # Alter the first IDAT payload byte without updating its CRC.
        invalid_filter = b"\x05" + self.rows[1:]
        corruptions = [bytes(png), self.reference["app-icon.png"][:-1],
                       encoded_png(256, b"not a DEFLATE stream"),
                       encoded_png(256, zlib.compress(invalid_filter)),
                       encoded_png(256, zlib.compress(self.rows + b"extra"))]
        for content in corruptions:
            with self.subTest(length=len(content)), self.assertRaises(ValueError):
                ICON.verify_asset("app-icon.png", content, self.reference["app-icon.png"])

    def test_variant_count_order_depth_and_offsets_are_checked(self):
        reference = self.reference["app-icon.ico"]
        missing = bytearray(reference)
        struct.pack_into("<H", missing, 4, len(ICON.SIZES) - 1)
        reordered = bytearray(reference)
        reordered[6:22], reordered[22:38] = reference[22:38], reference[6:22]
        wrong_depth = bytearray(reference)
        struct.pack_into("<H", wrong_depth, 6 + 6, 24)
        bad_offset = bytearray(reference)
        struct.pack_into("<I", bad_offset, 6 + 12, 0)
        for content in [missing, reordered, wrong_depth, bad_offset, reference + b"extra"]:
            with self.subTest(length=len(content)), self.assertRaises(ValueError):
                ICON.verify_asset("app-icon.ico", content, reference)

    def test_dib_transparency_mask_and_header_corruption_fail(self):
        reference = self.reference["app-icon.ico"]
        offset = struct.unpack_from("<I", reference, 6 + 12)[0]
        mask = bytearray(reference)
        mask[offset + 40 + 16 * 16 * 4] ^= 128
        header = bytearray(reference)
        struct.pack_into("<i", header, offset + 4, 15)
        for content in [mask, header]:
            with self.assertRaisesRegex(ValueError, "DIB pixels/mask/metadata differ"):
                ICON.verify_asset("app-icon.ico", content, reference)

    def test_all_standard_png_filters_decode_known_rgba(self):
        expected = bytes([10, 20, 30, 255, 40, 50, 60, 128,
                          15, 30, 45, 200, 80, 90, 100, 64])
        filtered = [
            ([10, 20, 30, 255, 40, 50, 60, 128], [15, 30, 45, 200, 80, 90, 100, 64]),
            ([10, 20, 30, 255, 30, 30, 30, 129], [15, 30, 45, 200, 65, 60, 55, 120]),
            ([10, 20, 30, 255, 40, 50, 60, 128], [5, 10, 15, 201, 40, 40, 40, 192]),
            ([10, 20, 30, 255, 35, 40, 45, 1], [10, 20, 30, 73, 53, 50, 48, 156]),
            ([10, 20, 30, 255, 30, 30, 30, 129], [5, 10, 15, 201, 40, 40, 40, 192]),
        ]
        for method, (first, second) in enumerate(filtered):
            with self.subTest(filter=method):
                rows = bytes([method, *first, method, *second])
                png = encoded_png(2, zlib.compress(rows))
                self.assertEqual(ICON.png_rgba(png, 2, "known pixels"), expected)


if __name__ == "__main__":
    unittest.main()
