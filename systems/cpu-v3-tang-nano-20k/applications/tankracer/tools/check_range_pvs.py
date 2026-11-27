"""Bounded interval-optimality, table traversal and Q16 camera-domain checks."""
import argparse
import json
import math
from pathlib import Path
import random
import re
import struct
import unittest

from range_pvs import cover, order_groups, plan_ranges
from check_view_pvs import camera, dot, heading, visible


class CoverTests(unittest.TestCase):
    def test_every_subset_matches_brute_force_weighted_gap_cover(self):
        pages = [0]*4+[1]*4
        weights = [3,11,2,7,5,1,13,4]
        for bits in range(1,256):
            required = [i for i in range(8) if bits>>i&1]
            needed = len({pages[i] for i in required})
            for limit in range(needed,5):
                ranges = cover(required,pages,weights,limit)
                actual = {i for a,b in ranges for i in range(a,b)}
                self.assertLessEqual(len(ranges),limit)
                self.assertTrue(set(required)<=actual)
                self.assertTrue(all(pages[a]==pages[b-1] for a,b in ranges))
                cost = sum(weights[i] for i in actual-set(required))
                best = math.inf
                for added in range(256):
                    included = {i for i in range(8) if added>>i&1}|set(required)
                    count = sum(i-1 not in included or pages[i-1]!=pages[i] for i in included)
                    if count<=limit:
                        best = min(best,sum(weights[i] for i in included-set(required)))
                self.assertEqual(cost,best)

    def test_physical_page_lower_bound_is_explicit(self):
        with self.assertRaisesRegex(ValueError,'physical page'):
            cover([0,2],[0,0,1],[1,1,1],1)
        self.assertEqual(cover([],[],[],4),())

    def test_single_group_prefix_is_removed_before_counting_pages(self):
        pages = list(range(5))
        required = list(range(5))
        weights = [1] * 5
        units = [dict(count=1) for _ in required]
        self.assertEqual(plan_ranges(required, units, pages, weights, 4),
                         (0, 1, 2, 2, 3, 3, 4, 4, 5))
        units[0]['count'] = 2
        with self.assertRaisesRegex(ValueError, 'physical page'):
            plan_ranges(required, units, pages, weights, 4)

    def test_spatial_priority_preserves_intersecting_and_rotating_bounds(self):
        def row(x,extent,billboard=False,radius=None):
            values = (x,0,0,math.sqrt(3)*extent if radius is None else radius,extent,extent,extent)
            return list(struct.unpack('<14H',struct.pack('<7i',*(round(v*65536) for v in values))))+[0,1,0,int(billboard)]
        disjoint = row(0,.1)+row(4,.1)
        self.assertEqual(order_groups(disjoint,[0,1],[1,0],0),[1,0])
        overlapping = row(0,1)+row(.5,1)
        self.assertEqual(order_groups(overlapping,[0,1],[1,0],0),[0,1])
        rotating = row(0,.1,True,5)+row(4,.1)
        self.assertEqual(order_groups(rotating,[0,1],[1,0],0),[0,1])


def check(snapshot, probes):
    folder = snapshot/'tankracer-static-assets'
    info = json.loads((folder/'assets.json').read_text())['range_pvs']
    source = (folder/'tankracer_static.rs').read_text()
    def array(name):
        return [int(x) for x in re.search(r'pub static '+name+r':.*?Buf::new\(\[(.*?)\]\);',source,re.S)[1].split(',') if x.strip()]
    flat = array('MAP_RANGE_UNITS')
    units = [flat[i:i+4] for i in range(0,len(flat),4)]
    blob = (folder/'map_range_draws.bin').read_bytes()
    draws = list(struct.iter_unpack('<4H',blob))
    data = (folder/'map_range_plans.bin').read_bytes()
    groups = list(struct.iter_unpack('<16H',(folder/'map_ring_group_data.bin').read_bytes()))
    assert len(draws)==len(groups) and sorted(d[3] for d in draws)==list(range(len(draws)))
    plans = {}
    roots = []
    max_ranges = 0
    for key in range(info['columns']*info['rows']*32):
        cell,yaw = divmod(key,32)
        row = struct.unpack_from('<H',data,cell*2)[0]
        root = struct.unpack_from('<H',data,info['heading_first_sector']*128+row*64+yaw*2)[0]
        roots.append(root)
        if root==65535 or root in plans:
            continue
        offset = info['plan_first_sector']*128+root*info['plan_words']*2
        words = struct.unpack_from(f'<{info["plan_words"]}H',data,offset)
        count,seed = words[:2]
        assert count<=info['limit']
        max_ranges = max(max_ranges,count)
        included = set()
        if seed!=65535:
            included.add(draws[units[seed][0]][3])
        prior = -1
        for i in range(count):
            if info.get('packed_ranges'):
                first,end = words[2+i]&255,words[2+i]>>8
            else:
                first,end = words[2+i*2:4+i*2]
            assert 0<=first<end<=len(units) and first>prior
            assert units[first][2]==units[end-1][2]
            prior = end-1
            for unit in units[first:end]:
                included.update(draws[j][3] for j in range(unit[0],unit[0]+unit[1]))
        plans[root] = included
    rng = random.Random(91421)
    origin = [v/65536 for v in info['origin_q16']]
    step = info['step_q16']/65536
    checked = admitted = skipped = 0
    for probe in range(probes):
        x,z = rng.randrange(info['columns']),rng.randrange(info['rows'])
        eye = [origin[0]+x*step+rng.choice([0,1/65536,step-1/65536,rng.random()*step]),
               rng.uniform(-12,12),origin[1]+z*step+rng.choice([0,1/65536,step-1/65536,rng.random()*step])]
        yaw = rng.randrange(32)*math.pi/16+rng.choice([-1,1])*math.pi/32+rng.uniform(-1e-4,1e-4)
        rows = camera(eye,yaw,math.radians(rng.uniform(-19,19)),rng.uniform(-math.pi,math.pi))
        translation = [r[3] for r in rows]
        position = [-dot([r[axis] for r in rows],translation)/65536 for axis in (0,2)]
        cell = [math.floor((v-o)/step) for v,o in zip(position,origin)]
        if not 0<=cell[0]<info['columns'] or not 0<=cell[1]<info['rows']:
            skipped += 1
            continue
        key = (cell[1]*info['columns']+cell[0])*32+heading(rows[2],32)
        root = roots[key]
        if root==65535:
            skipped += 1
            continue
        for record in groups:
            checked += 1
            if visible(rows,record):
                admitted += 1
                assert record[10] in plans[root],(probe,record[10],eye,yaw,root)
    return dict(keys=len(roots),plans=len(plans),probes=probes,skipped_outside_domain=skipped,
                group_checks=checked,visible_checks=admitted,false_rejections=0,max_ranges=max_ranges)


if __name__=='__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--snapshot',type=Path)
    parser.add_argument('--probes',type=int,default=1024)
    parser.add_argument('--output',type=Path)
    args = parser.parse_args()
    if args.snapshot:
        if not 1<=args.probes<=8192: parser.error('require 1..8192 probes')
        result = check(args.snapshot,args.probes)
        if args.output: args.output.write_text(json.dumps(result,indent=2)+'\n')
        print(json.dumps(result))
    else:
        unittest.main(argv=['check_range_pvs'])
