"""Evaluate opt-in NTREE2 outlines against the original packed-quad UV affine map.

Uses the source project's existing SUR decoder and old trim implementation read-only.
No runtime commands or matrix ABI fields are generated.
"""
import argparse
import base64
import copy
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import zipfile
import io
import sys

DIRECTIONS = ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1))


def atomic(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + '.tmp-outline')
    with temporary.open('xb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)
    assert path.read_bytes() == data
    if path.suffix in ('.json', '.py'):
        assert b'\0' not in data


def json_bytes(value):
    return (json.dumps(value, indent=2) + '\n').encode()


def cross(a, b, c):
    return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])


def area(poly):
    return abs(sum(a[0] * b[1] - a[1] * b[0] for a, b in zip(poly, poly[1:] + poly[:1]))) / 2


def ordered(poly):
    center = [sum(p[k] for p in poly) / len(poly) for k in range(2)]
    return sorted(poly, key=lambda p: math.atan2(p[1] - center[1], p[0] - center[0]))


def clip(poly, direction, bound):
    result = []
    nx, ny = direction
    for a, b in zip(poly, poly[1:] + poly[:1]):
        da = nx * a[0] + ny * a[1] - bound
        db = nx * b[0] + ny * b[1] - bound
        if da <= 1e-12:
            result.append(a)
        if da < 0 < db or db < 0 < da:
            t = da / (da - db)
            result.append([a[k] + (b[k] - a[k]) * t for k in range(2)])
    return result


def intersect(poly, boundary):
    for a, b in zip(boundary, boundary[1:] + boundary[:1]):
        # ordered() gives positive signed area.
        normal = (b[1] - a[1], a[0] - b[0])
        poly = clip(poly, normal, normal[0] * a[0] + normal[1] * a[1])
        if not poly:
            break
    return poly


def affine(points, values):
    a, b, c = points[:3]
    det = cross(a, b, c)
    assert det != 0, 'Original native quad must be nondegenerate'
    result = []
    for k in range(2):
        va, vb, vc = [v[k] for v in values[:3]]
        dx = ((vb - va) * (c[1] - a[1]) - (vc - va) * (b[1] - a[1])) / det
        dy = ((b[0] - a[0]) * (vc - va) - (c[0] - a[0]) * (vb - va)) / det
        result.append((dx, dy, va - dx * a[0] - dy * a[1]))
    return result


def evaluate(mapping, point):
    return [a * point[0] + b * point[1] + c for a, b, c in mapping]


def simplify(points):
    result = []
    for p in points:
        if not result or p != result[-1]:
            result.append(p)
    if len(result) > 1 and result[0] == result[-1]:
        result.pop()
    changed = True
    while changed and len(result) >= 3:
        changed = False
        for i in range(len(result)):
            if cross(result[i - 1], result[i], result[(i + 1) % len(result)]) == 0:
                result.pop(i)
                changed = True
                break
    return result


def billboard_shift(scene):
    return scene.get("billboard_position_shift", scene["position_shift"])


def packed_point(vertex, scene):
    return [round(vertex[3] * scene['scale'] * 512),
            round((vertex[1] - scene['origin'][1]) * scene['scale'] / (2 ** billboard_shift(scene) / 65536))]


def packed_uv(vertex):
    return [round(v * 65535) for v in vertex[4:6]]


def percentile(values, fraction):
    return sorted(values)[round((len(values) - 1) * fraction)]


