"""Frame-artifact checks for near geometry appearing, disappearing and staying distant."""
import json
from pathlib import Path
import struct
import tempfile
import unittest

from PIL import Image
from compare_frames import compare


def frame(folder, depth, color):
    folder.mkdir()
    image = Image.new('RGB', (400, 240), (0, 0, 0))
    image.putpixel((0, 0), color)
    image.save(folder / 'frame-001.ppm')
    values = [65535] * 96000
    values[0] = depth
    (folder / 'frame-001.d16').write_bytes(struct.pack('<96000H', *values))
    (folder / 'frame-001.json').write_text(json.dumps(dict(
        format='tankracer.frame-output.v1', frame=1, triangles=1, meshlets=1)))


class NearChecks(unittest.TestCase):
    def compare_depths(self, before, after):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            frame(root / 'before', before, (0, 0, 0))
            frame(root / 'after', after, (255, 0, 0))
            return (compare(root / 'before', root / 'after', 12),
                    compare(root / 'before', root / 'after', 12, near_mode='union'))

    def test_new_near_geometry_is_included_by_union(self):
        baseline, union = self.compare_depths(65535, 128)
        self.assertTrue(baseline['near_gate_passed'])
        self.assertFalse(union['near_gate_passed'])
        self.assertEqual(union['rows'][0]['near_rgb_changed'], 1)
        self.assertEqual(union['rows'][0]['near_depth_changed'], 1)

    def test_disappearing_near_geometry_is_detected_by_both(self):
        for result in self.compare_depths(128, 65535):
            self.assertFalse(result['near_gate_passed'])
            self.assertEqual(result['rows'][0]['near_pixels'], 1)

    def test_far_changes_remain_explicit_without_using_near_budget(self):
        for result in self.compare_depths(4096, 8192):
            self.assertTrue(result['near_gate_passed'])
            self.assertEqual(result['rows'][0]['rgb_changed'], 1)
            self.assertEqual(result['rows'][0]['depth_changed'], 1)
            self.assertEqual(result['rows'][0]['near_pixels'], 0)


if __name__ == '__main__':
    unittest.main()
