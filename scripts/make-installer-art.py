"""Картинки для NSIS-установщика в стиле «Луч»:
sidebar 164x314 (экраны приветствия/завершения) и header 150x57."""
from PIL import Image, ImageDraw, ImageFilter, ImageFont
import os

ROOT = os.path.join(os.path.dirname(__file__), "..")
OUT = os.path.join(ROOT, "src-tauri", "installer")
FONT = os.path.join(ROOT, "assets", "Unbounded.ttf")
os.makedirs(OUT, exist_ok=True)

def font(size, weight=600):
    f = ImageFont.truetype(FONT, size)
    try:
        f.set_variation_by_axes([weight])
    except Exception:
        pass
    return f

def beam(w, h, cx, top, spread, scale=4):
    """Луч из щели — рисуем крупно и уменьшаем для сглаживания."""
    W, H = w * scale, h * scale
    img = Image.new("RGB", (W, H), (8, 9, 13))
    d = ImageDraw.Draw(img)
    for y in range(H):  # фон: чуть светлее сверху
        t = y / H
        c = tuple(int(a + (b - a) * t) for a, b in zip((18, 19, 27), (6, 7, 10)))
        d.line([(0, y), (W, y)], fill=c)
    glow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    g = ImageDraw.Draw(glow)
    X, T = cx * scale, top * scale
    for mul, a in ((1.5, 70), (1.0, 120), (0.45, 170), (0.16, 230)):
        half = spread * W * mul
        g.polygon([(X - 6, T), (X + 6, T), (X + half, H * 1.05), (X - half, H * 1.05)], fill=(255, 192, 104, a))
    # затухание книзу
    fade = Image.linear_gradient("L").resize((W, H)).point(lambda v: int(255 - v * 0.6))
    glow.putalpha(Image.composite(glow.getchannel("A"), Image.new("L", (W, H), 0), fade))
    glow = glow.filter(ImageFilter.GaussianBlur(5 * scale))
    img = Image.alpha_composite(img.convert("RGBA"), glow)
    halo = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    ImageDraw.Draw(halo).ellipse([X - 70 * scale, T - 50 * scale, X + 70 * scale, T + 90 * scale], fill=(255, 200, 120, 70))
    img = Image.alpha_composite(img, halo.filter(ImageFilter.GaussianBlur(28 * scale)))
    # щель
    slit = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    sd = ImageDraw.Draw(slit)
    sd.rounded_rectangle([X - 2.5 * scale, T - 46 * scale, X + 2.5 * scale, T + 2 * scale], radius=3 * scale, fill=(255, 220, 160, 255))
    img = Image.alpha_composite(img, slit.filter(ImageFilter.GaussianBlur(5 * scale)))
    sd2 = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    ImageDraw.Draw(sd2).rounded_rectangle([X - 1.6 * scale, T - 44 * scale, X + 1.6 * scale, T + 1 * scale], radius=2 * scale, fill=(255, 250, 236, 255))
    img = Image.alpha_composite(img, sd2)
    # пылинки
    import random
    random.seed(7)
    dust = ImageDraw.Draw(img)
    for _ in range(45):
        x, y = random.uniform(0, W), random.uniform(T, H)
        inside = max(0, 1 - abs(x - X) / (6 + (y - T) / (H - T) * spread * W))
        r = random.uniform(0.4, 1.2) * scale
        a = int(30 + inside * 200)
        dust.ellipse([x - r, y - r, x + r, y + r], fill=(255, 214, 150, a) if inside > 0.05 else (210, 215, 230, 18))
    return img.resize((w, h), Image.LANCZOS).convert("RGB")

# --- боковая панель 164x314 ---
side = beam(164, 314, 82, 70, 0.55)
d = ImageDraw.Draw(side)
title = "Prosvet"
f = font(21, 600)
tw = d.textlength(title, font=f)
d.text(((164 - tw) / 2, 228), title, font=f, fill=(244, 241, 233))
f2 = font(8, 500)
sub = "ОБХОД БЛОКИРОВОК"
d.text(((164 - d.textlength(sub, font=f2)) / 2, 262), sub, font=f2, fill=(255, 198, 110))
side.save(os.path.join(OUT, "sidebar.bmp"))

# --- шапка 150x57 ---
head = beam(150, 57, 26, 14, 0.18)
d = ImageDraw.Draw(head)
d.text((46, 17), "Prosvet", font=font(15, 600), fill=(244, 241, 233))
head.save(os.path.join(OUT, "header.bmp"))

# превью для проверки
prev = Image.new("RGB", (164 + 20 + 150, 314), (240, 240, 240))
prev.paste(side, (0, 0)); prev.paste(head, (184, 0))
prev.save(os.path.join(OUT, "_preview.png"))
print("ok")