def run(args):
    input_bytes = args.input.read_bytes()
    digest = hashlib.sha256(input_bytes).hexdigest()
    assert digest == '3d65ab214cb632eec1709d0e7f0638e29eab2aa4c9b74ad4d116e59a0d955064', 'Expected qualified compact source'
    with zipfile.ZipFile(io.BytesIO(input_bytes)) as archive:
        entries = [(info, archive.read(info)) for info in archive.infolist()]
    files = {info.filename: data for info, data in entries}
    original = json.loads(files['scene.json'])
    inventory = json.loads(files['inventory.json'])
    precision = getattr(args, 'precision', None)
    exact_anchor = getattr(args, 'exact_anchor', False)
    precision_requested = precision is not None or exact_anchor
    if precision_requested:
        original['billboard_position_shift'] = precision if precision is not None else original['position_shift']
        original['billboard_exact_anchor'] = exact_anchor
    provenance = next(t for t in inventory['textures'] if t['name'] == 'NTREE2')
    region, side = provenance['region'], provenance['size']
    ids = [i for i, g in enumerate(original['groups']) if g.get('source_texture') == 'NTREE2' and g.get('billboard')]
    assert len(ids) == 104
    partial_ids = [i for i in ids if len(original['groups'][i]['vertices']) != 4 or len(original['groups'][i]['triangles']) != 2]
    ids = [i for i in ids if i not in partial_ids]
    assert len(ids) == 100 and len(partial_ids) == 4
    node = r"""
import {readFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
import path from 'node:path';
const root=process.argv[1], decoded=process.argv[2];
const {decode}=await import(pathToFileURL(path.join(root,'tools/map_format.mjs')));
const {trimBillboards}=await import(pathToFileURL(path.join(root,'tools/billboard_trim.mjs')));
const {nativeTexture}=await import(pathToFileURL(path.join(root,'tools/platform_texture.mjs')));
let text='';for await (const part of process.stdin) text+=part;
const input=JSON.parse(text), image=decode('image',await readFile(decoded)).mips[0];
const texture={image,...input.provenance};
const stats=trimBillboards(input.scene,new Map([['NTREE2',texture]]));
console.log(JSON.stringify({image:{width:image.width,height:image.height,rgba:Buffer.from(image.rgba).toString('base64')},native:Buffer.from(nativeTexture(image,false).bytes).toString('base64'),old:input.scene.groups,stats}));
"""
    trimmed_scene = {k: v for k, v in original.items() if k not in ('groups', 'materials')}
    trimmed_scene['groups'] = [original['groups'][i] for i in ids]
    trimmed_scene['position_shift'] = billboard_shift(original)
    raw_path = args.decoded / 'NTREE2.trrgba'
    decoded = subprocess.run(['node', '--input-type=module', '-e', node, str(args.source_root), str(raw_path)],
                             input=json.dumps({'scene': trimmed_scene, 'provenance': provenance}),
                             text=True, capture_output=True, check=True)
    decoded = json.loads(decoded.stdout)
    width, height = decoded['image']['width'], decoded['image']['height']
    assert (width, height) == (64, 64)
    rgba = base64.b64decode(decoded['image']['rgba'])
    alpha = [(x, y) for y in range(height) for x in range(width) if rgba[(y * width + x) * 4 + 3]]
    assert alpha, 'Must evaluate real nonzero alpha support'
    def word(data, size, x, y):
        offset = ((x // 16 * (size // 16) + y // 16) * 256 + y % 16 * 16 + x % 16) * 2
        return struct.unpack_from('<H', data, offset)[0]
    native = base64.b64decode(decoded['native'])
    pixel_checks = 0
    for y in range(-2, height + 2):
        for x in range(-2, width + 2):
            assert word(native, width, max(0, min(width - 1, x)), max(0, min(height - 1, y))) == word(files[provenance['file']], side, region['x'] + x, region['y'] + y)
            pixel_checks += 1
    result = copy.deepcopy(original)
    rows = []
    coverage_checks = 0
    geometry_sizes = {}
    for index, old_trim in zip(ids, decoded['old']):
        group = original['groups'][index]
        assert len(group['vertices']) == 4 and len(group['triangles']) == 2
        positions = [packed_point(v, original) for v in group['vertices']]
        uvs = [packed_uv(v) for v in group['vertices']]
        mapping = affine(positions, uvs)
        inverse = affine(uvs, positions)
        assert max(abs(a - b) for p, uv in zip(positions, uvs) for a, b in zip(evaluate(mapping, p), uv)) < 1e-8
        source_uv = lambda uv: [(uv[0] / 65536 * side - region['x']) / width, (uv[1] / 65536 * side - region['y']) / height]
        source_map = lambda p: source_uv(evaluate(mapping, p))
        domain = ordered([source_uv(uv) for uv in uvs])
        # Half a geometry lattice step plus half an attribute LSB, in source texels.
        guard = [1 + 0.5 * sum(abs(c) for c in mapping[k][:2]) * side / 65536 + 0.5 * side / 65536 for k in range(2)]
        support = []
        for nx, ny in DIRECTIONS:
            support.append(max(nx * ((x + 1 + guard[0]) if nx >= 0 else (x - guard[0])) / width + ny * ((y + 1 + guard[1]) if ny >= 0 else (y - guard[1])) / height for x, y in alpha))
        polygon = domain
        for direction, bound in zip(DIRECTIONS, support):
            polygon = clip(polygon, direction, bound)
        points = simplify([[round(v) for v in evaluate(inverse, [(p[0] * width + region['x']) / side * 65536, (p[1] * height + region['y']) / side * 65536])] for p in polygon])
        # Keep original triangle winding, although native billboard meshes disable culling.
        orientation = cross(*[positions[i] for i in group['triangles'][0]])
        if len(points) >= 3 and cross(*points[:3]) * orientation < 0:
            points.reverse()
        fallback = len(points) < 3 or len(points) > 8 or any(cross(points[i-1], points[i], points[(i+1) % len(points)]) * orientation <= 0 for i in range(len(points)))
        triangles = [[0, i, i + 1] for i in range(1, len(points) - 1)]
        fallback |= any(cross(*[points[i] for i in tri]) == 0 for tri in triangles)
        if fallback:
            points, triangles = positions, group['triangles']
        next_uvs = [[round(v) for v in evaluate(mapping, p)] for p in points]
        assert all(0 <= v <= 65535 for uv in next_uvs for v in uv)
        boundary = ordered([source_map(p) for p in points])
        uv_error = max(abs(a - b) * side / 65536 for p, uv in zip(points, next_uvs) for a, b in zip(evaluate(mapping, p), uv))
        old_error = max(abs(a - b) * side / 65536 for vertex in old_trim['vertices'] for a, b in zip(evaluate(mapping, packed_point(vertex, original)), packed_uv(vertex)))
        components = {'new_geometry_round': 0, 'original_quad_snap_map': 0, 'uv_round': 0}
        for vertex in old_trim['vertices']:
            floating = [vertex[3] * original['scale'] * 512,
                        (vertex[1] - original['origin'][1]) * original['scale'] / (2 ** billboard_shift(original) / 65536)]
            expected_float = evaluate(mapping, floating)
            expected_packed = evaluate(mapping, packed_point(vertex, original))
            ideal_uv = [v * 65535 for v in vertex[4:6]]
            actual_uv = packed_uv(vertex)
            for k in range(2):
                for key, delta in [('new_geometry_round', expected_packed[k] - expected_float[k]),
                                   ('original_quad_snap_map', expected_float[k] - ideal_uv[k]),
                                   ('uv_round', ideal_uv[k] - actual_uv[k])]:
                    components[key] = max(components[key], abs(delta) * side / 65536)
                assert abs((expected_packed[k] - actual_uv[k]) - ((expected_packed[k] - expected_float[k]) + (expected_float[k] - ideal_uv[k]) + (ideal_uv[k] - actual_uv[k]))) < 1e-8
        # Test only the alpha support that the original native quad could cover.
        # Expand by the measured UV-rounding residual before testing geometric containment.
        margin = 1 + uv_error
        for x, y in alpha:
            cell = [[(x-margin)/width, (y-margin)/height], [(x+1+margin)/width, (y-margin)/height], [(x+1+margin)/width, (y+1+margin)/height], [(x-margin)/width, (y+1+margin)/height]]
            for p in intersect(cell, domain):
                assert all(cross(a, b, p) >= -1e-10 for a, b in zip(boundary, boundary[1:] + boundary[:1])), f'Alpha support clipped at group {index}'
                coverage_checks += 1
        vertices = []
        for p, uv in zip(points, next_uvs):
            vertices.append([group['vertices'][0][0], p[1] * (2 ** billboard_shift(original) / 65536) / original['scale'] + original['origin'][1], group['vertices'][0][2], p[0] / (original['scale'] * 512), uv[0] / 65535, uv[1] / 65535])
        replacement = result['groups'][index]
        if not fallback:
            replacement['bounds_geometry'] = copy.deepcopy(group.get('bounds_geometry', {'vertices': group['vertices'], 'triangles': group['triangles']}))
            replacement['vertices'], replacement['triangles'] = vertices, triangles
        assert [packed_point(v, original) for v in replacement['vertices']] == points
        assert [packed_uv(v) for v in replacement['vertices']] == next_uvs
        assert replacement['name'] == group['name'] and replacement['material'] == group['material']
        size_key = (max(v[3] for v in group['vertices'])-min(v[3] for v in group['vertices']), max(v[1] for v in group['vertices'])-min(v[1] for v in group['vertices']))
        geometry_sizes[str(size_key)] = geometry_sizes.get(str(size_key), 0) + 1
        rows.append(dict(
            source_group_ordinal=index,
            name=group['name'],
            old_uv_error_source_texels=old_error,
            corrected_uv_error_source_texels=uv_error,
            old_trim_vertices=len(old_trim['vertices']),
            vertices=len(points),
            triangles=len(triangles),
            fallback_quad=fallback,
            native_geometry_area_ratio=area(points)/area(ordered(positions)),
            native_y_extent_grid=max(p[1] for p in positions)-min(p[1] for p in positions),
            source_uv_domain=domain,
            guard_source_texels=guard,
            old_error_component_max_source_texels=components,
        ))
    changed = set(ids)
    assert all(a == b for i, (a, b) in enumerate(zip(original['groups'], result['groups'])) if i not in changed)
    assert original['materials'] == result['materials']
    metadata = copy.deepcopy(inventory)
    metadata['outline_candidate'] = {'format': 'tankracer.native-affine-outline.v1', 'source_texture': 'NTREE2', 'input_sha256': digest, 'bounds_and_sort_units_preserved': True}
    def build_zip(scene=result, info_metadata=metadata):
        out = io.BytesIO()
        with zipfile.ZipFile(out, 'w') as z:
            for info, data in entries:
                if info.filename == 'scene.json':
                    data = (json.dumps(scene, separators=(',', ':')) + '\n').encode()
                elif info.filename == 'inventory.json':
                    data = json_bytes(info_metadata)
                z.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
        return out.getvalue()
    control = build_zip(original, inventory) if precision_requested else input_bytes
    candidate = build_zip()
    assert candidate == build_zip(), 'Repeat ZIP must be deterministic'
    old_scene = copy.deepcopy(original)
    for index, old in zip(ids, decoded['old']):
        old_scene['groups'][index] = old
    old_metadata = copy.deepcopy(inventory)
    old_metadata['outline_candidate'] = {'format': 'tankracer.legacy-outline-control.v1', 'source_texture': 'NTREE2', 'input_sha256': digest}
    old_candidate = build_zip(old_scene, old_metadata)
    # Exercise the actual packer and inspect the exact native vertex/UV fields.
    app_tools = Path(__file__).resolve().parent
    sys.path.insert(0, str(app_tools.parents[2] / 'tools'))
    sys.path.insert(0, str(app_tools))
    from static_assets import read_geometry
    payload_before = payload_after = 0
    for index in ids:
        original_group, next_group = original['groups'][index], result['groups'][index]
        before, bounds, _ = read_geometry(original_group, True, True, original['origin'], original['scale'], billboard_shift(original), exact_anchor)
        after, _, _ = read_geometry(next_group, True, True, original['origin'], original['scale'], billboard_shift(original), exact_anchor)
        _, bounds_next, _ = read_geometry(next_group.get('bounds_geometry', next_group), True, True, original['origin'], original['scale'], billboard_shift(original), exact_anchor)
        assert bounds == bounds_next, 'Transparent sort and culling bounds must stay exact'
        assert len(before) == len(after) == 1
        payload_before += len(before[0])
        payload_after += len(after[0])
        assert len(before[0]) == len(after[0]) == 256, 'One metadata sector plus one payload sector'
        record = after[0]
        origin_y = struct.unpack_from('<i', record, 20)[0] / 2 ** billboard_shift(original)
        assert origin_y == round(origin_y)
        for i, vertex in enumerate(next_group['vertices']):
            xyz, side_bits, uv = struct.unpack_from('<III', record, 128+i*12)
            side_int = side_bits if side_bits < 512 else side_bits - 1024
            assert [side_int, int(origin_y) + ((xyz >> 10) & 1023)] == packed_point(vertex, original)
            assert [uv & 65535, uv >> 16] == packed_uv(vertex)
    precision_rows = []
    for index, group in enumerate(original['groups']):
        if not group.get('billboard'):
            continue
        encoded, _, _ = read_geometry(group, True, True, original['origin'], original['scale'],
                                      billboard_shift(original), exact_anchor)
        assert len(encoded) == 1
        record = encoded[0]
        anchor_fixed = struct.unpack_from('<3i', record, 16)
        shift = struct.unpack_from('<I', record, 28)[0]
        assert shift == billboard_shift(original)
        errors = [0, 0, 0]
        max_delta = 0
        for i, vertex in enumerate(group['vertices']):
            xyz = struct.unpack_from('<I', record, 128 + i * 12)[0]
            delta = [(xyz >> (axis * 10)) & 1023 for axis in range(3)]
            if exact_anchor:
                assert delta[0] == delta[2] == 0
            max_delta = max(max_delta, *delta)
            for axis in range(3):
                reconstructed = (anchor_fixed[axis] + delta[axis] * 2**shift) / 65536
                world = reconstructed / original['scale'] + original['origin'][axis]
                errors[axis] = max(errors[axis], abs(world - vertex[axis]))
        precision_rows.append(dict(source_group_ordinal=index, vertices=len(group['vertices']),
                                   record_bytes=len(record), max_delta=max_delta,
                                   anchor_xz_error_world=max(errors[0], errors[2]), y_error_world=errors[1]))
    assert len(precision_rows) == 280
    # A real map camera targets NTREE2 source alpha on the center billboard column.
    group = original['groups'][ids[0]]
    positions = [packed_point(v, original) for v in group['vertices']]
    uvs = [packed_uv(v) for v in group['vertices']]
    mapping = affine(positions, uvs)
    center_y = sum(p[1] for p in positions) / 4
    focus_y = None
    for y in sorted(range(min(p[1] for p in positions), max(p[1] for p in positions)+1), key=lambda y: abs(y-center_y)):
        uv = evaluate(mapping, [0, y])
        xtex = uv[0] / 65536 * side - region['x']
        ytex = uv[1] / 65536 * side - region['y']
        x, yy = max(0,min(width-1,round(xtex))),max(0,min(height-1,round(ytex)))
        if rgba[(yy*width+x)*4+3] >= 128:
            focus_y = y
            break
    assert focus_y is not None
    target = [group['vertices'][0][0], focus_y * (2**billboard_shift(original)/65536)/original['scale']+original['origin'][1], group['vertices'][0][2]]
    cameras = [dict(
        eye=[target[0],target[1],target[2]-distance],
        target=target,
        label=f'ntree2-native-alpha-{distance}world-group-{ids[0]}',
    ) for distance in (4,6,8)]
    report = dict(
        format='tankracer.native-affine-outline-evaluation.v1',
        input_sha256=digest,
        candidate_sha256=hashlib.sha256(candidate).hexdigest(),
        control_sha256=hashlib.sha256(control).hexdigest(),
        billboard_precision=dict(position_shift=billboard_shift(original), exact_anchor=exact_anchor),
        billboard_groups_checked=len(precision_rows),
        billboard_max_delta=max(r['max_delta'] for r in precision_rows),
        billboard_anchor_xz_error_max_world=max(r['anchor_xz_error_world'] for r in precision_rows),
        billboard_y_error_max_world=max(r['y_error_world'] for r in precision_rows),
        billboard_record_bytes=sum(r['record_bytes'] for r in precision_rows),
        billboard_precision_rows=precision_rows,
        legacy_control_sha256=hashlib.sha256(old_candidate).hexdigest(),
        native_record_bytes_before=payload_before,
        native_record_bytes_after=payload_after,
        old_error_component_max_source_texels={k:max(r['old_error_component_max_source_texels'][k] for r in rows) for k in ('new_geometry_round','original_quad_snap_map','uv_round')},
        decoded_ntree2_sha256=hashlib.sha256(raw_path.read_bytes()).hexdigest(),
        source_image_size=[width,height],
        ntree2_sort_groups=104,
        untouched_partial_groups=partial_ids,
        source_nonzero_alpha_texels=len(alpha),
        card_source_geometry_sizes_world=geometry_sizes,
        cards=len(rows),
        fallback_quads=sum(r['fallback_quad'] for r in rows),
        triangles_before=2*len(rows),
        triangles_after=sum(r['triangles'] for r in rows),
        old_uv_error_max_source_texels=max(r['old_uv_error_source_texels'] for r in rows),
        old_uv_error_median_source_texels=percentile([r['old_uv_error_source_texels'] for r in rows],.5),
        corrected_uv_error_max_source_texels=max(r['corrected_uv_error_source_texels'] for r in rows),
        area_ratio_median=percentile([r['native_geometry_area_ratio'] for r in rows],.5),
        source_native_pixel_and_guard_checks=pixel_checks,
        alpha_support_corner_checks=coverage_checks,
        alpha_support_scope='Every nonzero source alpha texel expanded by one texel plus measured UV residual, intersected with original native quad UV domain; contained by final native geometry under original affine mapping.',
        stress_source_group=ids[0],
        stress_focus_source_texel=[xtex,ytex],
        stress_focus_alpha=rgba[(yy*width+x)*4+3],
        cameras=cameras,
        geometry_quantization={'y_world':2**billboard_shift(original)/65536/original['scale'],'side_world':1/(512*original['scale'])},
        textures_materials_other_groups_unchanged=True,
        native_uv_error_scope='Maximum per-vertex affine error bounds ideal triangle interpolation; fixed-point raster interpolation and rendered RGB attribution require GPU comparison.',
        deterministic=True,
        rows=rows,
    )
    args.output.mkdir(parents=True, exist_ok=False)
    atomic(args.output/'scene.zip',candidate)
    atomic(args.output/'scene-control.zip',control)
    atomic(args.output/'scene-old-trim.zip',old_candidate)
    atomic(args.output/'cameras-stress.json',json_bytes(cameras))
    atomic(args.output/'result.json',json_bytes(report))
    print(json.dumps({k:v for k,v in report.items() if k != 'rows'},indent=2))


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('source-root','decoded','input','output'):
        parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--precision', '--billboard-position-shift', type=int, choices=range(32))
    parser.add_argument('--billboard-exact-anchor', dest='exact_anchor', action='store_true')
    run(parser.parse_args())
