#!/usr/bin/env python3
"""Pack PNG files into a Windows .ico (PNG-compressed entries). Standard library only.

    tools/make-ico.py OUT.ico 16.png 32.png 64.png 128.png 256.png
"""
import struct
import sys
from pathlib import Path

PNG_MAGIC = b"\x89PNG\r\n\x1a\n"


def png_size(data):
    if not data.startswith(PNG_MAGIC) or data[12:16] != b"IHDR":
        raise ValueError("not a PNG")
    return struct.unpack(">II", data[16:24])


def build_ico(images):
    """`images` is a list of PNG byte strings; returns the .ico bytes."""
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, blobs = b"", b""
    for data in images:
        width, height = png_size(data)
        if width > 256 or height > 256:
            raise ValueError("ico entries are at most 256 x 256")
        entries += struct.pack("<BBBBHHII", width % 256, height % 256, 0, 0, 1, 32, len(data), offset)
        blobs += data
        offset += len(data)
    return header + entries + blobs


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    out, sources = sys.argv[1], sys.argv[2:]
    Path(out).write_bytes(build_ico([Path(p).read_bytes() for p in sources]))


if __name__ == "__main__":
    main()
