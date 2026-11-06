#!/usr/bin/env python3
"""One-off asset step: build the light and dark app icons from the supplied magnet artwork.

    python3 tools/make-app-icon.py "/path/to/Pixel Art Horseshoe Magnet with Sparks.png" "/path/to/Pixel Art Magnet U Icon.png"
                                    (light: on white)                                      (dark: on dark grey)

Writes, for each of `light` and `dark`, a macOS-style rounded icon (1024 px master with the usual margin
and soft shadow) holding the magnet, plus the derived sizes under assets/icon/. Needs Pillow; not part
of the build or CI. tools/make-icns.sh then turns the masters into AppIcon.icns and the .ico.

The supplied images are the magnet alone on a flat background (white, or dark grey with a faint texture).
The background is made transparent by a flood fill from the border over pixels close to the border's own
colour, so a pixel of the same colour inside the magnet is kept; a short ramp softens the edge, and specks
that are not part of the magnet are dropped before it is cropped.
"""
import sys
from collections import deque
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "icon"
SIZE = 1024
BODY = 824          # the rounded square, as in the macOS icon template (the rest is margin for the shadow)
RADIUS = 186
SUPERSAMPLE = 3
# Every size that is written: macOS (16 to 1024 in the .icns), Windows (16 to 256 in the .ico, with the 24 and
# 48 that Explorer and the taskbar ask for), the window and the Dock (256 and 512), Linux (256).
SIZES = (512, 256, 128, 64, 48, 32, 24, 16)

# How far (Euclidean, in 0..255 RGB) a pixel may be from the background colour to count as background: fully
# transparent below INNER, fully opaque above OUTER, a ramp between.
INNER, OUTER = 14.0, 34.0
# Pieces of the cut-out smaller than this many pixels are noise, not part of the magnet (the smallest spark
# is thousands of pixels).
MIN_PIECE = 400

THEMES = {
    "light": {"top": (0xFF, 0xFF, 0xFF), "bottom": (0xEE, 0xEE, 0xE8), "edge": (0xD6, 0xD6, 0xCE)},
    "dark": {"top": (0x34, 0x34, 0x3A), "bottom": (0x1B, 0x1B, 0x1E), "edge": (0x4E, 0x4E, 0x56)},
}


def border_colour(pixels, width, height):
    """The median colour of the image's border: what the background is."""
    samples = []
    for x in range(width):
        samples += [pixels[x, 0], pixels[x, height - 1]]
    for y in range(height):
        samples += [pixels[0, y], pixels[width - 1, y]]
    return tuple(sorted(sample[channel] for sample in samples)[len(samples) // 2] for channel in range(3))


def cut_out(source):
    """RGBA copy of `source` with the border-connected background made transparent."""
    source = source.convert("RGB")
    width, height = source.size
    pixels = source.load()
    background = border_colour(pixels, width, height)

    def distance(pixel):
        return sum((a - b) ** 2 for a, b in zip(pixel, background)) ** 0.5

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
        if distance(pixels[x, y]) >= OUTER:
            continue
        outside[y][x] = True
        queue.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)])

    result = Image.new("RGBA", (width, height))
    out = result.load()
    for y in range(height):
        for x in range(width):
            r, g, b = pixels[x, y]
            alpha = 255
            if outside[y][x]:
                d = distance((r, g, b))
                alpha = 0 if d <= INNER else round(255 * (d - INNER) / (OUTER - INNER))
            out[x, y] = (r, g, b, alpha)
    return result


def drop_specks(image, min_piece=MIN_PIECE):
    """Remove opaque pieces smaller than `min_piece` pixels (noise left in the background)."""
    width, height = image.size
    alpha = image.getchannel("A").load()
    pixels = image.load()
    seen = [[False] * width for _ in range(height)]
    for sy in range(height):
        for sx in range(width):
            if seen[sy][sx] or alpha[sx, sy] == 0:
                continue
            stack, members = [(sx, sy)], []
            seen[sy][sx] = True
            while stack:
                x, y = stack.pop()
                members.append((x, y))
                for nx, ny in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
                    if 0 <= nx < width and 0 <= ny < height and not seen[ny][nx] and alpha[nx, ny] > 0:
                        seen[ny][nx] = True
                        stack.append((nx, ny))
            if len(members) < min_piece:
                for x, y in members:
                    pixels[x, y] = (0, 0, 0, 0)
    return image


def squircle_mask(size, body, radius, scale):
    big = size * scale
    mask = Image.new("L", (big, big), 0)
    inset = (big - body * scale) // 2
    ImageDraw.Draw(mask).rounded_rectangle((inset, inset, big - inset - 1, big - inset - 1), radius=radius * scale, fill=255)
    return mask.resize((size, size), Image.LANCZOS)


def gradient(size, top, bottom):
    column = Image.new("RGB", (1, size))
    for y in range(size):
        t = y / (size - 1)
        column.putpixel((0, y), tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)))
    return column.resize((size, size))


def build(theme, source_path):
    spec = THEMES[theme]
    logo = drop_specks(cut_out(Image.open(source_path)))
    logo = logo.crop(logo.getbbox())

    mask = squircle_mask(SIZE, BODY, RADIUS, SUPERSAMPLE)
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))

    # Soft shadow under the body.
    shadow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    shadow.putalpha(mask.point(lambda v: round(v * 0.38)))
    shadow = ImageChops.offset(shadow, 0, 14).filter(ImageFilter.GaussianBlur(16))
    canvas.alpha_composite(shadow)

    # Body: gradient, with a thin edge line just inside the shape.
    body = gradient(SIZE, spec["top"], spec["bottom"]).convert("RGBA")
    body.putalpha(mask)
    canvas.alpha_composite(body)
    inner = mask.filter(ImageFilter.MinFilter(5))
    ring = ImageChops.subtract(mask, inner).point(lambda v: round(v * 0.9))
    edge = Image.new("RGBA", (SIZE, SIZE), spec["edge"] + (255,))
    edge.putalpha(ring)
    canvas.alpha_composite(edge)

    # The magnet, centred (nudged down a little: the sparks give the top extra visual weight).
    target_height = round(BODY * 0.70)
    scale = target_height / logo.height
    glyph = logo.resize((round(logo.width * scale), target_height), Image.LANCZOS)
    x = (SIZE - glyph.width) // 2
    y = (SIZE - glyph.height) // 2 + 14
    canvas.alpha_composite(glyph, (x, y))
    return canvas


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    OUT.mkdir(parents=True, exist_ok=True)
    for theme, source in zip(("light", "dark"), sys.argv[1:]):
        master = build(theme, source)
        master.save(OUT / f"app-{theme}-1024.png", optimize=True)
        for size in SIZES:
            master.resize((size, size), Image.LANCZOS).save(OUT / f"app-{theme}-{size}.png", optimize=True)
        print(f"wrote {theme} icons")


if __name__ == "__main__":
    main()
