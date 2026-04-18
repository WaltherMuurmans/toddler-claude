#!/usr/bin/env python3
"""Generate placeholder Tauri icons. Run from project root:
    python scripts/gen-icons.py
Creates src-tauri/icons/*.png and icon.ico with a simple green/dark-blue 'TC' mark.
"""
from PIL import Image, ImageDraw, ImageFont
from pathlib import Path
import sys

OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
OUT.mkdir(parents=True, exist_ok=True)

BG = (15, 23, 42, 255)   # slate-900
FG = (34, 197, 94, 255)  # green-500

def draw(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), BG)
    d = ImageDraw.Draw(img)
    # Rounded square accent
    pad = int(size * 0.12)
    d.rounded_rectangle(
        [pad, pad, size - pad, size - pad],
        radius=int(size * 0.18),
        outline=FG,
        width=max(2, size // 32),
    )
    # "TC" letters, positioned inside
    try:
        font = ImageFont.truetype("arialbd.ttf", int(size * 0.42))
    except Exception:
        try:
            font = ImageFont.truetype("DejaVuSans-Bold.ttf", int(size * 0.42))
        except Exception:
            font = ImageFont.load_default()
    text = "TC"
    bbox = d.textbbox((0, 0), text, font=font)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    d.text(((size - tw) / 2 - bbox[0], (size - th) / 2 - bbox[1]),
           text, font=font, fill=FG)
    return img

sizes = {
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 512,
    "Square30x30Logo.png": 30,
    "Square44x44Logo.png": 44,
    "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89,
    "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142,
    "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284,
    "Square310x310Logo.png": 310,
    "StoreLogo.png": 50,
}
for name, s in sizes.items():
    draw(s).save(OUT / name)
    print("wrote", OUT / name)

# ICO with several nested sizes
ico_sizes = [16, 24, 32, 48, 64, 128, 256]
ico_imgs = [draw(s) for s in ico_sizes]
ico_path = OUT / "icon.ico"
ico_imgs[0].save(ico_path, format="ICO",
                 sizes=[(s, s) for s in ico_sizes],
                 append_images=ico_imgs[1:])
print("wrote", ico_path)

print("done.")
