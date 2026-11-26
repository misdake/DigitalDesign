"""Check fixed ring-chunk bounds used by the runtime coarse classifier."""
import math
from pathlib import Path
import random
import struct
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools'))
from static_assets import ring_chunks
from route_ring import signed


class RuntimeRingChunks(unittest.TestCase):
    def test_chunks_enclose_boxes_and_false_positive_spheres(self):
        rng = random.Random(1834)
        metadata = []
        for identity in range(81):
            center = [rng.randint(-12000, 12000) for _ in range(3)]
            extent = [rng.randint(1, 800) for _ in range(3)]
            # Deliberately let some group spheres extend beyond their box corners.
            radius = math.ceil(math.hypot(*extent)) + 80
            metadata.extend([*(v & 65535 for v in center), *extent, radius,
                             identity, 1, 0, identity, 0, 0, 0, 0, 0])
        chunks = ring_chunks(metadata, 26)
        self.assertEqual(len(chunks), 4 * 16)
        seen = []
        for at in range(0, len(chunks), 16):
            fixed = struct.unpack('<7i', struct.pack('<14H', *chunks[at:at+14]))
            center = [v / 65536 for v in fixed[:3]]
            radius = fixed[3] / 65536
            extent = [v / 65536 for v in fixed[4:]]
            first, count = chunks[at+14:at+16]
            self.assertLessEqual(count, 26)
            for index in range(first, first + count):
                record = metadata[index*16:(index+1)*16]
                point = [signed(v)/128 for v in record[:3]]
                size = [v/128 for v in record[3:6]]
                for axis in range(3):
                    self.assertLessEqual(center[axis]-extent[axis], point[axis]-size[axis])
                    self.assertGreaterEqual(center[axis]+extent[axis], point[axis]+size[axis])
                self.assertGreaterEqual(radius, math.dist(center, point)+record[6]/128)
                seen.append(record[10])
        self.assertEqual(seen, list(range(81)))

    def test_last_partial_chunk_and_empty_table(self):
        row = [0, 0, 0, 128, 256, 384, 512, 0, 1, 0, 0, 0, 0, 0, 0, 0]
        chunks = ring_chunks(row * 697, 26)
        self.assertEqual(len(chunks), 27 * 16)
        self.assertEqual(chunks[-2:], [676, 21])
        self.assertEqual(ring_chunks([], 26), [])

    def test_malformed_metadata_is_rejected(self):
        for data, size in [([0]*15, 26), ([], 0)]:
            with self.assertRaises(ValueError):
                ring_chunks(data, size)


if __name__ == '__main__':
    unittest.main()
