"""Native billboard precision keeps the existing record schema and global Y grid."""
import struct
import io
import json
import os
import tempfile
import zipfile
from unittest.mock import patch
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools'))
from static_assets import build, read_geometry


def quad(low=0, high=6, x=1.00390625, z=-2.0078125):
    return {'vertices': [[x, low, z, -1.5, 0, 1], [x, high, z, 1.5, 1, 0],
                         [x, high, z, -1.5, 0, 0], [x, low, z, 1.5, 1, 1]],
            'triangles': [[0, 1, 2], [0, 3, 1]]}


def encode(group, shift=8, exact=True, origin=(0, 0, 0)):
    return read_geometry(group, True, True, origin, 0.125, shift, exact)[0][0]


def position(record, index):
    base = struct.unpack_from('<3i', record, 16)
    shift = struct.unpack_from('<I', record, 28)[0]
    xyz = struct.unpack_from('<I', record, 128 + index * 12)[0]
    return [base[axis] + ((xyz >> (axis * 10)) & 1023) * 2**shift for axis in range(3)]


class BillboardPrecision(unittest.TestCase):
    def test_default_optional_flag_is_identical(self):
        group = quad()
        self.assertEqual(read_geometry(group, True, True, (0, 0, 0), 0.125, 10),
                         read_geometry(group, True, True, (0, 0, 0), 0.125, 10, False))

    def test_exact_anchor_uses_q16_origin_and_zero_xz_delta(self):
        group = quad()
        record = encode(group)
        anchor = struct.unpack_from('<3i', record, 16)
        self.assertEqual(anchor[0], round(group['vertices'][0][0] * 0.125 * 65536))
        self.assertEqual(anchor[2], round(group['vertices'][0][2] * 0.125 * 65536))
        for i in range(4):
            xyz = struct.unpack_from('<I', record, 128+i*12)[0]
            self.assertEqual(xyz & 1023, 0)
            self.assertEqual(xyz >> 20, 0)
        self.assertEqual(len(record), 256)

    def test_shared_y_endpoint_uses_global_grid(self):
        origin = (0, 0.013, 0)
        lower = encode(quad(0.1, 2.37), origin=origin)
        upper = encode(quad(2.37, 5.1), origin=origin)
        self.assertEqual(position(lower, 1)[1], position(upper, 0)[1])
        self.assertEqual(position(lower, 1)[1] % 256, 0)

    def test_y_grid_uses_ties_even(self):
        record = encode(quad(1/64, 3/64))
        self.assertEqual(position(record, 0)[1], 0)
        self.assertEqual(position(record, 1)[1], 512)

    def test_nonvertical_centerline_rejected_only_when_exact(self):
        group = quad()
        group['vertices'][1][0] += 0.01
        with self.assertRaisesRegex(ValueError, 'common X/Z centerline'):
            encode(group)
        encode(group, exact=False)

    def test_delta10_overflow_rejected(self):
        with self.assertRaisesRegex(ValueError, 'delta10 span'):
            encode(quad(0, 33))

    def test_scene_option_types_and_ranges_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            map_path = folder / 'scene.zip'
            camera_path = folder / 'camera.json'
            camera_path.write_text('[]')
            cases = [('billboard_position_shift', value) for value in (-1, 32, True, 8.0, '8')]
            cases += [('billboard_exact_anchor', value) for value in (0, 1, None, 'true')]
            for key, value in cases:
                with self.subTest(key=key, value=value):
                    payload = io.BytesIO()
                    with zipfile.ZipFile(payload, 'w') as archive:
                        archive.writestr('scene.json', json.dumps(dict(
                            version=1, materials=[{}], groups=[], **{key: value})))
                    map_path.write_bytes(payload.getvalue())
                    with patch.dict(os.environ, {'TANKRACER_MAP_INPUT': str(map_path),
                                                'TANKRACER_CAMERA_INPUT': str(camera_path),
                                                'TANKRACER_ROUTE_INPUT': ''}):
                        with self.assertRaisesRegex(ValueError, key):
                            build(folder / 'rejected')

    def test_non_billboard_encoding_unchanged(self):
        geometry = {'vertices': [[0, 0, 0, 0, 0], [1, 1, 0, 1, 1], [0, 1, 0, 0, 1]],
                    'triangles': [[0, 1, 2]]}
        self.assertEqual(read_geometry(geometry, True, False, (0, 0, 0), 0.125, 10),
                         read_geometry(geometry, True, False, (0, 0, 0), 0.125, 10, True))


if __name__ == '__main__':
    unittest.main()
