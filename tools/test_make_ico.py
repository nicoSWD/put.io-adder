#!/usr/bin/env python3
"""Tests for make-ico.py (run: python3 -I tools/test_make_ico.py)."""
import importlib.util
import struct
import unittest
import zlib
from pathlib import Path

spec = importlib.util.spec_from_file_location("make_ico", Path(__file__).parent / "make-ico.py")
make_ico = importlib.util.module_from_spec(spec)
spec.loader.exec_module(make_ico)


def png(width, height):
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    raw = b"".join(b"\x00" + b"\x00" * (width * 4) for _ in range(height))
    return make_ico.PNG_MAGIC + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


class IcoTest(unittest.TestCase):
    def test_directory_describes_every_image(self):
        images = [png(16, 16), png(32, 32), png(256, 256)]
        ico = make_ico.build_ico(images)
        reserved, kind, count = struct.unpack("<HHH", ico[:6])
        self.assertEqual((reserved, kind, count), (0, 1, 3))
        sizes = []
        for i in range(count):
            w, h, _, _, _, bpp, length, offset = struct.unpack("<BBBBHHII", ico[6 + 16 * i : 22 + 16 * i])
            sizes.append((w or 256, h or 256))
            self.assertEqual(ico[offset : offset + length], images[i])
            self.assertEqual(bpp, 32)
        self.assertEqual(sizes, [(16, 16), (32, 32), (256, 256)])

    def test_rejects_non_png_and_oversized(self):
        with self.assertRaises(ValueError):
            make_ico.build_ico([b"GIF89a....................."])
        with self.assertRaises(ValueError):
            make_ico.build_ico([png(512, 512)])


if __name__ == "__main__":
    unittest.main()
