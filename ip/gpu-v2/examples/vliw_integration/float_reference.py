"""Independent float64 ray/triangle renderer and interior-pixel RTL audit.

Consumes V96 inputs, not the fixed-point geometry/interpolation oracle. Hardware
trace data is used only for comparison and identifying the final visible owner.
"""
import argparse
import json
import math
from pathlib import Path

import numpy as np
from PIL import Image


def signed(value, bits):
    value &= (1 << bits)-1
    return value-(1 << bits) if value >> (bits-1) else value


def normalize(v):
    return v / np.maximum(np.linalg.norm(v, axis=-1, keepdims=True), 1e-30)


def shade(normal, x, y, shading, unlit=False):
    if unlit:
        return np.full(np.shape(x), 256.), np.zeros(np.shape(x))
    n = normalize(normal)
    light = np.array(shading['light']) / 16384
    # Pixel-center NDC. Match the public projection contract, not RTL arithmetic.
    ndc = np.stack(((2*x-399)/400, (239-2*y)/240), axis=-1)
    view = normalize(np.concatenate((ndc*np.array(shading['ray_scale'])/16384,
                                    np.full((*np.shape(x), 1), shading['ray_k']/16384)), axis=-1))
    half = normalize(light + view)
    nl = np.sum(n*light, axis=-1)
    nh = np.maximum(0, np.minimum(1, np.sum(n*half, axis=-1)))
    power = (4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64)[shading['shininess']]
    return (shading['ambient']+shading['directional']*np.maximum(0, nl),
            shading['directional']*nh**power*(nl > 0))


def filtered_texture(uv, lod):
    """Floating bilinear/trilinear filtering, independent of integer tap weights."""
    source = Image.open(Path(__file__).with_name('assets')/'room-tiles.png').convert('RGB')
    result = np.zeros((len(uv), 3))
    lower = np.floor(lod).astype(int)
    for n in range(7):
        side = 1 << n
        tex = np.asarray(source.resize((side, side), Image.Resampling.BOX), dtype=float)
        q = np.rint(tex*np.array([31, 63, 31])/255).astype(int)
        tex = np.stack(((q[..., 0] << 3) | (q[..., 0] >> 2),
                        (q[..., 1] << 2) | (q[..., 1] >> 4),
                        (q[..., 2] << 3) | (q[..., 2] >> 2)), axis=-1)
        # A1 uses the same two coordinate phases as the documented sampler.
        fine_side = np.maximum(2, 64 >> lower)
        fine = uv*fine_side[:, None]-.5
        xy = np.where((n == 6-lower)[:, None], fine,
                      np.where((fine_side > 2)[:, None], (fine-.5)/2, fine))
        base = np.floor(xy).astype(int)
        frac = xy-base
        weight = np.where(n == 6-lower, 1-(lod-lower), np.where(n == np.maximum(0, 5-lower), lod-lower, 0))
        weight[lod == 6] = 1 if n == 0 else 0
        for dx, dy in ((0, 0), (1, 0), (0, 1), (1, 1)):
            w = weight*(frac[:, 0] if dx else 1-frac[:, 0])*(frac[:, 1] if dy else 1-frac[:, 1])
            result += w[:, None]*tex[(base[:, 1]+dy) % side, (base[:, 0]+dx) % side]
    return result


