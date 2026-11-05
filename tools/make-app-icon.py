#!/usr/bin/env python3
"""One-off asset step: build the light and dark app icons from the supplied logos.

    python3 tools/make-app-icon.py "/path/to/Pixel Magnet pUT.io Logo.png" "/path/to/Pixel Art Magnet Logo.png"
                                    (light-background logo)                 (dark-background logo)

Writes, for each of `light` and `dark`, a macOS-style rounded icon (1024 px master with the usual margin
and soft shadow) holding the logo's magnet, plus the derived sizes under assets/icon/. Needs Pillow; not part
of the build or CI. tools/make-icns.sh then turns the masters into AppIcon.icns and the .ico.
"""
import importlib.util
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "icon"
SIZE = 1024
BODY = 824          # the rounded square, as in the macOS icon template (the rest is margin for the shadow)
RADIUS = 186
SUPERSAMPLE = 3

# Where the magnet sits in the 2170 x 725 source images (found from its red pixels), with room for the
# sparks above and the yellow shadow around it. The right edge stops short of the "T".
MAGNET_BOX = (478, 24, 932, 612)

THEMES = {
    "light": {"top": (0xFF, 0xFF, 0xFF), "bottom": (0xEE, 0xEE, 0xE8), "edge": (0xD6, 0xD6, 0xCE), "variant": "light"},
    "dark": {"top": (0x34, 0x34, 0x3A), "bottom": (0x1B, 0x1B, 0x1E), "edge": (0x4E, 0x4E, 0x56), "variant": "dark"},
}


def load_cut_out():
    spec = importlib.util.spec_from_file_location("prepare_logo", ROOT / "tools" / "prepare-logo.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.cut_out


def drop_stray_fragments(image, max_area=4000):
    """Remove small opaque fragments that touch the crop's left or right edge (slivers of the neighbouring
    letters). The sparks above the magnet are small too, but they are inside the crop, not on its edge."""
    width, height = image.size
    alpha = image.getchannel("A").load()
    seen = [[False] * width for _ in range(height)]
    pixels = image.load()
    for sy in range(height):
        for sx in range(width):
            if seen[sy][sx] or alpha[sx, sy] == 0:
                continue
            stack, members, touches_edge = [(sx, sy)], [], False
            seen[sy][sx] = True
            while stack:
                x, y = stack.pop()
                members.append((x, y))
                touches_edge = touches_edge or x in (0, width - 1)
                for nx, ny in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
                    if 0 <= nx < width and 0 <= ny < height and not seen[ny][nx] and alpha[nx, ny] > 0:
                        seen[ny][nx] = True
                        stack.append((nx, ny))
            if touches_edge and len(members) < max_area:
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


def build(theme, cut_out, source_path):
    spec = THEMES[theme]
    # The icon keeps the magnet's own colours: the letter-brightening used for the in-app logo is not wanted here.
    logo = cut_out(Image.open(source_path), spec["variant"], lift=1.0).crop(MAGNET_BOX)
    logo = drop_stray_fragments(logo)
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
    cut_out = load_cut_out()
    OUT.mkdir(parents=True, exist_ok=True)
    for theme, source in zip(("light", "dark"), sys.argv[1:]):
        master = build(theme, cut_out, source)
        master.save(OUT / f"app-{theme}-1024.png", optimize=True)
        for size in (512, 256, 128, 64, 32, 16):
            master.resize((size, size), Image.LANCZOS).save(OUT / f"app-{theme}-{size}.png", optimize=True)
        print(f"wrote {theme} icons")


if __name__ == "__main__":
    main()
