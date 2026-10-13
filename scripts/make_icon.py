"""Generate Nova's app icon source image (app-icon.png).

The Tauri icon set is then generated from it with:

    npx tauri icon app-icon.png -o src-tauri/icons

Requires Pillow (pip install pillow).
"""
from __future__ import annotations

import os

from PIL import Image, ImageDraw, ImageFilter

SIZE = 1024
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "app-icon.png")


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def main() -> None:
    img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))

    # Rounded-square dark background with a subtle vertical gradient.
    bg = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    bgd = ImageDraw.Draw(bg, "RGBA")
    for y in range(SIZE):
        t = y / SIZE
        bgd.rectangle([0, y, SIZE, y + 1], fill=lerp((18, 16, 34), (9, 8, 18), t) + (255,))
    mask = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, SIZE - 1, SIZE - 1], radius=int(SIZE * 0.22), fill=255
    )
    img.paste(bg, (0, 0), mask)

    cx = cy = SIZE // 2
    max_r = int(SIZE * 0.335)
    violet = (124, 92, 255)
    cyan = (64, 224, 255)

    # Outer glow.
    glow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow, "RGBA")
    steps = 200
    for i in range(steps, 0, -1):
        r = int(max_r * i / steps)
        t = i / steps
        col = lerp(violet, cyan, 1 - t)
        alpha = int(64 * (1 - t) + 6)
        gd.ellipse(
            [cx - r, cy - r, cx + r, cy + r],
            outline=col + (alpha,),
            width=max(2, int(5 * (1 - t) + 1)),
        )
        if t < 0.5:
            gd.ellipse(
                [cx - r, cy - r, cx + r, cy + r],
                fill=col + (int(110 * (0.5 - t) / 0.5),),
            )
    glow = glow.filter(ImageFilter.GaussianBlur(46))
    img.alpha_composite(glow)

    # Solid core disc with soft edge.
    core_r = int(max_r * 0.6)
    core = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    cd = ImageDraw.Draw(core, "RGBA")
    for i in range(core_r, 0, -1):
        t = i / core_r
        col = lerp((152, 122, 255), (96, 212, 255), 1 - t)
        cd.ellipse([cx - i, cy - i, cx + i, cy + i], fill=col + (255,))
    core = core.filter(ImageFilter.GaussianBlur(8))
    img.alpha_composite(core)

    # "N" monogram.
    d = ImageDraw.Draw(img, "RGBA")
    w = int(SIZE * 0.05)
    left = cx - int(SIZE * 0.125)
    right = cx + int(SIZE * 0.125)
    top = cy - int(SIZE * 0.135)
    bottom = cy + int(SIZE * 0.135)
    stroke = (255, 255, 255, 240)

    d.line([left, bottom, left, top], fill=stroke, width=w)
    d.line([left, top, right, bottom], fill=stroke, width=w)
    d.line([right, bottom, right, top], fill=stroke, width=w)

    # Rounded endpoints.
    half = w // 2
    for (x, y) in [(left, top), (left, bottom), (right, top), (right, bottom)]:
        d.ellipse([x - half, y - half, x + half, y + half], fill=stroke)

    img.save(OUT)
    print(f"wrote {os.path.abspath(OUT)}")


if __name__ == "__main__":
    main()
