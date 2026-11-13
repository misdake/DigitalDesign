"""Source geometry retains native bytes and respects the actual meshlet capacities."""
import struct
import unittest
import tempfile
from pathlib import Path
from mesh_source import Vertex, PositionEncoding, encode_mesh
from gpu_mesh_layout import native_record


class MeshSource(unittest.TestCase):
    def test_base_font_and_geometry_publish_complete_sdk_dependencies(self):
        from native_asset_source import Sources
        with tempfile.TemporaryDirectory() as folder:
            source = Sources(folder)
            source.font('TEST_FONT', 'HI')
            source.geometry('TEST_MESH', [Vertex((0,0,0))], [[0,0,0]])
            manifest = source.write_manifest('assets.rs', sources=[__file__])
            self.assertTrue((Path(folder)/'assets.json').is_file())
            self.assertEqual(len(manifest['resources']), 2)
            paths = manifest['sources']
            self.assertTrue(any(path.endswith('mesh_source.py') for path in paths))
            self.assertTrue(any(path.endswith('native-8x16.txt') or 'assets/fonts/' in path for path in paths))

    def test_source_plane_matches_existing_native_record_and_directory(self):
        points = [(-1,-1,0),(1,-1,0),(1,1,0),(-1,1,0)]
        uv = [(0,0),(1,0),(1,1),(0,1)]
        vertices = [Vertex(point, uv=texcoord) for point,texcoord in zip(points,uv)]
        faces = [[0,1,2],[0,2,3]]
        payload, count, relocations = encode_mesh(vertices,faces)
        packed = [[x | (y << 10),511 << 20,u | (v << 16)]
                  for (x,y),(u,v) in zip([(0,0),(512,0),(512,512),(0,512)],
                                        [(0,0),(65535,0),(65535,65535),(0,65535)])]
        record = native_record(dict(vertices=packed,triangles=faces,config=[-65536,-65536,0,8]))
        expected = bytearray(128)+record
        struct.pack_into('<H',expected,2,len(record)//128-1)
        self.assertEqual(payload,expected)
        self.assertEqual((count,relocations),(1,[(0,128)]))

    def test_large_mesh_preserves_every_triangle_in_bounded_aligned_records(self):
        vertices = [Vertex((i / 32, i % 3, 0)) for i in range(99)]
        triangles = [[i,i+1,i+2] for i in range(97)]*2
        payload,count,relocations = encode_mesh(vertices,triangles)
        self.assertGreater(count,1)
        decoded = 0
        for index,(pointer,offset) in enumerate(relocations):
            self.assertEqual(pointer,index*4)
            self.assertEqual(offset % 128,0)
            header = struct.unpack_from('<Q',payload,offset)[0]
            self.assertLessEqual(header & 255,64)
            self.assertLessEqual((header >> 8) & 127,96)
            decoded += (header >> 8) & 127
            self.assertEqual(struct.unpack_from('<H',payload,pointer)[0],0)
        self.assertEqual(decoded,len(triangles))

    def test_invalid_source_values_fail_before_record_encoding(self):
        for vertices,faces,encoding in [
            ([Vertex((0,0,0),normal=(0,0,0))],[[0,0,0]],None),
            ([Vertex((0,0,0),uv=(2,0))],[[0,0,0]],None),
            ([Vertex((0,0,0))],[[0,1,0]],None),
            ([Vertex((1,0,0))],[[0,0,0]],PositionEncoding((0,0,0),0)),
        ]:
            with self.subTest(vertices=vertices),self.assertRaises(ValueError):
                encode_mesh(vertices,faces,encoding)


if __name__ == '__main__':
    unittest.main()
