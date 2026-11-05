#!/usr/bin/env python3
"""One-off asset step: turn a supplied logo (opaque, flat background) into a transparent PNG.

    python3 tools/prepare-logo.py light "/path/to/Pixel Magnet pUT.io Logo.png"       # near-white background
    python3 tools/prepare-logo.py dark  "/path/to/Pixel Art Magnet Logo.png"          # near-black background

Needs Pillow. It is not part of the build or of CI: the results (assets/brand/putio-logo-light.png and
-dark.png) are committed, and their sources and digests are recorded in assets/PROVENANCE.md.

Only background that is connected to the image border becomes transparent (a flood fill), so areas of the
same colour *inside* letters (the counters of "p" and "o") are kept. A short alpha ramp softens the edge.
"""
import sys
from collections import deque
from pathlib import Path

from PIL import Image

BRAND = Path(__file__).resolve().parent.parent / "assets" / "brand"
WIDTH = 900  # shown at ~40 logical px tall; 900 px wide stays sharp up to 3x displays

# For each variant: how "background-like" a pixel is (higher = more background), and the ramp.
# `score(pixel)` runs from 0 (ink) up to 255 (pure background); alpha fades between the two limits.
VARIANTS = {
    # White background: the darkest channel tells how close to white a pixel is.
    "light": {"score": lambda p: min(p[:3]), "ink_below": 214, "bg_above": 244},
    # Black background: invert, so near-black scores high. Letters are dark grey (about 44), so the
    # ramp has to end below that.
    "dark": {"score": lambda p: 255 - max(p[:3]), "ink_below": 255 - 34, "bg_above": 255 - 6},
}


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in VARIANTS:
        sys.exit(__doc__)
    variant = VARIANTS[sys.argv[1]]
    score, ink_below, bg_above = variant["score"], variant["ink_below"], variant["bg_above"]
    source = Image.open(sys.argv[2]).convert("RGB")
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
        if score(pixels[x, y]) < ink_below:
            continue
        outside[y][x] = True
        queue.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)])

    result = Image.new("RGBA", (width, height))
    out = result.load()
    for y in range(height):
        for x in range(width):
            r, g, b = pixels[x, y]
            if outside[y][x]:
                value = score((r, g, b))
                alpha = 0 if value >= bg_above else round(255 * (bg_above - value) / (bg_above - ink_below))
                out[x, y] = (r, g, b, alpha)
            else:
                out[x, y] = (r, g, b, 255)

    scaled = result.resize((WIDTH, round(height * WIDTH / width)), Image.LANCZOS)
    target = BRAND / f"putio-logo-{sys.argv[1]}.png"
    scaled.save(target, optimize=True)
    print(f"wrote {target} ({scaled.size[0]}x{scaled.size[1]})")


if __name__ == "__main__":
    main()