def render(data):
    yy, xx = np.mgrid[:240, :400]
    rays = np.stack(((2*xx-399)/512, (2*yy-239)/512, np.ones_like(xx)), axis=-1)
    z = np.full((240, 400), np.inf)
    owner = np.full((240, 400), -1, dtype=int)
    normal = np.zeros((240, 400, 3))
    attr = np.zeros((240, 400, 3))
    margin = np.zeros((240, 400))
    unlit = np.zeros((240, 400), dtype=bool)
    textured = np.zeros((240, 400), dtype=bool)
    inverses = {}
    triangle_id = 0
    for instance in data['instances']:
        config, uniform = instance['config'], instance['uniform']
        h_matrix = np.array(uniform[:12]).reshape(3, 4)/65536
        n_matrix = np.array(uniform[12:21]).reshape(3, 3)/65536
        positions, normals, attrs = [], [], []
        for p, n, a in instance['vertices']:
            world = np.array([config[k]+((p >> (10*k)) & 1023)*(1 << (config[3] & 31))
                              for k in range(3)])/65536
            positions.append(h_matrix @ np.append(world, 1))
            normals.append(n_matrix @ np.array([signed(n >> (10*k), 10)/511 for k in range(3)]))
            attrs.append([(a >> (8*k)) & 255 for k in range(3)] if instance['color_mode']
                         else [(a & 65535)/65536, (a >> 16)/65536, 0])
        positions, normals, attrs = map(np.array, (positions, normals, attrs))
        for refs in instance['triangles']:
            pid = triangle_id
            triangle_id += 1
            matrix = positions[refs].T
            det = np.linalg.det(matrix)
            if abs(det) < 1e-20 or instance.get('cull_negative', 1) and det < 0:
                continue
            inverse = np.linalg.inv(matrix)
            inverses[pid] = inverse
            coefficients = rays @ inverse.T
            denominator = np.sum(coefficients, axis=-1)
            inside = (np.min(coefficients, axis=-1) >= 0) & (denominator > 0)
            depth = 1/np.maximum(denominator, 1e-30)
            take = inside & (depth < z) & (depth < instance.get('cfg_far_z', 199*65536)/65536)
            if not np.any(take):
                continue
            weights = coefficients[take]/denominator[take, None]
            normal[take] = weights @ normals[refs]
            value = weights @ attrs[refs]
            if instance['color_mode']:
                attr[take] = value
            else:
                gradient = np.einsum('pic,ia->pca', attrs[refs, :2][None, :, :]-value[:, None, :2],
                                     inverse[:, :2]*2/512)/denominator[take, None, None]
                rho = np.max(abs(gradient), axis=(1, 2))*64*math.sqrt(2)
                lod = np.clip(np.log2(np.maximum(rho, 1e-30)), 0, 6)
                attr[take] = filtered_texture(value[:, :2], lod)
            z[take], owner[take] = depth[take], pid
            edge_distance = coefficients[take] / np.maximum(np.linalg.norm(inverse[:, :2]*2/512, axis=1), 1e-30)
            margin[take] = np.min(edge_distance, axis=-1)
            unlit[take] = instance.get('unlit', False)
            textured[take] = not instance['color_mode']
    shading = data['info']['camera']['shading']
    g, h = shade(normal, xx, yy, shading)
    g[unlit], h[unlit] = 256, 0
    rgb = np.clip(attr*g[..., None]/256 + np.array(shading['specular'])*h[..., None]/256, 0, 255)
    return dict(owner=owner, normal=normal, attr=attr, margin=margin, z=z, g=g, h=h, rgb=rgb, unlit=unlit, textured=textured, inverses=inverses)


