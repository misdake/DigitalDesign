"""Conservative map/route priors for investigation; does not reorder rendering."""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import zipfile

def hull(points):
    points = sorted(set(points))
    def turn(a, b, c):
        return (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])
    if len(points) < 2:
        return points
    halves = []
    for order in (points, points[::-1]):
        part = []
        for p in order:
            while len(part) > 1 and turn(part[-2], part[-1], p) <= 0:
                part.pop()
            part.append(p)
        halves.append(part[:-1])
    return halves[0]+halves[1]


def supports(points):
    boundary = hull(points)
    result = []
    for a, b in zip(boundary, boundary[1:]+boundary[:1]):
        dx, dz = b[0]-a[0], b[1]-a[1]
        length = math.hypot(dx, dz)
        if length:
            n = dz/length, -dx/length
            result.append((n, max(n[0]*p[0]+n[1]*p[1] for p in points)))
    return result


def tube_margin(plane, endpoints, radius):
    n, d = plane
    return min(n[0]*x+n[1]*z-d for x, z in endpoints)-radius


def box_margin(plane, lo, hi):
    n, d = plane
    return sum(a*(low if a >= 0 else high) for a, low, high in zip(n, lo, hi))-d


def analyse(scene, route, corridor_world=24, candidate_domain=None):
    if not 0 <= corridor_world <= 256 or len(scene['groups']) > 5460:
        raise ValueError('analysis domain exceeds its bounded scene/corridor limits')
    origin, scale = scene.get('origin', [0, 0, 0]), scene.get('scale', .125)
    if not math.isfinite(scale) or scale <= 0:
        raise ValueError('invalid render scale')
    points = {p['name']: tuple((p['position'][k]-origin[k])*scale for k in (0, 2)) for p in route['points']}
    circuit = [points[name] for name in route['main_loop']]
    segments = list(zip(circuit, circuit[1:]+circuit[:1]))
    if not 3 <= len(segments) <= 512:
        raise ValueError('expected a bounded closed route')
    corridor = corridor_world*scale
    alpha, opaque = [], []
    for source_id, group in enumerate(scene['groups']):
        material = scene['materials'][group['material']]
        billboard = group.get('billboard', False)
        shift = scene.get('billboard_position_shift', scene.get('position_shift', 10)) if billboard else scene.get('position_shift', 10)
        step = (1 << shift)/65536
        shape = []
        for geometry in [group, *[group[k] for k in ('lod', 'bounds_geometry') if k in group]]:
            for v in geometry['vertices']:
                x, z = ((v[k]-origin[k])*scale for k in (0, 2))
                if billboard and scene.get('billboard_exact_anchor', False):
                    x, z = round(x*65536)/65536, round(z*65536)/65536
                else:
                    x, z = round(x/step)*step, round(z/step)*step
                radius = abs(round(v[3]*scale*512))/512 if billboard else 0
                shape.extend((x+sx*radius, z+sz*radius) for sx in (-1, 1) for sz in (-1, 1))
        lo, hi = [min(p[k] for p in shape)-step for k in (0, 1)], [max(p[k] for p in shape)+step for k in (0, 1)]
        # Include native compact-bound rounding, not only source geometry.
        lo, hi = [math.floor(v*128)/128-1/128 for v in lo], [math.ceil(v*128)/128+1/128 for v in hi]
        reachable = set(range(len(segments)))
        if candidate_domain is not None:
            reachable = candidate_domain.get(source_id, set())
        item = dict(source_id=source_id, name=group.get('name'), material=group['material'], billboard=billboard,
                    lo=lo, hi=hi, reachable=reachable, planes=[] if billboard else supports(shape))
        (alpha if material.get('blend', False) else opaque).append(item)
    guard = 2/65536
    one_sided, certified = [], []
    covisible = independent = 0
    for a in alpha:
        endpoints = [p for i in a['reachable'] for p in segments[i]]
        margins = [tube_margin(plane, endpoints, corridor) for plane in a['planes']] if endpoints else []
        if margins and max(margins) > guard:
            best = max(range(len(margins)), key=margins.__getitem__)
            one_sided.append(dict(source_id=a['source_id'], name=a['name'], reachable_segments=sorted(a['reachable']),
                                  plane=a['planes'][best], corridor_clearance_render=margins[best]))
    for ia, a in enumerate(alpha):
        for b in alpha[ia+1:]:
            common = a['reachable'] & b['reachable']
            if not common:
                independent += 1
                continue
            covisible += 1
            endpoints = [p for i in common for p in segments[i]]
            for back, front in ((a, b), (b, a)):
                proof = next((plane for plane in back['planes'] if tube_margin(plane, endpoints, corridor) > guard
                              and box_margin(plane, front['lo'], front['hi']) > guard), None)
                if proof:
                    certified.append(dict(back=back['source_id'], front=front['source_id'], shared_segments=sorted(common),
                                          separating_plane=proof))
                    break
    return dict(format='tankracer.map-priors.v1', scope='Conservative XZ route-tube investigation; no renderer changes or sampled-camera proof claims',
                domain=dict(corridor_world=corridor_world, render_scale=scale, main_route_segments=len(segments),
                            candidate_scope='All route segments' if candidate_domain is None else 'Exact compiled route candidate covering spans, including every covering-gap extra',
                            fallback='Free cameras, another branch or a wider driving corridor require dynamic selection/sorting'),
                counts=dict(opaque=len(opaque), blended=len(alpha), billboards=sum(a['billboard'] for a in alpha),
                            fixed_blended=sum(not a['billboard'] for a in alpha),
                            fixed_always_outside_support_plane=len(one_sided), possible_covisible_pairs=covisible,
                            disjoint_route_domain_pairs=independent, certified_painter_pairs=len(certified),
                            unresolved_pairs=covisible-len(certified)),
                one_sided_fixed_groups=one_sided, painter_constraints=certified,
                billboards_by_source_image=dict(Counter(g.get('source_texture', '?') for g in scene['groups'] if g.get('billboard'))),
                limitations=['A one-sided card alone does not fix its order against another card.',
                             'A certified edge applies only when both groups are visible inside the stated route tube.',
                             'The current center-depth sort can differ from physical painter order; do not replace its output without complete replay comparisons.',
                             'These pairwise constraints do not themselves prove a global total order or an acyclic graph.',
                             'Billboards rotate; their complete horizontal envelopes are used as front bounds and never as fixed separating planes.'])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('scene', type=Path)
    parser.add_argument('route', type=Path)
    parser.add_argument('--corridor-world', type=float, default=24)
    parser.add_argument('--selection', type=Path, help='compiled route-ring.json with adjacent assets.json')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    with zipfile.ZipFile(args.scene) as z:
        scene = json.loads(z.read('scene.json'))
    route = json.loads(args.route.read_text())
    domain = None
    if args.selection:
        manifest = json.loads(args.selection.with_name('assets.json').read_text())
        digest = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
        if (manifest['map_sha256'] != digest(args.scene) or manifest['route']['input_sha256'] != digest(args.route)
                or manifest['route_ring']['sha256'] != digest(args.selection)):
            raise ValueError('compiled selection input identity mismatch')
        ring = json.loads(args.selection.read_text())
        if ring['world_camera_corridor'] != args.corridor_world:
            raise ValueError('selection proof corridor mismatch')
        opaque = [i for i, g in enumerate(scene['groups']) if not scene['materials'][g['material']].get('blend', False)]
        opaque.sort(key=lambda i: scene['groups'][i]['material'])
        ids = opaque+[i for i, g in enumerate(scene['groups']) if scene['materials'][g['material']].get('blend', False)]
        if len(ids) != ring['group_count'] or ring['segment_count'] != len(route['main_loop']):
            raise ValueError('compiled route domain differs from scene')
        domain = {i: set() for i in ids}
        for segment in ring['segments']:
            for offset in range(segment['count']):
                stable = ring['order'][(segment['start']+offset) % len(ids)]
                domain[ids[stable]].add(segment['segment'])
    result = analyse(scene, route, args.corridor_world, domain)
    result['inputs_sha256'] = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in [args.scene, args.route]}
    if args.selection:
        result['inputs_sha256'][str(args.selection)] = digest(args.selection)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(result['counts']))
