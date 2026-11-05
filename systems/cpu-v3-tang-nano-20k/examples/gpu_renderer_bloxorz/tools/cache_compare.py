"""Compare bounded static-cache timing and RGB images, preserving raw differences."""
import argparse
import json
import struct
from pathlib import Path
from PIL import Image, ImageChops, ImageEnhance, ImageDraw


def timing(folder):
    report = json.loads((folder / 'timing.json').read_text())
    assert report['complete'], 'incomplete system run'
    events = report['frame2']
    submits = [e for e in events if e['kind'] == 'cpu_submit']
    dones = [e for e in events if e['kind'] == 'gpu_done']
    assert len(submits) == len(dones) == 2
    begin = next(e['tick'] for e in events if e['kind'] == 'frame_start')
    points = {'frame_start': begin}
    for role, submit in zip(('static', 'dynamic'), submits):
        points[f'cpu_{role}_submit'] = submit['tick']
        points[f'gpu_{role}_done'] = next(e['tick'] for e in dones if e['submission'] == submit['submission'])
    hz = report['quantum_hz']
    relative = {k: (v - begin) * 1000 / hz for k, v in points.items()}
    durations = {
        'frame_to_static_submit_ms': relative['cpu_static_submit'],
        'static_gpu_ms': relative['gpu_static_done'] - relative['cpu_static_submit'],
        'between_cpu_submits_ms': relative['cpu_dynamic_submit'] - relative['cpu_static_submit'],
        'dynamic_gpu_ms': relative['gpu_dynamic_done'] - relative['cpu_dynamic_submit'],
        'frame_to_gpu_done_ms': relative['gpu_dynamic_done'],
    }
    return dict(absolute_ticks=points, relative_ms=relative, durations=durations,
                issue_window_note='CPU issue intervals include any producer wait; they are not pure active CPU execution time.')


def raw_rgb(path):
    words = struct.unpack('<102400H', path.read_bytes())
    image = Image.new('RGB', (400, 240))
    pixels = []
    for y in range(240):
        for x in range(400):
            word = words[(x // 16) * 4096 + (y // 16) * 256 + (y % 16) * 16 + x % 16]
            r, g, b = word >> 11, (word >> 5) & 63, word & 31
            pixels.append(((r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)))
    image.putdata(pixels)
    return image


def compare(a, b, out, name):
    assert a.size == b.size
    diff = ImageChops.difference(a, b)
    count = sum(any(pixel) for pixel in diff.getdata())
    a.save(out / f'{name}-before.png')
    b.save(out / f'{name}-after.png')
    diff.save(out / f'{name}-diff.png')
    ImageEnhance.Brightness(diff).enhance(8).save(out / f'{name}-diff-x8.png')
    w, h = a.size
    panel = Image.new('RGB', (w * 3, h + 24), '#202020')
    draw = ImageDraw.Draw(panel)
    for n, (label, image) in enumerate((('before', a), ('after', b), ('diff x8', ImageEnhance.Brightness(diff).enhance(8)))):
        draw.text((n*w+8, 5), label, fill='white')
        panel.paste(image, (n*w, 24))
    panel.save(out / f'{name}-comparison.png')
    return dict(changed_pixels=count, pixels=w*h, bounding_box=diff.getbbox(), maximum_channel_difference=max(diff.getextrema()[i][1] for i in range(3)))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('before', type=Path)
    parser.add_argument('after', type=Path)
    parser.add_argument('out', type=Path)
    parser.add_argument('--gallery', action='store_true')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    report = {}
    if args.gallery:
        for a in sorted(args.before.glob('*display.ppm')):
            b = args.after / a.name
            report[a.stem] = compare(Image.open(a).convert('RGB'), Image.open(b).convert('RGB'), args.out, a.stem)
        names=list(report)
        assert names, 'no gallery images'
        for start in range(0,len(names),16):
            subset=names[start:start+16]
            sheet=Image.new('RGB',(1200,((len(subset)+1)//2)*156),'#202020')
            draw=ImageDraw.Draw(sheet)
            for i,name in enumerate(subset):
                x=(i%2)*600;y=(i//2)*156
                draw.text((x+4,y+2),name,fill='white')
                panel=Image.open(args.out/f'{name}-comparison.png').resize((600,132))
                sheet.paste(panel,(x,y+20))
            sheet.save(args.out/f'contact-{start:02}.png')
    else:
        report['timing'] = {name: timing(folder) for name, folder in [('before', args.before), ('after', args.after)]}
        report['rgb'] = compare(raw_rgb(args.before/'frame2-color.bin'), raw_rgb(args.after/'frame2-color.bin'), args.out, 'frame2')
        a, b = [(folder/'frame2-depth.bin').read_bytes() for folder in (args.before, args.after)]
        report['depth_changed_words'] = sum(x != y for x, y in zip(struct.unpack('<102400H', a), struct.unpack('<102400H', b)))
    (args.out/'comparison.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report if not args.gallery else {'cases': len(report), 'changed_cases': {k:v for k,v in report.items() if v['changed_pixels']}}, indent=2))


if __name__ == '__main__':
    main()
