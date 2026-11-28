"""Native grid coverage and independent old full-scan oracle checks."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import random
import struct
import unittest

from route_ring import (build_location_grid, fixed_words, native_fixed,
                        native_segment_cell_within, segment_box_distance_squared)
from export_route import write_json_atomic
from render_stage import read_table

REAL = None
Q = 65536


def scan(directory, point, accept, ids=None):
    """The frozen full-scan arithmetic, with no spatial preselection."""
    best, best_distance, accepted = -Q, accept * accept >> 16, []
    if ids is None:
        ids = range(len(directory) // 12)
    for segment in ids:
        at = segment * 12
        a = [native_fixed(directory, at + 2), native_fixed(directory, at + 4)]
        d = [native_fixed(directory, at + 6), native_fixed(directory, at + 8)]
        r = [point[k] - a[k] for k in range(2)]
        if any(not min(0, d[k]) - accept <= r[k] <= max(0, d[k]) + accept for k in range(2)):
            continue
        dot = sum(x * y for x, y in zip(r, d)) >> 16
        t = max(0, min(Q, dot * native_fixed(directory, at + 10) >> 16))
        e = [point[k] - a[k] - (d[k] * t >> 16) for k in range(2)]
        if any(abs(v) > accept for v in e):
            continue
        distance = sum(v * v for v in e) >> 16
        if distance <= accept * accept >> 16:
            accepted.append(segment)
            if best < 0 or distance < best_distance:
                best = (segment * Q + t) % (len(directory) // 12 * Q)
                best_distance = distance
    return best, accepted


def lookup(cells, lists, info, point):
    if not info['enabled']:
        return lists[1:1 + lists[0]]
    if any(p < lo or p >= hi for p, lo, hi in zip(point, info['origin_q16'], info['end_q16'])):
        return []
    indices = []
    for p, origin in zip(point, info['origin_q16']):
        delta = p - origin
        # This is the exact positive F16 multiply/FTOI used by the runtime.
        native_index = (delta * info['inverse_step_q16'] >> 16) >> 16
        assert native_index == delta // info['step_q16']
        indices.append(native_index)
    offset = cells[indices[1] * info['columns'] + indices[0]]
    return lists[offset + 1:offset + 1 + lists[offset]]


def directory_of(segments):
    result = []
    for a, b in segments:
        dx, dz = b[0] - a[0], b[1] - a[1]
        length = dx * dx + dz * dz
        result.extend([0, 0, *fixed_words(a[0]), *fixed_words(a[1]),
                       *fixed_words(dx), *fixed_words(dz), *fixed_words(1 / length)])
    return result


def verify_points(directory, scale, corridor, points):
    cells, lists, info = build_location_grid(directory, scale, corridor)
    accept = round((corridor - scale) * Q)
    checks = ties = empty = outside = 0
    for point in points:
        ids = lookup(cells, lists, info, point)
        self_key, accepted = scan(directory, point, accept)
        grid_key, _ = scan(directory, point, accept, ids)
        assert set(accepted) <= set(ids), (point, accepted, ids)
        assert grid_key == self_key, (point, self_key, grid_key)
        assert ids == sorted(set(ids))
        checks += 1
        ties += len(accepted) > 1
        empty += not ids
        outside += info['enabled'] and any(p < lo or p >= hi for p, lo, hi in zip(point, info['origin_q16'], info['end_q16']))
    return cells, lists, info, dict(oracle_points=checks, multiple_accepted_points=ties,
                                   empty_list_points=empty, outside_grid_points=outside)


class LocationGridChecks(unittest.TestCase):
    def test_exact_integer_distance_including_crossings_and_threshold(self):
        self.assertTrue(native_segment_cell_within([-10, 1], [10, 1], [0, 0], [2, 2], 0))
        self.assertFalse(native_segment_cell_within([-10, 3], [10, 3], [0, 0], [2, 2], 0))
        self.assertTrue(native_segment_cell_within([7*Q, 0], [7*Q, Q], [0, 0], [4*Q, 4*Q], 3*Q))
        self.assertFalse(native_segment_cell_within([7*Q+1, 0], [7*Q+1, Q], [0, 0], [4*Q, 4*Q], 3*Q))
        rng = random.Random(782)
        for _ in range(1000):
            a, b = [[rng.randint(-30, 30) for _ in range(2)] for _ in range(2)]
            if a == b:
                continue
            radius = rng.randint(0, 12)
            expected = segment_box_distance_squared(a, b, [-3, -7], [9, 4]) <= radius * radius
            self.assertEqual(native_segment_cell_within(a, b, [-3, -7], [9, 4], radius), expected)

    def test_all_tied_segments_are_retained_and_native_quantization_is_used(self):
        directory = directory_of([([-8, -1], [8, -1]), ([-8, 1], [8, 1])])
        cells, lists, info, _ = verify_points(directory, 0.125, 3, [[0, 0], [Q, 0], [-Q, 0]])
        self.assertEqual(lookup(cells, lists, info, [0, 0]), [0, 1])
        self.assertEqual(scan(directory, [0, 0], 23*Q//8)[0], Q//2)
        # The source float could be just outside the threshold while its native
        # endpoint rounds onto it. The producer accepts only native coefficients.
        original = 7 + 0.49/Q
        self.assertGreater(segment_box_distance_squared([original, 0], [original, 1], [0, 0], [4, 4]), 9)
        native = directory_of([( [original, 0], [original, 1] )])
        self.assertEqual(native_fixed(native, 2), 7*Q)

    def test_different_scale_uses_exact_step_and_unsafe_layout_keeps_full_scan(self):
        directory = directory_of([([0, 0], [16, 0])])
        _, _, info = build_location_grid(directory, 0.1, 2.4)
        self.assertTrue(info['enabled'])
        self.assertEqual(info['step_q16'] * info['inverse_step_q16'], Q*Q)
        for scale in (1e-9, 1e9):
            cells, lists, info = build_location_grid(directory, scale, 3)
            self.assertFalse(info['enabled'])
            self.assertEqual(lookup(cells, lists, info, [100*Q, 0]), [0])
        large = directory_of([([-1000, -1000], [1000, 1000])])
        _, lists, info = build_location_grid(large, 0.125, 3)
        self.assertFalse(info['enabled'])
        self.assertEqual(lists, [1, 0])
        tiny = directory_of([([0, 0], [.02, .02])])
        cells, lists, info = build_location_grid(tiny, .0001, .0024)
        self.assertFalse(info['enabled'])
        self.assertIn('acceptance quantization', info['fallback_reason'])
        # A real old-scan acceptance beyond24world at this tiny scale comes
        # from squared-distance truncation. Retain its exact old behavior.
        accept = round(.0023*Q)
        self.assertGreaterEqual(scan(tiny, [275, -25], accept)[0], 0)
        self.assertEqual(scan(tiny, [275, -25], accept),
                         scan(tiny, [275, -25], accept, lookup(cells, lists, info, [275, -25])))
        _, lists, info = build_location_grid([], 0.125, 0)
        self.assertFalse(info['enabled'])
        self.assertEqual(lists, [0])

    def test_real_grid_edges_corners_corridor_and_endpoints_match_old_oracle(self):
        if REAL is None:
            self.skipTest('Supply --packed for the real native-directory audit')
        folder = REAL.packed
        directory = read_table(folder, 'MAP_RING_SEGMENTS')
        ring = json.loads((folder/'route-ring.json').read_bytes())
        cells, lists, info = build_location_grid(directory, ring['render_scale'], ring['camera_corridor'])
        points = []
        # Both sides of every native cell edge/corner, including outside bounds.
        for z in range(info['rows'] + 1):
            for x in range(info['columns'] + 1):
                corner = [info['origin_q16'][0] + x*info['step_q16'],
                          info['origin_q16'][1] + z*info['step_q16']]
                points.extend([[corner[0]+dx, corner[1]+dz] for dx in (-1, 0, 1) for dz in (-1, 0, 1)])
        accept = round((ring['camera_corridor'] - ring['render_scale'])*Q)
        for at in range(0, len(directory), 12):
            a = [native_fixed(directory, at+2), native_fixed(directory, at+4)]
            d = [native_fixed(directory, at+6), native_fixed(directory, at+8)]
            for fraction in (0, .25, .5, .75, 1):
                center = [round(a[k]+fraction*d[k]) for k in range(2)]
                for angle in range(8):
                    for radius in (0, accept-1, accept, accept+1):
                        points.append([center[0]+round(radius*math.cos(angle*math.pi/4)),
                                       center[1]+round(radius*math.sin(angle*math.pi/4))])
        _, _, actual, checks = verify_points(directory, ring['render_scale'], ring['camera_corridor'], points)
        self.assertEqual(actual['max_segments'], 3)
        self.assertEqual((actual['columns'], actual['rows'], actual['unique_lists']), (18, 27, 77))
        self.assertGreater(checks['multiple_accepted_points'], 0)
        self.assertGreater(checks['empty_list_points'], 0)
        self.assertGreater(checks['outside_grid_points'], 0)
        write_json_atomic(REAL.output, dict(format='tankracer.route-location-grid-check.v1',
            directory_sha256=hashlib.sha256(struct.pack('<'+'H'*len(directory), *directory)).hexdigest(),
            grid=actual, **checks, all_accepted_native_segment_ids_retained=True,
            all_s_keys_equal_old_full_scan=True,
            analytic_margin='23world accepted distance plus native projection/dot truncation error is below24world cell coverage.'), indent=2)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--packed', type=Path)
    parser.add_argument('--output', type=Path)
    args, remaining = parser.parse_known_args()
    if args.packed:
        if args.output is None:
            parser.error('--output required with --packed')
        REAL = args
    unittest.main(argv=[__file__, *remaining])