def read_actual(run, data):
    owner = np.full((240, 400), -1, dtype=int)
    z = np.full((240, 400), 65536, dtype=int)
    normals = np.zeros((240, 400, 3))
    gs, hs = np.zeros((240, 400)), np.zeros((240, 400))
    rgb = np.zeros((240, 400, 3))
    records, packets, payload, winners = {}, [], {}, {}
    base = 0
    for i, instance in enumerate(data['instances']):
        classes = [int(v, 16) for v in (run/f'task{i}/classes.hex').read_text().split()]
        packets.extend(base+j for j, c in enumerate(classes) if c == 0)
        base += len(classes)
    lighting = {}
    for line in (run/'lighting-input.txt').read_text().splitlines():
        ident, *v = map(int, line.split())
        lighting[ident] = v[:3]
    values = {}
    for line in (run/'back.txt').read_text().splitlines():
        a = line.split()
        if a[0] == 'L':
            values[int(a[2]), int(a[3])] = list(map(int, a[4:]))
        elif a[0] == 'F':
            key = int(a[2]), int(a[3])
            v = int(a[4], 16)
            values.setdefault(key, [256, 0]).append([(v >> (8*k)) & 255 for k in range(3)])
    for line in (run/'front.txt').read_text().splitlines():
        a = line.split()
        seq = int(a[2])
        if a[0] == 'A':
            records[seq] = list(map(int, a[3:]))
        elif a[0] == 'W':
            lane, word, value = int(a[3]), int(a[4]), int(a[5], 16)
            if word == 0:
                payload[seq, lane] = [signed(value, 12), signed(value >> 12, 12)]
            elif word == 1:
                payload[seq, lane].append(signed(value, 12))
        elif a[0] == 'N':
            lane, depth, passed = map(int, a[3:])
            packet, _, qx, qy = records[seq]
            x, y = 2*qx+(lane & 1), 2*qy+(lane >> 1)
            if not passed or depth >= z[y, x]:
                continue
            z[y, x], owner[y, x] = depth, packets[packet]
            winners[x, y] = seq, lane
    # Z can retire ahead of the three attribute words; join after the full trace.
    for (x, y), (seq, lane) in winners.items():
        normals[y, x] = payload[seq, lane]
        if seq*4+lane in lighting:
            assert list(normals[y, x]) == lighting[seq*4+lane]
        gs[y, x], hs[y, x], rgb[y, x] = values[seq, lane]
    return dict(owner=owner, normal=normals, z=z, g=gs, h=hs, rgb=rgb)


def save_image(rgb, path, root):
    import re
    text = (root/'systems/cpu-v3-tang-nano-20k/src/display.rs').read_text()
    lut = np.array(list(map(int, re.search(r'LINEAR6_TO_SRGB8: \[u8; 64\] = \[(.*?)\];', text, re.S)[1].replace('\n', '').split(',')[:-1])))
    quant = np.rint(rgb*np.array([31, 63, 31])/255).astype(int)
    display = lut[quant*np.array([2, 1, 2]) + (quant >> np.array([4, 30, 4]))].astype('uint8')
    im = Image.fromarray(display)
    im.save(path)
    im.resize((800, 480), Image.Resampling.NEAREST).save(path.with_stem(path.stem+'-2x'))


