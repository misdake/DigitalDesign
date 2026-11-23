"""Compare complete functional frame outputs, with an explicit near-depth gate."""
import argparse
import json
import math
from pathlib import Path
import re
import struct

from PIL import Image


def frames(folder):
    result = {}
    for path in folder.glob('frame-*.json'):
        if not re.fullmatch(r'frame-\d{3}\.json', path.name):
            continue
        value = json.loads(path.read_text())
        if value['format'] not in ('tankracer.gpu-input.v1', 'tankracer.frame-output.v1'):
            raise ValueError(f'unsupported frame format: {path}')
        frame = value['frame']
        if frame in result or not 1 <= frame <= 192:
            raise ValueError(f'invalid or duplicate frame: {path}')
        result[frame] = value
    if not result:
        raise ValueError(f'no frame metadata in {folder}')
    return result


def pixels(folder, frame):
    with Image.open(folder / f'frame-{frame:03}.ppm') as image:
        if image.size != (400, 240) or image.mode != 'RGB':
            raise ValueError('expected a 400x240 RGB output')
        rgb = image.tobytes()
    depth = (folder / f'frame-{frame:03}.d16').read_bytes()
    if len(depth) != 400 * 240 * 2:
        raise ValueError('expected a 400x240 little-endian D16 plane')
    return rgb, struct.unpack('<96000H', depth)


def compare(before, after, near, color_budget=0, depth_budget=0, near_mode='baseline'):
    if near_mode not in ('baseline', 'union'):
        raise ValueError('near mode must be baseline or union')
    left, right = frames(before), frames(after)
    if left.keys() != right.keys():
        raise ValueError('frame sets differ')
    rows = []
    for frame in sorted(left):
        rgb0, z0 = pixels(before, frame)
        rgb1, z1 = pixels(after, frame)
        row = dict(frame=frame, rgb_changed=0, depth_changed=0,
                   near_pixels=0, near_rgb_changed=0, near_depth_changed=0, maximum_channel_delta=0,
                   triangles_before=left[frame]['triangles'], triangles_after=right[frame]['triangles'],
                   meshlets_before=left[frame]['meshlets'], meshlets_after=right[frame]['meshlets'])
        for pixel, (a, b) in enumerate(zip(z0, z1)):
            at = pixel * 3
            delta = max(abs(rgb0[at + i] - rgb1[at + i]) for i in range(3))
            row['rgb_changed'] += delta != 0
            row['depth_changed'] += a != b
            tested_depth = min(a, b) if near_mode == 'union' else a
            if tested_depth < near * 256:  # Native D16 retains eight fractional render-unit bits.
                row['near_pixels'] += 1
                row['near_rgb_changed'] += delta != 0
                row['near_depth_changed'] += a != b
            row['maximum_channel_delta'] = max(row['maximum_channel_delta'], delta)
        rows.append(row)
    return dict(format='tankracer.frame-comparison.v1', frames=len(rows), near_render_units=near, near_selection=near_mode,
                near_rgb_equal=all(r['near_rgb_changed'] == 0 for r in rows),
                near_depth_equal=all(r['near_depth_changed'] == 0 for r in rows),
                near_color_budget_per_frame=color_budget, near_depth_budget_per_frame=depth_budget,
                near_gate_passed=all(r['near_rgb_changed'] <= color_budget
                                     and r['near_depth_changed'] <= depth_budget for r in rows),
                scope=('Near pixels use the union of both opaque D16 planes. ' if near_mode == 'union'
                       else 'Near pixels use baseline opaque D16. ') +
                      'Transparent foreground over a far background '
                      'needs separate visual review. Far differences remain explicit per frame.',
                rows=rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('before', type=Path)
    parser.add_argument('after', type=Path)
    parser.add_argument('--near', type=float, required=True, help='Positive render-unit depth')
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--near-mode', choices=('baseline', 'union'), default='baseline',
                        help='Use both depth planes to include newly appearing near geometry')
    parser.add_argument('--near-color-budget', type=int, default=0, help='Allowed changed near RGB pixels per frame')
    parser.add_argument('--near-depth-budget', type=int, default=0, help='Allowed changed near D16 pixels per frame')
    args = parser.parse_args()
    if not math.isfinite(args.near) or not 0 < args.near < 256:
        parser.error('near depth must be finite and within D16 range')
    if args.out.exists():
        parser.error('a fresh output file is required')
    if not 0 <= args.near_color_budget <= 96000 or not 0 <= args.near_depth_budget <= 96000:
        parser.error('near budgets must be within the pixel count')
    result = compare(args.before, args.after, args.near, args.near_color_budget, args.near_depth_budget, args.near_mode)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key: value for key, value in result.items() if key != 'rows'}))
    return 0 if result['near_gate_passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
