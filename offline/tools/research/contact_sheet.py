"""Contact sheet of a folder's PNG frames, labelled by file name: contact_sheet.py DIR [COLS]"""
import sys
from pathlib import Path

from PIL import Image, ImageDraw

d = Path(sys.argv[1])
cols = int(sys.argv[2]) if len(sys.argv) > 2 else 4
fs = sorted(d.glob('*.png'))
w, h = 637, 360
s = Image.new('RGB', (w * cols, h * ((len(fs) + cols - 1) // cols)))
for i, f in enumerate(fs):
    im = Image.open(f).resize((w, h))
    ImageDraw.Draw(im).text((5, 5), f.stem, fill=(255, 255, 0))
    s.paste(im, ((i % cols) * w, (i // cols) * h))
out = d.parent / f'{d.name}-sheet.png'
s.save(out)
print(len(fs), out)