def audit(run):
    data = json.loads((run/'input.json').read_text())
    expected, actual = render(data), read_actual(run, data)
    save_image(expected['rgb'], run/'float-reference.png', Path(__file__).resolve().parents[4])
    same = (expected['owner'] == actual['owner']) & (expected['owner'] >= 0)
    interior = same & (expected['margin'] >= 1)
    lit = interior & ~expected['unlit']
    exact_interior = (expected['owner'] >= 0) & (expected['margin'] >= 1)
    angle = np.degrees(np.arccos(np.clip(np.sum(normalize(expected['normal'])*normalize(actual['normal']), axis=-1), -1, 1)))
    def stats(v, mask=interior):
        a = v[mask]
        return dict(mean=float(np.mean(a)), p99=float(np.quantile(a, .99)), maximum=float(np.max(a)))
    out = dict(reference='Independent float64 V96 transform, ray/triangle intersection, perspective attributes and Blinn-Phong; no fixed-point oracle',
               interior_rule='Same winning source primitive; at least 1 pixel from all three exact edges',
               interior_pixels=int(interior.sum()), same_owner=int(same.sum()),
               ownership_mismatch_pixels=int(np.sum(expected['owner'] != actual['owner'])),
               exact_interior_ownership_mismatch_pixels=int(np.sum(exact_interior & ~same)),
               exact_interior_rgb8_error_including_owner_mismatch=stats(np.max(abs(actual['rgb']-expected['rgb']), axis=-1), exact_interior),
               lit_interior_pixels=int(lit.sum()), textured_interior_pixels=int(np.sum(interior & expected['textured'])),
               normal_angle_degrees=stats(angle), diffuse_q8_error=stats(abs(actual['g']-expected['g'])),
               specular_q8_error=stats(abs(actual['h']-expected['h'])),
               lit_normal_angle_degrees=stats(angle, lit),
               lit_diffuse_q8_error=stats(abs(actual['g']-expected['g']), lit),
               lit_specular_q8_error=stats(abs(actual['h']-expected['h']), lit),
               rgb8_max_channel_error=stats(np.max(abs(actual['rgb']-expected['rgb']), axis=-1)),
               lit_rgb8_max_channel_error=stats(np.max(abs(actual['rgb']-expected['rgb']), axis=-1), lit),
               gross_lighting_pixels=int(np.sum(interior & ((abs(actual['g']-expected['g']) > 16) | (abs(actual['h']-expected['h']) > 32)))),
               patch=[dict(x=179, y=y, exact_normal=expected['normal'][y,179].tolist(), rtl_normal=actual['normal'][y,179].tolist(),
                           exact_g=float(expected['g'][y,179]), rtl_g=float(actual['g'][y,179]), owner=int(actual['owner'][y,179])) for y in range(127,132)])
    out['gross_lighting_thresholds_q8'] = dict(diffuse=16, specular=32)
    out['groups'] = {}
    base = 0
    group_ids = {}
    for instance in data['instances']:
        count = len(instance['triangles'])
        group_ids.setdefault(instance['group'], []).extend(range(base, base+count))
        base += count
    for name, ids in group_ids.items():
        mask = interior & np.isin(expected['owner'], ids)
        if np.any(mask):
            out['groups'][name] = dict(interior_pixels=int(mask.sum()), normal_angle_degrees=stats(angle, mask),
                                      diffuse_q8_error=stats(abs(actual['g']-expected['g']), mask),
                                      specular_q8_error=stats(abs(actual['h']-expected['h']), mask),
                                      rgb8_max_channel_error=stats(np.max(abs(actual['rgb']-expected['rgb']), axis=-1), mask))
    # A background interior can still lie on a foreground edge. Inspect both
    # owners, and expose coincident/depth-tie ownership changes separately.
    actual_margin = expected['margin'].copy()
    details = []
    for y, x in zip(*np.where(exact_interior & ~same)):
        pid = int(actual['owner'][y, x])
        if pid < 0:
            actual_margin[y, x] = 0
            details.append(dict(x=int(x), y=int(y), classification='missing coverage'))
            continue
        inv = expected['inverses'][pid]
        coefficients = inv @ np.array([(2*x-399)/512, (2*y-239)/512, 1])
        distance = float(np.min(coefficients/np.maximum(np.linalg.norm(inv[:, :2]*2/512, axis=1), 1e-30)))
        actual_margin[y, x] = distance
        depth_difference = float((1/np.sum(coefficients)-expected['z'][y, x])*256)
        classification = 'foreground edge' if distance < 1 else 'depth tie' if abs(depth_difference) < 1 else 'unexplained owner change'
        details.append(dict(x=int(x), y=int(y), expected_owner=int(expected['owner'][y, x]), actual_owner=pid,
                            actual_edge_distance=distance, depth_difference_d16_codes=depth_difference, classification=classification))
    nonboundary = exact_interior & (actual_margin >= 1)
    out['interior_owner_changes'] = details
    out['nonboundary_pixels_including_depth_ties'] = int(nonboundary.sum())
    out['nonboundary_gross_lighting_pixels_including_depth_ties'] = int(np.sum(nonboundary & ((abs(actual['g']-expected['g']) > 16) | (abs(actual['h']-expected['h']) > 32))))
    (run/'float-reference.json').write_text(json.dumps(out, indent=2))
    print(json.dumps(out, indent=2))
    return out


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('run', type=Path)
    audit(parser.parse_args().run)
