"""Bounded host checks for streamed selection records and conservative bounds."""
import math
import random
import struct
import tempfile
import unittest
import json
from pathlib import Path
from PIL import Image
from compare_frames import compare

from static_assets import CLUSTER_GROUPS, CLUSTER_WORDS, GROUP_WORDS, cluster_groups, opaque_spans, prepared_ring, q16


def signed(word):
    return word - 65536 if word & 32768 else word


def row(center, extent, identity):
    return [word for value in (*center, math.hypot(*extent), *extent) for word in q16(value)] + [
        identity, 1, identity % 17, identity % 2]


class SelectionRecords(unittest.TestCase):
    def test_prepared_ring_uses_route_order_for_disjoint_groups(self):
        rows = sum((row([i * 4, 0, 0], [1, 1, 1], i) for i in range(3)), [])
        records = sum(([0] * 10 + [i] + [0] * 5 for i in [2, 0, 1]), [])
        directory, index, pages = prepared_ring(rows, 3, (records, [], {}))
        self.assertEqual(directory[3::5], [2, 0, 1])
        self.assertEqual(index, [1, 2, 0])
        self.assertEqual(pages, [0, 3])
        records[10] = 0
        with self.assertRaisesRegex(ValueError, 'each opaque group once'):
            prepared_ring(rows, 3, (records, [], {}))

    def test_prepared_ring_preserves_quantized_overlap_order(self):
        rows = row([0, 0, 0], [1, 1, 1], 0) + row([2.02, 0, 0], [1, 1, 1], 1)
        records = [0] * 10 + [1] + [0] * 15 + [0] + [0] * 5
        self.assertEqual(prepared_ring(rows, 2, (records, [], {}))[0][3::5], [0, 1])
        self.assertEqual(prepared_ring(rows, 2, (records, [], {}), 0)[0][3::5], [1, 0])

    def test_prepared_pages_budget_commands_and_push_storage(self):
        rows = sum((row([i * 4, 0, 0], [1, 1, 1], i) for i in range(260)), [])
        for i in range(260):
            rows[i * 18 + 15] = 2
        directory, _, pages = prepared_ring(rows, 260, None)
        self.assertEqual(pages, [0, 196, 260])
        self.assertEqual(directory[195 * 5 + 4:197 * 5:5], [0, 1])
        for i in range(260):
            rows[i * 18 + 15] = 1
        self.assertEqual(prepared_ring(rows, 260, None)[2], [0, 256, 260])
        rows[15] = 700
        with self.assertRaisesRegex(ValueError, 'one opaque group'):
            prepared_ring(rows, 260, None)
        prepared_ring(rows, 260, None, strict=False)

    def test_empty_prepared_ring_has_a_valid_empty_page(self):
        self.assertEqual(prepared_ring([], 0, None), ([0] * 5, [0], [0, 0]))

    def test_opaque_span_order_keeps_sparse_ranges(self):
        rows = row([0, 0, 0], [1, 1, 1], 0) + row([0, 0, 0], [1, 1, 1], 1)
        rows[14:18] = [0, 2, 7, 0]
        rows[32:36] = [5, 1, 8, 0]
        spans = opaque_spans(rows, 2, 6)
        self.assertEqual(len(spans), 48)
        self.assertEqual([spans[n:n+3] for n in [0, 15]], [[0, 2, 7], [5, 1, 8]])
        self.assertTrue(all(spans[n * 3 + 1] == 0 for n in [1, 2, 3, 4, 6, 15]))
        rows[32] = 0
        with self.assertRaisesRegex(ValueError, 'unique'):
            opaque_spans(rows, 2, 6)

    def test_frame_comparison_near_gate(self):
        with tempfile.TemporaryDirectory() as folder:
            before, after = Path(folder) / 'before', Path(folder) / 'after'
            before.mkdir()
            after.mkdir()
            metadata = dict(format='tankracer.frame-output.v1', frame=1, triangles=1, meshlets=1)
            depth = [65535] * 96000
            depth[0], depth[1] = 256, 9000
            for destination in [before, after]:
                (destination / 'frame-001.json').write_text(json.dumps(metadata))
                Image.new('RGB', (400, 240), (64, 64, 64)).save(destination / 'frame-001.ppm')
                (destination / 'frame-001.d16').write_bytes(struct.pack('<96000H', *depth))
            image = Image.open(after / 'frame-001.ppm')
            image.putpixel((0, 0), (255, 0, 0))
            image.save(after / 'frame-001.ppm')
            result = compare(before, after, 25)
            self.assertFalse(result['near_rgb_equal'])
            self.assertFalse(result['near_gate_passed'])
            allowed = compare(before, after, 25, color_budget=1)
            self.assertTrue(allowed['near_gate_passed'])
            self.assertFalse(allowed['near_rgb_equal'])
            self.assertTrue(result['near_depth_equal'])
            self.assertEqual(result['rows'][0]['near_pixels'], 1)
            image.putpixel((0, 0), (64, 64, 64))
            image.putpixel((1, 0), (255, 0, 0))
            image.save(after / 'frame-001.ppm')
            depth[1] = 65535
            (after / 'frame-001.d16').write_bytes(struct.pack('<96000H', *depth))
            result = compare(before, after, 25)
            self.assertTrue(result['near_rgb_equal'] and result['near_depth_equal'])
            self.assertEqual(result['rows'][0]['depth_changed'], 1)
            depth[0] = 512
            (after / 'frame-001.d16').write_bytes(struct.pack('<96000H', *depth))
            self.assertFalse(compare(before, after, 25)['near_depth_equal'])

    def test_signed_quantization_and_cluster_enclosure(self):
        rng = random.Random(1978)
        original = []
        for identity in range(257):
            center = [rng.uniform(-64, 64) for _ in range(3)]
            extent = [rng.uniform(0.01, 8) for _ in range(3)]
            original.extend(row(center, extent, identity))
        packed, clusters, counts = cluster_groups(original)
        self.assertEqual(sum(counts), 257)
        self.assertEqual(len(packed), GROUP_WORDS * 257)
        self.assertLessEqual(max(counts), CLUSTER_GROUPS)
        seen = set()
        for at in range(0, len(clusters), CLUSTER_WORDS):
            floats = struct.unpack('<7i', struct.pack('<14H', *clusters[at:at + 14]))
            start, count = clusters[at + 14:at + 16]
            for offset in range(start, start + count * GROUP_WORDS, GROUP_WORDS):
                values = packed[offset:offset + GROUP_WORDS]
                identity = values[10]
                self.assertNotIn(identity, seen)
                seen.add(identity)
                source = original[identity * 18:identity * 18 + 18]
                old = struct.unpack('<7i', struct.pack('<14H', *source[:14]))
                center = [signed(v) * 512 for v in values[:3]]
                extent = [v * 512 for v in values[3:6]]
                for axis in range(3):
                    self.assertLessEqual(center[axis] - extent[axis], old[axis] - old[axis + 4])
                    self.assertGreaterEqual(center[axis] + extent[axis], old[axis] + old[axis + 4])
                    self.assertLessEqual(floats[axis] - floats[axis + 4], center[axis] - extent[axis])
                    self.assertGreaterEqual(floats[axis] + floats[axis + 4], center[axis] + extent[axis])
                self.assertGreaterEqual(values[6] * 512, old[3] + math.dist(center, old[:3]))
                self.assertEqual(values[7:9], source[14:16])
                self.assertEqual(values[9] & 32767, source[16])
                self.assertEqual(values[9] >> 15, source[17])
        self.assertEqual(seen, set(range(257)))

    def test_cell_splits_and_original_ids(self):
        original = sum((row([-0.5, 0, -0.5], [0.125] * 3, i) for i in range(65)), [])
        packed, clusters, counts = cluster_groups(original)
        self.assertEqual(counts, [32, 32, 1])
        self.assertEqual([clusters[i + 14] for i in range(0, len(clusters), CLUSTER_WORDS)],
                         [0, 32 * GROUP_WORDS, 64 * GROUP_WORDS])
        self.assertEqual([packed[i + 10] for i in range(0, len(packed), GROUP_WORDS)], list(range(65)))

    def test_invalid_coordinate_and_material(self):
        with self.assertRaisesRegex(ValueError, 'center'):
            cluster_groups(row([256, 0, 0], [1, 1, 1], 0))
        words = row([0, 0, 0], [1, 1, 1], 0)
        words[16] = 32768
        with self.assertRaisesRegex(ValueError, 'material'):
            cluster_groups(words)


if __name__ == '__main__':
    unittest.main()
