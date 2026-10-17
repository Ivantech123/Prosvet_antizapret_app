"""Рисует логотип Prosvet: тёмный скруглённый квадрат, в нём вертикальная
щель, через которую пробивается луч света. Плюс иконки трея по состояниям."""
from PIL import Image, ImageDraw, ImageFilter
import os

ROOT = os.path.join(os.path.dirname(__file__), "..")
S = 1024

def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(len(a)))

def logo(size=S, accent=(255, 214, 120), accent2=(255, 140, 70), bg_top=(28, 30, 38), bg_bot=(12, 13, 18), dim=False):
    k = size / S
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    # фон с вертикальным градиентом
    bg = Image.new("RGBA", (size, size))
    d = ImageDraw.Draw(bg)
    for y in range(size):
        d.line([(0, y), (size, y)], fill=lerp(bg_top, bg_bot, y / size) + (255,))
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, size - 1, size - 1], radius=int(230 * k), fill=255)
    img.paste(bg, (0, 0), mask)

    # свечение луча
    glow = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    g = ImageDraw.Draw(glow)
    cx = size // 2
    g.polygon([(cx - 40 * k, 170 * k), (cx + 40 * k, 170 * k), (cx + 330 * k, 900 * k), (cx - 330 * k, 900 * k)],
              fill=accent + (90 if not dim else 30,))
    glow = glow.filter(ImageFilter.GaussianBlur(70 * k))
    img.alpha_composite(Image.composite(glow, Image.new("RGBA", (size, size)), mask))

    # луч (веер из щели)
    ray = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    r = ImageDraw.Draw(ray)
    steps = 60
    for i in range(steps):
        t = i / steps
        w0 = 26 * k
        w1 = (60 + 230 * (1 - t)) * k
        a = int((150 if not dim else 50) * (1 - t) ** 1.6)
        col = lerp(accent, accent2, t) + (a,)
        r.polygon([(cx - w0, 190 * k), (cx + w0, 190 * k), (cx + w1, 880 * k), (cx - w1, 880 * k)], fill=col)
    ray = ray.filter(ImageFilter.GaussianBlur(10 * k))
    img.alpha_composite(Image.composite(ray, Image.new("RGBA", (size, size)), mask))

    # сама щель — яркая вертикальная полоса
    slit = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    sd = ImageDraw.Draw(slit)
    sd.rounded_rectangle([cx - 30 * k, 140 * k, cx + 30 * k, 560 * k], radius=int(30 * k),
                         fill=(255, 250, 235, 255) if not dim else (150, 150, 160, 255))
    img.alpha_composite(slit.filter(ImageFilter.GaussianBlur(2 * k)))
    return img

def tray(color, ring=None, size=64):
    """Маленькая иконка трея: щель-полоска в круге состояния."""
    big = 256
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([8, 8, big - 8, big - 8], radius=64, fill=(22, 24, 30, 255))
    cx = big // 2
    d.polygon([(cx - 14, 60), (cx + 14, 60), (cx + 80, 220), (cx - 80, 220)], fill=color + (110,))
    d.rounded_rectangle([cx - 16, 40, cx + 16, 150], radius=16, fill=color + (255,))
    if ring:
        d.ellipse([big - 92, big - 92, big - 12, big - 12], fill=ring + (255,), outline=(22, 24, 30, 255), width=10)
    return img.resize((size, size), Image.LANCZOS)

os.makedirs(os.path.join(ROOT, "assets"), exist_ok=True)
logo().save(os.path.join(ROOT, "assets", "logo.png"))
logo(256).save(os.path.join(ROOT, "ui", "logo.png"))
icons = os.path.join(ROOT, "src-tauri", "icons")
os.makedirs(icons, exist_ok=True)
tray((255, 214, 120), (64, 200, 120)).save(os.path.join(icons, "tray-on.png"))
tray((140, 144, 156)).save(os.path.join(icons, "tray-off.png"))
tray((255, 214, 120), (90, 150, 255)).save(os.path.join(icons, "tray-busy.png"))
print("ok")
