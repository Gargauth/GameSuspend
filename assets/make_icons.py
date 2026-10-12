"""Generates the GameSuspend icons. Requires Pillow: `python assets/make_icons.py`.

app.ico       - application and idle tray icon (blue, pause bars)
suspended.ico - tray icon while a game is suspended (night blue, crescent moon)
icon.png      - 256 px preview for the README
"""

from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
SS = 8  # supersampling factor


def rounded_bg(d: ImageDraw.ImageDraw, s: int, top: tuple, bottom: tuple) -> None:
    """Vertical gradient inside a rounded square covering the whole canvas."""
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, s - 1, s - 1), radius=round(s * 0.22), fill=255)
    grad = Image.new("RGBA", (s, s))
    gd = ImageDraw.Draw(grad)
    for y in range(s):
        t = y / (s - 1)
        gd.line([(0, y), (s, y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)) + (255,))
    d._image.paste(grad, (0, 0), mask)


def draw_idle(s: int) -> Image.Image:
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    rounded_bg(d, s, (59, 130, 246), (29, 78, 216))
    bar_w, bar_h, gap = s * 0.16, s * 0.48, s * 0.12
    y0 = (s - bar_h) / 2
    for x0 in (s / 2 - gap / 2 - bar_w, s / 2 + gap / 2):
        d.rounded_rectangle((x0, y0, x0 + bar_w, y0 + bar_h), radius=bar_w * 0.3, fill=(255, 255, 255, 255))
    return img


def draw_suspended(s: int) -> Image.Image:
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    rounded_bg(d, s, (67, 56, 202), (30, 27, 75))
    # Crescent moon: full disc minus an offset disc.
    moon = Image.new("L", (s, s), 0)
    md = ImageDraw.Draw(moon)
    r = s * 0.29
    cx, cy = s * 0.47, s * 0.52
    md.ellipse((cx - r, cy - r, cx + r, cy + r), fill=255)
    ox, oy = cx + r * 0.55, cy - r * 0.45
    md.ellipse((ox - r * 0.85, oy - r * 0.85, ox + r * 0.85, oy + r * 0.85), fill=0)
    img.paste((253, 224, 71, 255), (0, 0), moon)
    return img


def render(draw, size: int) -> Image.Image:
    return draw(size * SS).resize((size, size), Image.LANCZOS)


def save_ico(draw, name: str) -> list:
    frames = [render(draw, s) for s in SIZES]
    frames[-1].save(HERE / name, format="ICO", sizes=[(s, s) for s in SIZES], append_images=frames[:-1])
    return frames


if __name__ == "__main__":
    idle = save_ico(draw_idle, "app.ico")
    save_ico(draw_suspended, "suspended.ico")
    idle[-1].save(HERE / "icon.png")
