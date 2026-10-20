"""Draws Glint's app icon (assets/AppIcon.icns): a black squircle holding the
mark. A search bar (outline), the selected result (solid) and one more
(outline), with a gold glint beside the selection. Slat's slats, turned on
their side. Run: python3 scripts/make-icon.py"""
import math, os, subprocess, tempfile
from PIL import Image, ImageDraw, ImageFilter

S = 1024 * 4  # supersample, then downscale for smooth edges
k = S / 1024
GOLD = (243, 214, 148, 255)


def astroid(cx, cy, r, n=720):
    """A four-point star with curved sides: x = r cos^3 t, y = r sin^3 t."""
    pts = []
    for i in range(n):
        t = 2 * math.pi * i / n
        c, s = math.cos(t), math.sin(t)
        pts.append((cx + r * math.copysign(abs(c) ** 3, c), cy + r * math.copysign(abs(s) ** 3, s)))
    return pts


img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
# macOS icon grid: 824 px body centred in 1024, ~185 px corner radius
m = 100 * k
d.rounded_rectangle([m, m, S - m, S - m], radius=185 * k, fill=(0, 0, 0, 255))

# the mark, centred with the glint included
w, h = 600 * k, 470 * k
cxf, Rk = 0.93, 105
x0, y0 = (S - (cxf * w + Rk * k)) / 2, (S - h) / 2
stroke = int(24 * k)
cx, cy, R = x0 + cxf * w, y0 + 0.51 * h, Rk * k

# soft glow behind the glint, clipped to the body
glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ImageDraw.Draw(glow).ellipse([cx - R * 1.3, cy - R * 1.3, cx + R * 1.3, cy + R * 1.3], fill=(243, 214, 148, 70))
glow = glow.filter(ImageFilter.GaussianBlur(60 * k))
body = Image.new("L", (S, S), 0)
ImageDraw.Draw(body).rounded_rectangle([m, m, S - m, S - m], radius=185 * k, fill=255)
img.paste(glow, (0, 0), Image.composite(glow, Image.new("RGBA", (S, S)), body).split()[3])
d = ImageDraw.Draw(img)
rows = [(0.0, 0.28, 1.0, False), (0.37, 0.65, 0.72, True), (0.74, 1.02, 0.48, False)]
for top, bottom, frac, solid in rows:
    box = [x0, y0 + top * h, x0 + frac * w, y0 + bottom * h]
    r = (box[3] - box[1]) / 2
    if solid:
        d.rounded_rectangle(box, radius=r, fill=(255, 255, 255, 255))
    else:
        d.rounded_rectangle(box, radius=r, outline=(255, 255, 255, 255), width=stroke)

# the glint
d.polygon(astroid(cx, cy, R), fill=GOLD)

# hairline rim so the icon reads on dark docks
d.rounded_rectangle([m, m, S - m, S - m], radius=185 * k, outline=(255, 255, 255, 38), width=int(4 * k))

icon = img.resize((1024, 1024), Image.LANCZOS)
root = os.path.join(os.path.dirname(__file__), "..", "assets")
icon.save(os.path.join(root, "icon-1024.png"))
with tempfile.TemporaryDirectory() as t:
    iconset = os.path.join(t, "AppIcon.iconset")
    os.mkdir(iconset)
    for size in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            px = size * scale
            name = f"icon_{size}x{size}{'@2x' if scale == 2 else ''}.png"
            icon.resize((px, px), Image.LANCZOS).save(os.path.join(iconset, name))
    subprocess.run(["iconutil", "-c", "icns", iconset, "-o", os.path.join(root, "AppIcon.icns")], check=True)
