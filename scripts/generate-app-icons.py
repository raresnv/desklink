#!/usr/bin/env python3
"""Generate DeskLink's vector/native icons from shared geometry (requires Pillow)."""
from pathlib import Path
from PIL import Image, ImageDraw
import tempfile
import subprocess
import sys

root = Path(__file__).resolve().parent.parent
out = root / 'assets/icons'
out.mkdir(parents=True, exist_ok=True)
background = '#145f4d'
screens = [(218, 272, 638, 584, '#c5e3a1'), (362, 432, 782, 744, '#f7f8f2')]
svg = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">',
       '  <title>DeskLink — linked screens</title>',
       f'  <rect x="48" y="48" width="928" height="928" rx="210" fill="{background}"/>']
for x0, y0, x1, y1, color in screens:
    svg.append(f'  <rect x="{x0}" y="{y0}" width="{x1-x0}" height="{y1-y0}" rx="62" fill="{background}" stroke="{color}" stroke-width="48"/>')
svg.append('</svg>')
(out / 'desklink.svg').write_text('\n'.join(svg)+'\n')


def render(size):
    # Draw above output resolution for smooth transparent edges. Small sizes use
    # a slightly heavier stroke to keep the two screens legible in a title bar.
    scale = max(2, 4096 // size)
    canvas = size * scale
    factor = canvas / 1024
    image = Image.new('RGBA', (canvas, canvas))
    draw = ImageDraw.Draw(image)
    def box(rect):
        return tuple(round(v * factor) for v in rect)
    draw.rounded_rectangle(box((48, 48, 976, 976)), radius=round(210*factor), fill=background)
    stroke = 60 if size <= 32 else 48
    # Pillow strokes sit inside a rectangle; expand the bounds by half the SVG
    # stroke so both exports share the same outside and inside edges.
    for x0, y0, x1, y1, color in screens:
        half = stroke / 2
        draw.rounded_rectangle(box((x0-half, y0-half, x1+half, y1+half)),
                               radius=round((62+half)*factor), fill=color)
        draw.rounded_rectangle(box((x0+half, y0+half, x1-half, y1-half)),
                               radius=round((62-half)*factor), fill=background)
    image = image.resize((size, size), Image.Resampling.LANCZOS)
    image.putalpha(image.getchannel('A').point(lambda value: 0 if value <= 1 else value))
    return image


sizes = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
for size in sizes:
    render(size).save(out / f'desklink-{size}.png')
render(256).save(out / 'desklink.ico', sizes=[(s, s) for s in sizes if s <= 256],
                 append_images=[render(s) for s in sizes if s < 256])
if sys.platform == 'darwin':
    with tempfile.TemporaryDirectory(suffix='.iconset') as temp:
        iconset = Path(temp)
        for size in [16, 32, 128, 256, 512]:
            render(size).save(iconset / f'icon_{size}x{size}.png')
            render(size*2).save(iconset / f'icon_{size}x{size}@2x.png')
        subprocess.run(['iconutil', '-c', 'icns', str(iconset), '-o', str(out / 'DeskLink.icns')], check=True)
else:
    print('Use macOS iconutil to regenerate DeskLink.icns; PNG/ICO/SVG generated.')
print(f'Generated icons in {out}')
