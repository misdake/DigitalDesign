"""Bounded native-Q16 camera/roll/cell-edge checks of baked PVS conservatism."""
import argparse
import json
import math
from pathlib import Path
import random
import re
import struct
import view_pvs


def signed(x):
    return x-65536 if x & 32768 else x


def dot(a, b):
    return sum(x*y for x, y in zip(a, b)) >> 16


def visible(rows, record):
    c = [signed(x)*512 for x in record[:3]]
    e = [x*512 for x in record[3:6]]
    radius = record[6]*512
    d = -dot(rows[2][:3], c)-rows[2][3]
    x, y = [abs(dot(r[:3], c)+r[3]) for r in rows[:2]]
    if 0 < d <= 25*65536 and x <= d*51456 >> 16 and y <= d*30976 >> 16:
        return True
    far = d+dot([abs(x) for x in rows[2][:3]], e)
    return far > 0 and d-radius <= 25*65536 and x <= (far*51456 >> 16)+dot([abs(x) for x in rows[0][:3]], e) and y <= (far*30976 >> 16)+dot([abs(x) for x in rows[1][:3]], e)


def visible_sphere(rows, record):
    center = [signed(x)*512 for x in record[:3]]
    radius = record[6]*512+512
    depth = -dot(rows[2][:3],center)-rows[2][3]
    if depth+radius <= 0 or depth-record[6]*512 > 25*65536:
        return False
    x,y = [abs(dot(row[:3],center)+row[3]) for row in rows[:2]]
    return x <= (depth*51456 >> 16)+(radius*round(1.273*65536) >> 16) and y <= (depth*30976 >> 16)+(radius*round(1.107*65536) >> 16)


def camera(eye, yaw, pitch, roll):
    s, c, sp, cp = math.sin(yaw), math.cos(yaw), math.sin(pitch), math.cos(pitch)
    right, up, back = (c, 0, -s), (-s*sp, cp, -c*sp), (s*cp, sp, c*cp)
    cr, sr = math.cos(roll), math.sin(roll)
    axes = [[cr*x+sr*y for x, y in zip(right, up)], [-sr*x+cr*y for x, y in zip(right, up)], back]
    return [tuple(round(v*65536) for v in [*axis, -sum(a*b for a, b in zip(axis, eye))]) for axis in axes]


def heading(row, sectors):
    x, z = row[0], row[2]
    ax, az = abs(x), abs(z)
    best, selected = az, 0
    for i in range(1, sectors//4+1):
        a = i*2*math.pi/sectors
        score = (ax*round(math.sin(a)*65536) >> 16)+(az*round(math.cos(a)*65536) >> 16)
        if score > best:
            best, selected = score, i
    if z < 0:
        return sectors//2+selected if x < 0 else sectors//2-selected
    return (sectors-selected) % sectors if x < 0 else selected


def check(snapshot, probes):
    folder = snapshot/'tankracer-static-assets'
    manifest = json.loads((folder/'assets.json').read_text())
    info = manifest['view_pvs']
    if not info['enabled']:
        raise ValueError('this snapshot has no enabled PVS')
    grid = json.loads((folder/'route-ring.json').read_text())['location_grid']
    packed = (folder/'map_ring_group_data.bin').read_bytes()
    records = [struct.unpack_from('<16H', packed, i) for i in range(0, len(packed), 32)]
    coarse = info.get('encoding') == 'direct-chunk-masks'
    domains = None
    if coarse:
        source = (folder/'tankracer_static.rs').read_text()
        def array(name):
            body = re.search(r'pub static '+name+r':.*?Buf::new\(\[(.*?)\]\);',source,re.S)[1]
            return [int(value) for value in body.split(',') if value.strip()]
        cells,lists,directory = [array(name) for name in
            ('MAP_ROUTE_LOCATION_CELLS','MAP_ROUTE_LOCATION_LISTS','MAP_RING_SEGMENTS')]
        domains = []
        for at in cells:
            segments = set(lists[at+1:at+1+lists[at]])
            segments |= {(i+1)%(len(directory)//12) for i in segments}
            chunks = set()
            for segment in segments:
                start,count = directory[segment*12:segment*12+2]
                chunks |= {((start+i)%len(records))//32 for i in range(count)}
            domains.append(chunks)
    data = (folder/'map_view_pvs.bin').read_bytes()
    origin = [v/65536 for v in grid['origin_q16']]
    step = grid['step_q16']/65536
    rng = random.Random(91527)
    selected = 0
    for probe in range(probes):
        ix, iz = rng.randrange(grid['columns']), rng.randrange(grid['rows'])
        # Include exact cell edges and native-unit perturbations, arbitrary roll,
        # high/low camera heights and headings at both sides of sector boundaries.
        dx = rng.choice([0, 1/65536, step-1/65536, rng.random()*step])
        dz = rng.choice([0, 1/65536, step-1/65536, rng.random()*step])
        eye = [origin[0]+ix*step+dx, rng.uniform(-12, 12), origin[1]+iz*step+dz]
        yaw = rng.randrange(info['yaw_sectors'])*2*math.pi/info['yaw_sectors']+rng.choice([-1, 1])*math.pi/info['yaw_sectors']+rng.uniform(-1e-4, 1e-4)
        rows = camera(eye, yaw, math.radians(rng.uniform(-19, 19)), rng.uniform(-math.pi, math.pi))
        translation = [r[3] for r in rows]
        reconstructed = [-dot([r[axis] for r in rows], translation) for axis in (0, 2)]
        cell = [math.floor((v/65536-o)/step) for v, o in zip(reconstructed, origin)]
        if not 0 <= cell[0] < grid['columns'] or not 0 <= cell[1] < grid['rows']:
            continue  # Runtime falls back outside the grid.
        key = (cell[1]*grid['columns']+cell[0])*info['yaw_sectors']+heading(rows[2], info['yaw_sectors'])
        mask = view_pvs.decode(data, info, key)
        for i, record in enumerate(records):
            if coarse and i//32 not in domains[key//info['yaw_sectors']]:
                continue
            if visible_sphere(rows,record) if coarse else visible(rows, record):
                selected += 1
                bit = mask[i >> 9] & (1 << ((i >> 5)&15)) if coarse else mask[i >> 4] & (1 << (i&15))
                assert bit, (probe, i, eye, yaw)
    return dict(probes=probes, potential_group_checks=probes*len(records), selected_group_checks=selected,
                false_rejections=0, seed=91527, domain='native-Q16 rigid cameras, arbitrary roll, grid/heading boundaries, absolute pitch <=19 degrees')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('snapshot', type=Path)
    parser.add_argument('--probes', type=int, default=1024)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if not 1 <= args.probes <= 8192:
        parser.error('require 1..8192 bounded probes')
    result = check(args.snapshot, args.probes)
    args.output.write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result))
