"""Independently check photo assets/reference results and render measured figures.

Requires Pillow and NumPy; takes the texture_photo_probe output directory.
"""
import csv
import json
import math
from pathlib import Path
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

PHOTOS = ['peppers', 'mandrill', 'sailboat', 'airplane']
SCENES = [640, 400, 256, 160, 80, 20, 3]


def rgb565(image):
    rgb = np.asarray(image, dtype=np.float64)
    q = np.rint(rgb * np.array([31, 63, 31]) / 255).astype(np.uint16)
    return np.stack(((q[:, :, 0] << 3) | (q[:, :, 0] >> 2),
                     (q[:, :, 1] << 2) | (q[:, :, 1] >> 4),
                     (q[:, :, 2] << 3) | (q[:, :, 2] >> 2)), axis=-1).astype(float)


def sample(levels, u, v, lod):
    total = np.zeros(np.broadcast_arrays(u, v)[0].shape + (3,))
    level = math.floor(lod)
    for l, parent in [(level, 1 - (lod - level)), (level + 1, lod - level)]:
        if parent == 0:
            continue
        n = 9 - l
        a, logical = 1 << max(n, 1), 1 << n
        px, py = np.mod(u, 1) * a - 0.5, np.mod(v, 1) * a - 0.5
        ix, iy = np.floor(px).astype(int), np.floor(py).astype(int)
        fx, fy = px - ix, py - iy
        for dx, dy in [(0, 0), (1, 0), (0, 1), (1, 1)]:
            weight = parent * (fx if dx else 1 - fx) * (fy if dy else 1 - fy)
            total += weight[..., None] * levels[n][(iy + dy) % logical, (ix + dx) % logical]
    return total


def load_csv(path):
    with path.open(newline='') as f:
        return list(csv.DictReader(f))


def summarize(root, report):
    with (root / 'summary.csv').open('w', newline='') as f:
        writer = csv.writer(f)
        writer.writerow(['photo', 'configuration', 'channels', 'max_rgb_code_error', 'mean_rgb_code_error'])
        for photo in PHOTOS:
            for name in ['baseline', 'unorm9', 'zero_mask9']:
                rows = [r for r in report if r['photo'] == photo and r['configuration'] == name]
                count = sum(int(r['channels']) for r in rows)
                total = sum(int(r['channels']) * float(r['mean_rgb_code_error']) for r in rows)
                writer.writerow([photo, name, count, max(float(r['max_rgb_code_error']) for r in rows), total / count])


