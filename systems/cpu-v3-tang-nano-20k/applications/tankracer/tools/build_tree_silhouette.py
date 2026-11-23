"""Build guarded tree-crown/trunk geometry from original decoded NTREE2 alpha.

Retains native quad UV mapping, original bounds, material and sort identity.
The eight shared vertices occupy the same native record as the convex outline.
"""
import argparse
import base64
import copy
import hashlib
import io
import json
from pathlib import Path
import struct
import subprocess
import sys
import zipfile

from evaluate_tree_outline import (
    affine, area, atomic, billboard_shift, cross, evaluate, json_bytes,
    packed_point, packed_uv,
)


def triangulate(points):
    """Clip deterministic ears; all six triangles share their actual vertices."""
    order = list(range(len(points)))
    orientation = sum(a[0] * b[1] - a[1] * b[0]
                      for a, b in zip(points, points[1:] + points[:1]))
    triangles = []
    while len(order) > 3:
        for j, b in enumerate(order):
            a, c = order[j - 1], order[(j + 1) % len(order)]
            if cross(points[a], points[b], points[c]) * orientation <= 0:
                continue
            blocked = False
            for k in order:
                if k in (a, b, c):
                    continue
                if all(cross(points[x], points[y], points[k]) * orientation >= 0
                       for x, y in ((a, b), (b, c), (c, a))):
                    blocked = True
                    break
            if blocked:
                continue
            triangles.append([a, b, c])
            order.pop(j)
            break
        else:
            raise ValueError('Tree silhouette has no nondegenerate ear')
    triangles.append(order)
    if len(triangles) != len(points) - 2 or abs(
        sum(abs(cross(*[points[k] for k in t])) / 2 for t in triangles) - area(points)
    ) > 1e-7:
        raise ValueError('Tree silhouette triangulation does not cover its polygon')
    return triangles


def fit_silhouette(positions, uvs, alpha, side, region):
    mapping = affine(positions, uvs)
    inverse = affine(uvs, positions)
    if abs(mapping[0][1]) + abs(mapping[1][0]) > 1e-9:
        raise ValueError('Tree silhouette requires the source axis-aligned quad UV map')

    def source_map(point):
        return [v / 65536 * side - region[k]
                for v, k in zip(evaluate(mapping, point), ('x', 'y'))]

    domain = [source_map(p) for p in positions]
    lo = [min(p[k] for p in domain) for k in range(2)]
    hi = [max(p[k] for p in domain) for k in range(2)]
    guard = [1 + 0.5 * sum(abs(c) for c in mapping[k][:2]) * side / 65536
             + 0.5 * side / 65536 for k in range(2)]
    best = None
    for split in range(4, 60):
        upper = [(x, y) for x, y in alpha if y - guard[1] < split]
        lower = [(x, y) for x, y in alpha if y + 1 + guard[1] > split]
        if not upper or not lower:
            continue

        def limits(pixels):
            return [max(lo[0], min(x - guard[0] for x, y in pixels)),
                    min(hi[0], max(x + 1 + guard[0] for x, y in pixels))]

        left, right = limits(upper)
        inner_left, inner_right = limits(lower)
        top = max(lo[1], min(y - guard[1] for x, y in alpha))
        bottom = min(hi[1], max(y + 1 + guard[1] for x, y in alpha))
        if not left < inner_left < inner_right < right or not top < split < bottom:
            continue
        polygon = [[left, top], [right, top], [right, split], [inner_right, split],
                   [inner_right, bottom], [inner_left, bottom],
                   [inner_left, split], [left, split]]
        points = [[round(v) for v in evaluate(inverse, [
            (p[0] + region['x']) / side * 65536,
            (p[1] + region['y']) / side * 65536,
        ])] for p in polygon]
        actual = [source_map(p) for p in points]
        attributes = [[round(v) for v in evaluate(mapping, p)] for p in points]
        error = max(abs(a - b) * side / 65536
                    for p, uv in zip(points, attributes)
                    for a, b in zip(evaluate(mapping, p), uv))
        split_y = actual[2][1]
        boxes = [[actual[0][0], actual[1][0], actual[0][1], split_y],
                 [actual[5][0], actual[4][0], split_y, actual[4][1]]]
        margin = 1 + error
        good, checks = True, 0
        for x, y in alpha:
            x0, x1 = max(lo[0], x - margin), min(hi[0], x + 1 + margin)
            y0, y1 = max(lo[1], y - margin), min(hi[1], y + 1 + margin)
            if x0 >= x1 or y0 >= y1:
                continue
            # Check both portions of every bilinear support rectangle, including
            # rectangles spanning the two concave crown/trunk corners.
            for l, r, t, b in boxes:
                if max(y0, t) < min(y1, b) and (x0 < l - 1e-8 or x1 > r + 1e-8):
                    good = False
            if y0 < boxes[0][2] - 1e-8 or y1 > boxes[1][3] + 1e-8:
                good = False
            if not good:
                break
            checks += 4
        if good and (best is None or area(points) < area(best['points'])):
            best = dict(points=points, attributes=attributes, mapping=mapping,
                        uv_error_source_texels=error, support_checks=checks,
                        split_source_y=split, guard_source_texels=guard)
    if best is None:
        raise ValueError('Original alpha cannot fit the guarded eight-vertex silhouette')
    return best


