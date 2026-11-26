"""Bounded route-ring enclosure and wrap checks."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import random
import statistics
import struct
import zipfile
import unittest

from check_route import sample
from route_ring import (build_ring, box_gap_squared, covering_span, split_span,
                        distance_squared, segment_box_distance_squared, signed)
from export_route import write_json_atomic

REAL_INPUTS = None


class RingChecks(unittest.TestCase):
    def test_shortest_span_includes_all_candidates_with_two_slices(self):
        rng = random.Random(775)
        for total in range(1, 50):
            for _ in range(20):
                required = [i for i in range(total) if rng.randrange(3) == 0]
                start, count = covering_span(required, total)
                spans = split_span(start, count, total)
                selected = {i for first, length in spans for i in range(first, first + length)}
                self.assertLessEqual(len(spans), 2)
                self.assertTrue(set(required) <= selected)
                # Exhaustively rule out any shorter cyclic cover.
                for shorter in range(count):
                    self.assertFalse(any(set(required) <= {(s + i) % total for i in range(shorter)}
                                         for s in range(total)))

    def test_segment_bbox_is_not_the_segment_distance(self):
        a, b, lo, hi = [0, 0], [100, 100], [90, 0], [100, 10]
        self.assertEqual(box_gap_squared([0, 0], [100, 100], lo, hi), 0)
        self.assertEqual(segment_box_distance_squared(a, b, lo, hi), 3200)
        self.assertEqual(segment_box_distance_squared(a, b, [50, 50], [60, 60]), 0)

    def test_hairpin_and_large_object_are_conservative(self):
        route = sample()
        positions = [[0, 0, 0], [100, 0, 0], [100, 0, 10], [20, 0, 10]]
        for point, position in zip(route["points"], positions):
            point["position"] = position
            point["position_q10"] = [v * 1024 for v in position]
        records = []
        for identity, (x, z, extent) in enumerate([(10, 0, 1), (90, 10, 1), (200, 100, 180)]):
            records.extend([round(x * 128), 0, round(z * 128), round(extent * 128), 128,
                            round(extent * 128), 32768, identity, 1, 0, identity])
        metadata, directory, info = build_ring(route, records, 3, [0, 0, 0], 1, 20, 4)
        self.assertEqual(len(metadata), 3 * 16)
        self.assertEqual(len(directory), 3 * 12)
        self.assertEqual(set(info["order"]), {0, 1, 2})
        self.assertIn(1, info["segments"][0]["required_ids"], "nearby return side of hairpin")
        self.assertIn(2, info["segments"][0]["required_ids"], "large object bounds, not just center")
        for row in info["segments"]:
            selected = {info["order"][i] for first, count in row["spans"] for i in range(first, first + count)}
            self.assertTrue(set(row["required_ids"]) <= selected)

    def test_all_static_ring_preserves_transparent_stable_records(self):
        route = sample()
        packed = []
        for identity in range(3):
            packed.extend([identity * 128, 0, 0, 128, 128, 128, 256,
                           identity, 1, identity | 32768, identity])
        old, _, old_info = build_ring(route, packed, 1, [0, 0, 0], 1, 20)
        all_groups, _, info = build_ring(route, packed, 1, [0, 0, 0], 1, 20,
                                       include_blended=True)
        self.assertEqual(old_info['group_count'], 1)
        self.assertEqual(len(old), 16)
        self.assertEqual(info['group_count'], 3)
        self.assertEqual(info['opaque_stable_id_range'], [0, 1])
        self.assertEqual(info['transparent_stable_id_range'], [1, 3])
        self.assertEqual(set(info['order']), {0, 1, 2})
        for index, identity in enumerate(info['order']):
            self.assertEqual(all_groups[index*16:index*16+11], packed[identity*11:identity*11+11])

    def test_box_bound_covers_any_point_pair_in_camera_corridor(self):
        rng = random.Random(991)
        for _ in range(1000):
            a = [rng.uniform(-100, 100) for _ in range(2)]
            b = [rng.uniform(-100, 100) for _ in range(2)]
            t = rng.random()
            eye = [x + t * (y - x) + rng.uniform(-3, 3) for x, y in zip(a, b)]
            visible = [x + rng.uniform(-10, 10) for x in eye]
            lo = [v - rng.random() for v in visible]
            hi = [v + rng.random() for v in visible]
            lower = box_gap_squared(lo, hi, [min(x, y) for x, y in zip(a, b)],
                                    [max(x, y) for x, y in zip(a, b)])
            self.assertLessEqual(lower, sum((x - y) ** 2 for x, y in zip(visible, [x + t * (y - x) for x, y in zip(a, b)])) + 1e-9)


def audit_real(scene_path, directory, reference=None):
    source_bytes = scene_path.read_bytes()
    source = json.loads(zipfile.ZipFile(scene_path).read("scene.json"))
    assets = json.loads((directory / "assets.json").read_bytes())
    info = json.loads((directory / "route-ring.json").read_bytes())
    route = json.loads((directory / "route.json").read_bytes())["route"]
    raw = (directory / "map_group_data.bin").read_bytes()
    words = struct.unpack(f"<{len(raw) // 2}H", raw)
    packed = {words[i + 10]: list(words[i:i + 11]) for i in range(0, len(words), 11)}
    metadata_raw = (directory / "map_ring_group_data.bin").read_bytes()
    metadata = struct.unpack(f"<{len(metadata_raw) // 2}H", metadata_raw)
    assert sorted(info["order"]) == list(range(info["group_count"]))
    for index, identity in enumerate(info["order"]):
        assert list(metadata[index * 16:index * 16 + 11]) == packed[identity]
    names = {group["name"]: i for i, group in enumerate(assets["groups"])}
    boxes, original = {}, {}
    for index, group in enumerate(source["groups"]):
        if info["group_count"] != len(source["groups"]) and source["materials"][group["material"]].get("blend", False):
            continue
        identity = names[group["name"]]
        vertices = (group.get("bounds_geometry") or group)["vertices"]
        if group.get("billboard"):
            boxes[identity] = ([min(v[a] - abs(v[3]) for v in vertices) for a in (0, 2)],
                               [max(v[a] + abs(v[3]) for v in vertices) for a in (0, 2)])
        else:
            boxes[identity] = ([min(v[a] for v in vertices) for a in (0, 2)],
                               [max(v[a] for v in vertices) for a in (0, 2)])
        original[identity] = index
    scale, origin = assets["render_scale"], assets["origin"]
    quantized = {}
    for identity in boxes:
        row = packed[identity]
        center = [signed(row[a]) / 128 / scale + origin[a] for a in (0, 2)]
        extent = [row[a] / 128 / scale for a in (3, 5)]
        quantized[identity] = ([c - e for c, e in zip(center, extent)],
                               [c + e for c, e in zip(center, extent)])
        lo, hi = quantized[identity]
        assert all(a <= b for a, b in zip(lo, boxes[identity][0]))
        assert all(a >= b for a, b in zip(hi, boxes[identity][1]))
    point_map = {p["name"]: [p["position"][0], p["position"][2]] for p in route["points"]}
    circuit = [point_map[n] for n in route["main_loop"]]
    segments = [(a, circuit[(i + 1) % len(circuit)]) for i, a in enumerate(circuit)]
    def sort_order(bounds):
        keys = {}
        for identity, (lo, hi) in bounds.items():
            center = [(a + b) / 2 for a, b in zip(lo, hi)]
            _, t, segment = min(((*distance_squared(center, a, b), i)
                                 for i, (a, b) in enumerate(segments)), key=lambda v: (v[0], v[2]))
            keys[identity] = segment, t, original[identity]
        return sorted(bounds, key=keys.get)
    def variant(bounds, order, loose):
        required_counts, counts = [], []
        for a, b in segments:
            lower = [min(x, y) for x, y in zip(a, b)]
            upper = [max(x, y) for x, y in zip(a, b)]
            required = [i for i, identity in enumerate(order)
                        if (box_gap_squared(*bounds[identity], lower, upper) if loose else
                            segment_box_distance_squared(a, b, *bounds[identity])) <= 224 ** 2]
            required_counts.append(len(required))
            counts.append(covering_span(required, len(order))[1])
        return dict(required=[min(required_counts), statistics.median(required_counts), max(required_counts)],
                    ring=[min(counts), statistics.median(counts), max(counts)],
                    required_counts=required_counts, counts=counts)
    raw_order = sort_order(boxes)
    variants = dict(raw_exact_raw_order=variant(boxes, raw_order, False),
                    raw_segment_aabb_raw_order=variant(boxes, raw_order, True),
                    packed_exact_raw_order=variant(quantized, raw_order, False),
                    packed_exact_current_order=variant(quantized, info["order"], False),
                    packed_segment_aabb_current_order=variant(quantized, info["order"], True))
    assert variants["packed_exact_current_order"]["counts"] == [s["count"] for s in info["segments"]]
    if reference:
        expected = json.loads(reference.read_bytes())
        assert expected["source_sha256"] == hashlib.sha256(source_bytes).hexdigest()
        assert variants["raw_exact_raw_order"]["counts"] == [s["count"] for s in expected["rows"]]
        assert variants["raw_exact_raw_order"]["required_counts"] == [s["required_groups"] for s in expected["rows"]]
    cameras = checks = large_hits = neighboring_leg_hits = 0
    nearest = {}
    for identity, (lo, hi) in quantized.items():
        center = [(a + b) / 2 for a, b in zip(lo, hi)]
        nearest[identity] = min(range(len(segments)), key=lambda s: (distance_squared(center, *segments[s])[0], s))
    for segment, (a, b) in enumerate(segments):
        row = info["segments"][segment]
        selected = {info["order"][i] for first, count in row["spans"] for i in range(first, first + count)}
        for t in (0.0, 0.5, 1.0):
            anchor = [x + t * (y - x) for x, y in zip(a, b)]
            offsets = [(0.0, 0.0)] + [(24 * math.cos(i * math.pi / 4), 24 * math.sin(i * math.pi / 4)) for i in range(8)]
            for offset in offsets:
                eye = [x + d for x, d in zip(anchor, offset)]
                assert distance_squared(eye, a, b)[0] <= 24 ** 2 + 1e-9
                cameras += 1
                for identity, (lo, hi) in quantized.items():
                    if box_gap_squared(eye, eye, lo, hi) <= 200 ** 2:
                        checks += 1
                        assert identity in selected, (segment, eye, identity)
                        center = [(x + y) / 2 for x, y in zip(lo, hi)]
                        if math.dist(eye, center) > 200:
                            large_hits += 1
                        if min((nearest[identity] - segment) % len(segments),
                               (segment - nearest[identity]) % len(segments)) > 1:
                            neighboring_leg_hits += 1
    assert large_hits > 0 and neighboring_leg_hits > 0
    expansion = max(max(a - c, d - b) for identity in boxes
                    for a, b, c, d in zip(*boxes[identity], *quantized[identity]))
    return dict(format="tankracer.route-structure-check.v1", source_sha256=hashlib.sha256(source_bytes).hexdigest(),
                variants=variants, sampled_cameras=cameras, enclosed_visible_group_checks=checks,
                large_bounds_hits=large_hits, neighboring_leg_hits=neighboring_leg_hits,
                maximum_outward_expansion_world=expansion, group_metadata_bytes=len(metadata_raw),
                metadata_payloads_preserved=True, threshold_world=dict(display=200, corridor=24),
                radius_field_used=False)


class RealRingChecks(unittest.TestCase):
    def test_real_england_corridor_bounds_and_stable_payloads(self):
        if REAL_INPUTS is None:
            self.skipTest("supply --scene and --packed for the real-data audit")
        args = REAL_INPUTS
        result = audit_real(args.scene, args.packed, args.reference_estimate)
        if args.output:
            write_json_atomic(args.output, result, indent=2)
        print(json.dumps({k: v for k, v in result.items() if k != "variants"}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--scene", type=Path)
    parser.add_argument("--packed", type=Path)
    parser.add_argument("--reference-estimate", type=Path)
    parser.add_argument("--output", type=Path)
    args, remaining = parser.parse_known_args()
    if bool(args.scene) != bool(args.packed):
        parser.error("--scene and --packed must be supplied together")
    REAL_INPUTS = args if args.scene else None
    unittest.main(argv=[__file__, *remaining])