def check(root, worst):
    checks = {'asset_words': 0, 'reference_render_pixels': 0, 'round_tie_differences': 0,
              'worst_samples': 0, 'max_worst_reference_difference': 0.0}
    for name in PHOTOS:
        levels = {n: rgb565(Image.open(root / 'assets' / f'{name}-mip{n}.png')) for n in range(10)}
        raw = np.frombuffer((root / 'assets' / f'{name}.raw565').read_bytes(), dtype='<u2')
        prefix = 0
        for n in range(10):
            side = max(8, 1 << n)
            y, x = np.indices((side, side))
            address = prefix + ((y // 8) * (side // 8) + x // 8) * 64 + (y % 8) * 8 + x % 8
            w = raw[address]
            actual = np.stack(((w >> 11) * 8 + (w >> 13),
                ((w >> 5) & 63) * 4 + ((w >> 9) & 3), (w & 31) * 8 + ((w >> 2) & 7)), axis=-1)
            assert np.array_equal(actual, levels[n][y % (1 << n), x % (1 << n)]), name
            prefix += side * side
        assert prefix == len(raw)
        checks['asset_words'] += prefix
        for side in SCENES:
            y, x = np.indices((side, side))
            lod = max(0.0, min(9.0, math.log2(512 / side)))
            rgb = sample(levels, (x + 0.31) / side, (y + 0.67) / side, lod)
            want = np.rint(rgb).astype(np.int16)
            actual = np.asarray(Image.open(root / f'{name}-image{side}-reference.ppm')).astype(np.int16)
            diff = np.abs(want - actual)
            assert diff.max() <= 1
            mismatch = diff > 0
            # Helpers computed at each quad can differ from a global constant
            # derivative by float roundoff. Only genuine half-code ties may differ.
            assert np.all(np.abs(np.mod(rgb[mismatch], 1) - 0.5) < 1e-8)
            checks['round_tie_differences'] += int(mismatch.sum())
            checks['reference_render_pixels'] += side * side
        for r in (r for r in worst if r['photo'] == name):
            uv = np.array([[float(r[f'q{i}u']), float(r[f'q{i}v'])] for i in range(4)])
            slope = max(np.max(np.abs(uv[b] - uv[a])) for a, b in [(0, 1), (2, 3), (0, 2), (1, 3)])
            lod = min(9, max(0, math.log2(slope * 512)))
            assert abs(lod - float(r['ideal_lod'])) < 1e-10
            rgb = sample(levels, np.array(float(r['u'])), np.array(float(r['v'])), lod)
            want = np.array([float(r[f'reference_{c}']) for c in 'rgb'])
            error = float(np.abs(rgb - want).max())
            assert error < 1e-8, (name, r['configuration'], error)
            checks['worst_samples'] += 1
            checks['max_worst_reference_difference'] = max(checks['max_worst_reference_difference'], error)
    (root / 'independent-check.json').write_text(json.dumps(checks, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(checks))


def label(draw, x, y, text, size=20, color='#182332'):
    draw.text((x, y), text, fill=color, font=ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf', size))


def figures(root, worst):
    image = Image.new('RGB', (1120, 1280), '#f4f5f7')
    draw = ImageDraw.Draw(image)
    label(draw, 20, 12, 'Classic photos: coherent mip chains, 512 x 512 / UV18', 28)
    label(draw, 20, 54, '400 x 400 sampled images; actual colors, no amplified error.', 20)
    columns = [('Source photo', None), ('Continuous reference', 'reference'),
               ('UNORM9 ( / 511 )', 'unorm9'), ('9-bit + zero mask ( / 512 )', 'zero_mask9')]
    for i, (title, _) in enumerate(columns):
        label(draw, 20 + i * 280, 89, title, 19)
    for row, name in enumerate(PHOTOS):
        y = 129 + row * 281
        for col, (_, config) in enumerate(columns):
            path = root / 'assets' / f'{name}.tiff' if config is None else root / f'{name}-image400-{config}.ppm'
            photo = Image.open(path).convert('RGB').resize((256, 256), Image.Resampling.LANCZOS)
            image.paste(photo, (20 + col * 280, y))
        label(draw, 20, y + 259, name.title(), 17)
    image.save(root / 'comparison.png')

    # The largest maximum among both new formats, including repeat borders.
    r = max((r for r in worst if r['configuration'] != 'baseline'), key=lambda r: float(r['error']))
    image = Image.new('RGB', (1080, 585), '#f4f5f7')
    draw = ImageDraw.Draw(image)
    label(draw, 25, 16, f'Worst measured photo sample: {r["photo"].title()}', 29)
    label(draw, 25, 60, f'Max error {float(r["error"]):.6f} codes ({"RGB"[int(r["channel"])]}); case {r["case"]}, lane {r["lane"]}', 21)
    source = Image.open(root / 'assets' / f'{r["photo"]}.tiff').convert('RGB').resize((360, 360))
    image.paste(source, (25, 130))
    x, y = 25 + (float(r['u']) % 1) * 360, 130 + (float(r['v']) % 1) * 360
    draw.rectangle((x - 12, y - 12, x + 12, y + 12), outline='red', width=3)
    label(draw, 25, 97, 'Source photo; marker at sample UV', 19)
    reference = [float(r[f'reference_{c}']) for c in 'rgb']
    actual = [int(r[f'actual_{c}']) for c in 'rgb']
    for col, (title, rgb, caption) in enumerate([
        ('Continuous reference', [round(c) for c in reference], ', '.join(f'{c:.3f}' for c in reference)),
        ('UNORM9 / zero mask', actual, ', '.join(str(c) for c in actual))]):
        x = 430 + col * 310
        label(draw, x, 140, title, 22)
        draw.rectangle((x, 188, x + 280, 365), fill=tuple(rgb))
        label(draw, x, 385, f'RGB ({caption})', 19)
    label(draw, 430, 440, f'LOD: {float(r["ideal_lod"]):.6f} -> {float(r["actual_lod"]):.6f}', 21)
    label(draw, 430, 479, 'Both new encodings give the same RGB at this sample.', 18)
    label(draw, 25, 540, 'Actual colors; reference rounded only for display. The old LOD table is held fixed.', 19)
    image.save(root / 'worst-sample.png')


def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else 'target/gpu-v2-texture-photos')
    report = load_csv(root / 'precision.csv')
    worst = load_csv(root / 'worst.csv')
    summarize(root, report)
    check(root, worst)
    figures(root, worst)


if __name__ == '__main__':
    main()