def decode_source(source_root, decoded):
    script = """
import {readFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
import path from 'node:path';
const [root,file]=process.argv.slice(1);
const {decode}=await import(pathToFileURL(path.join(root,'tools/map_format.mjs')));
const {nativeTexture}=await import(pathToFileURL(path.join(root,'tools/platform_texture.mjs')));
const image=decode('image',await readFile(file)).mips[0];
console.log(JSON.stringify({width:image.width,height:image.height,
  rgba:Buffer.from(image.rgba).toString('base64'),
  native:Buffer.from(nativeTexture(image,false).bytes).toString('base64')}));
"""
    result = subprocess.run(['node', '--input-type=module', '-e', script,
                             str(source_root), str(decoded)],
                            text=True, capture_output=True, check=True)
    image = json.loads(result.stdout)
    if (image['width'], image['height']) != (64, 64):
        raise ValueError('Expected original 64x64 NTREE2 image')
    return base64.b64decode(image['rgba']), base64.b64decode(image['native'])


def run(args):
    data = args.input.read_bytes()
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        entries = [(i, z.read(i)) for i in z.infolist()]
    files = {i.filename: b for i, b in entries}
    scene = json.loads(files['scene.json'])
    if scene.get('billboard_exact_anchor') is not True:
        raise ValueError('Tree silhouettes require exact native billboard anchors')
    result = copy.deepcopy(scene)
    inventory = json.loads(files['inventory.json'])
    texture = next(t for t in inventory['textures'] if t['name'] == 'NTREE2')
    decoded = args.decoded / 'NTREE2.trrgba'
    rgba, native = decode_source(args.source_root, decoded)
    alpha = [(x, y) for y in range(64) for x in range(64) if rgba[4 * (64 * y + x) + 3]]
    side, region = texture['size'], texture['region']

    def word(payload, size, x, y):
        offset = ((x // 16 * (size // 16) + y // 16) * 256 + y % 16 * 16 + x % 16) * 2
        return struct.unpack_from('<H', payload, offset)[0]

    for y in range(-2, 66):
        for x in range(-2, 66):
            if word(native, 64, max(0, min(63, x)), max(0, min(63, y))) != word(
                files[texture['file']], side, region['x'] + x, region['y'] + y
            ):
                raise ValueError('Original NTREE2 quantization or atlas guard differs')

    sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools'))
    from static_assets import read_geometry
    rows = []
    for index, group in enumerate(scene['groups']):
        if group.get('source_texture') != 'NTREE2' or not group.get('billboard'):
            continue
        quad = group.get('bounds_geometry')
        if quad is None:
            continue
        if len(quad['vertices']) != 4 or len(quad['triangles']) != 2:
            raise ValueError('Expected retained full source quad')
        positions = [packed_point(v, scene) for v in quad['vertices']]
        fitted = fit_silhouette(positions, [packed_uv(v) for v in quad['vertices']], alpha, side, region)
        points, attributes = fitted['points'], fitted['attributes']
        triangles = triangulate(points)
        orientation = cross(*[positions[i] for i in quad['triangles'][0]])
        if cross(*[points[k] for k in triangles[0]]) * orientation < 0:
            triangles = [list(reversed(t)) for t in triangles]
        vertices = []
        for p, uv in zip(points, attributes):
            vertices.append([
                group['vertices'][0][0],
                p[1] * (2 ** billboard_shift(scene) / 65536) / scene['scale'] + scene['origin'][1],
                group['vertices'][0][2], p[0] / scene['scale'] / 512,
                *[v / 65535 for v in uv],
            ])
        replacement = result['groups'][index]
        replacement['vertices'], replacement['triangles'] = vertices, triangles
        params = (True, True, scene['origin'], scene['scale'], billboard_shift(scene), True)
        before, _, _ = read_geometry(group, *params)
        after, _, _ = read_geometry(replacement, *params)
        if len(before) != 1 or len(after) != 1 or len(before[0]) != 256 or len(after[0]) != 256:
            raise ValueError('Tree silhouette must retain one 256-byte native record')
        if [packed_point(v, scene) for v in vertices] != points:
            raise ValueError('Tree vertex reconstruction differs from the native lattice')
        rows.append(dict(source_group_ordinal=index, vertices=8, triangles=6, record_bytes=256,
                         old_triangles=len(group['triangles']),
                         old_area=area([packed_point(v, scene) for v in group['vertices']]),
                         new_area=area(points), **{k: fitted[k] for k in (
                             'uv_error_source_texels', 'support_checks', 'split_source_y', 'guard_source_texels')}))
    if len(rows) != 100:
        raise ValueError('Expected 100 complete NTREE2 source quads')
    changed = {r['source_group_ordinal'] for r in rows}
    if any(a != b for i, (a, b) in enumerate(zip(scene['groups'], result['groups'])) if i not in changed):
        raise ValueError('An unrelated scene group changed')
    inventory['outline_candidate'] = dict(
        format='tankracer.native-affine-silhouette.v1', source_texture='NTREE2',
        input_sha256=hashlib.sha256(data).hexdigest(), bounds_and_sort_units_preserved=True,
    )

    def pack():
        output = io.BytesIO()
        with zipfile.ZipFile(output, 'w') as z:
            for info, payload in entries:
                if info.filename == 'scene.json':
                    payload = (json.dumps(result, separators=(',', ':')) + '\n').encode()
                elif info.filename == 'inventory.json':
                    payload = json_bytes(inventory)
                z.writestr(info, payload, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
        return output.getvalue()

    payload = pack()
    if pack() != payload:
        raise ValueError('Tree silhouette ZIP is not deterministic')
    atomic(args.output / 'scene-compact.zip', payload)
    report = dict(format='tankracer.tree-silhouette.v1', cards=len(rows), rows=rows,
                  input_sha256=hashlib.sha256(data).hexdigest(), output_sha256=hashlib.sha256(payload).hexdigest(),
                  source_decoded_sha256=hashlib.sha256(decoded.read_bytes()).hexdigest(),
                  source_rgba_sha256=hashlib.sha256(rgba).hexdigest(), native_bytes_delta=0,
                  triangles_delta=sum(r['triangles'] - r['old_triangles'] for r in rows),
                  support_checks=sum(r['support_checks'] for r in rows),
                  area_ratio_to_input=sum(r['new_area'] for r in rows) / sum(r['old_area'] for r in rows))
    atomic(args.output / 'silhouette.json', json_bytes(report))
    print(json.dumps({k: v for k, v in report.items() if k != 'rows'}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--source-root', type=Path, required=True)
    parser.add_argument('--decoded', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    run(parser.parse_args())
