#!/usr/bin/env python3
"""One-off asset step: turn the supplied logo (opaque, near-white background) into a transparent PNG.

    python3 tools/prepare-logo.py "/path/to/Pixel Magnet pUT.io Logo.png"

Needs Pillow. It is not part of the build or of CI: the result (assets/brand/putio-logo.png) is committed,
and its source and digest are recorded in assets/PROVENANCE.md.

Only background that is connected to the image border becomes transparent (a flood fill), so the light
areas *inside* letters (the counters of "p" and "o") are kept. A short alpha ramp softens the edge.
"""
import sys
from collections import deque
from pathlib import Path

from PIL import Image

OUT = Path(__file__).resolve().parent.parent / "assets" / "brand" / "putio-logo.png"
WIDTH = 900  # shown at ~40 logical px tall; 900 px wide stays sharp up to 3x displays
SOLID_ABOVE = 244   # a pixel whose darkest channel is above this is background
FADE_FROM = 214     # below this it is ink; in between it fades out


def is_background_candidate(pixel):
    return min(pixel[:3]) >= FADE_FROM


def main():
    source = Image.open(sys.argv[1]).convert("RGB")
    width, height = source.size
    pixels = source.load()

    outside = [[False] * width for _ in range(height)]
    queue = deque()
    for x in range(width):
        queue.extend([(x, 0), (x, height - 1)])
    for y in range(height):
        queue.extend([(0, y), (width - 1, y)])
    while queue:
        x, y = queue.popleft()
        if not (0 <= x < width and 0 <= y < height) or outside[y][x]:
            continue
        if not is_background_candidate(pixels[x, y]):
            continue
        outside[y][x] = True
        queue.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)])

    result = Image.new("RGBA", (width, height))
    out = result.load()
    for y in range(height):
        for x in range(width):
            r, g, b = pixels[x, y]
            if outside[y][x]:
                darkest = min(r, g, b)
                alpha = 0 if darkest >= SOLID_ABOVE else round(255 * (SOLID_ABOVE - darkest) / (SOLID_ABOVE - FADE_FROM))
                out[x, y] = (r, g, b, alpha)
            else:
                out[x, y] = (r, g, b, 255)

    scaled = result.resize((WIDTH, round(height * WIDTH / width)), Image.LANCZOS)
    scaled.save(OUT, optimize=True)
    print(f"wrote {OUT} ({scaled.size[0]}x{scaled.size[1]})")


if __name__ == "__main__":
    main()
