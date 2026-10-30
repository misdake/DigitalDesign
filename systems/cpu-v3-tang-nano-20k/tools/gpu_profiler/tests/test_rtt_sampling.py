"""Fixed framebuffer PC decoding/sampling; independent of packet/cache ordering."""
from pathlib import Path
import struct
import sys
import unittest
import numpy as np

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from scene_oracle import expand, sample, texture_levels
from test_draw_oracle import SyntheticCapture


class RttSamplingTests(unittest.TestCase):
    def test_physical_column_plane_decodes_all_pixels_and_excludes_padding(self):
        capture = SyntheticCapture()
        raw = bytearray()
        for column in range(25):
            for macro_y in range(16):
                for local_y in range(16):
                    for local_x in range(16):
                        x, y = column*16+local_x, macro_y*16+local_y
                        raw += struct.pack('<H', (x*37+y*101) & 65535 if y < 240 else 0x5a5a)
        capture.put(0x600000, raw)
        descriptor = (0x600000//128) | (9 << 16) | (1 << 23)
        result = texture_levels(capture, descriptor)[9]
        expected = np.array([[(x*37+y*101) & 65535 for x in range(400)] for y in range(240)])
        np.testing.assert_array_equal(result, expand(expected))
        for invalid in [descriptor | (1 << 20), descriptor | (2 << 21), descriptor+1]:
            with self.assertRaises(ValueError):
                texture_levels(capture, invalid)

    def test_signed18_clamp_true_dimensions_and_bilinear_edges(self):
        texture = np.array([[[x & 255, y & 255, (x+y) & 255] for x in range(400)]
                            for y in range(240)], dtype=np.int64)
        uv = np.array([[-.25, -.5], [0., 0.], [.5, .5], [1., 1.], [1.25, 1.5], [2., 2.]])
        expected = [[0, 0, 0], [0, 0, 0], [200, 120, 64], [143, 239, 126],
                    [143, 239, 126], [0, 0, 0]]
        for filtering in (0, 1):
            descriptor = (9 << 16) | (1 << 23) | (filtering << 21)
            np.testing.assert_array_equal(sample({9:texture}, descriptor, uv, np.zeros(len(uv))), expected)


if __name__ == '__main__':
    unittest.main()
