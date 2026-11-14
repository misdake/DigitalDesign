"""Source geometry retains native bytes and respects the actual meshlet capacities."""
import struct
import unittest
import tempfile
from pathlib import Path
from mesh_source import Vertex, PositionEncoding, encode_mesh
from gpu_mesh_layout import native_record
from mesh_inspect import inspect_mesh


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
            self.assertTrue(any(path.endswith('mesh_inspect.py') for path in paths))
            self.assertTrue(any(path.endswith('meshopt_clusterizer.mjs') for path in paths))
            self.assertTrue(any(path.endswith('build_meshlets.mjs') for path in paths))
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
        explicit, _, _ = encode_mesh(vertices, faces, PositionEncoding((-1,-1,0),8))
        self.assertEqual(explicit, expected)
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

    def test_shared_vertices_decode_identically_across_meshlet_boundaries(self):
        vertices = [Vertex((i / 37, (i % 3) / 7, 0), uv=(i / 128, 0)) for i in range(99)]
        payload, _, relocations = encode_mesh(vertices, [[i,i+1,i+2] for i in range(97)])
        decoded, shared = {}, 0
        for _, offset in relocations:
            count = payload[offset]
            ox, oy, oz, shift = struct.unpack_from('<iiiI', payload, offset + 16)
            for i in range(count):
                position, _, uv = struct.unpack_from('<3I', payload, offset + 128 + i * 12)
                source = round((uv & 65535) * 128 / 65535)
                world = tuple(origin + ((position >> (axis * 10)) & 1023) * (1 << shift)
                              for axis, origin in enumerate((ox, oy, oz)))
                if source in decoded:
                    self.assertEqual(world, decoded[source], f'shared vertex {source}')
                    shared += 1
                decoded[source] = world
        self.assertGreater(shared, 0)
        self.assertEqual(len(decoded), len(vertices))

    def test_local_origins_keep_distant_meshlets_on_a_fine_shared_grid(self):
        points = [(0,0,0),(0.125,0,0),(0,0.125,0)]
        vertices = [Vertex((x + offset,y,z)) for offset in (0,1000) for x,y,z in points]
        payload, _, relocations = encode_mesh(vertices, [[0,1,2]]*96 + [[3,4,5]]*96)
        self.assertEqual(len(relocations), 2)
        for _, offset in relocations:
            shift = struct.unpack_from('<I', payload, offset + 28)[0]
            self.assertLessEqual(shift, 4, 'step follows local spans, not the 1000-unit mesh AABB')

    def test_canonical_boundary_keeps_fine_interiors_and_distinct_seam_attributes(self):
        from mesh_source import _position_grids
        vertices = [Vertex((0.0123,0.0123,0)), Vertex((8,0,0)), Vertex((0,8,0)),
                    Vertex((0.015,0.0123,0)), Vertex((0.0123,0.015,0)),
                    Vertex((0.0123,0.0123,0),normal=(0,1,0),uv=(1,1))]
        faces = [[0,1,2]]*96 + [[0,3,4]]*96 + [[5,3,4]]*96
        # Quantization constraints do not depend on the clustering heuristic.
        groups = [([vertices[i] for i in ids],[[0,1,2]]) for ids in [[0,1,2],[0,3,4],[5,3,4]]]
        grids = list(_position_grids(groups,None))
        self.assertEqual([shift for _,_,shift in grids], [10,0,0])
        self.assertEqual(grids[1][0][1], tuple(round(v*65536) for v in vertices[3].position))
        audit = {}
        payload, count, _ = encode_mesh(vertices,faces,audit=audit)
        inspect_mesh(payload,count,vertices,faces,source_maps=audit['source_maps'])

    def test_boundary_rounding_recomputes_meshlet_level_until_stable(self):
        from mesh_source import _position_grids
        vertices = [Vertex((x/65536,0,0)) for x in [1537,0,8*65536,514,515]]
        # The fine record initially fits shift0 (1537-514=1023). Its boundary
        # moves to 2048 on the coarse grid, requiring shift1 in a later round.
        groups = [([vertices[i] for i in ids],[[0,1,2]]) for ids in [[0,1,2],[0,3,4]]]
        grids = list(_position_grids(groups,None))
        self.assertEqual([shift for _,_,shift in grids], [10,1])
        self.assertEqual(grids[1][0][2][0], 516)

    def test_independent_inspector_rejects_position_normal_attribute_and_layout_mutations(self):
        from unittest.mock import patch
        vertices = [Vertex((i/37,(i%3)/7,0),uv=(i/128,0)) for i in range(99)]
        faces = [[i,i+1,i+2] for i in range(97)]
        audit = {}
        payload, count, relocations = encode_mesh(vertices, faces,audit=audit)
        inspect_mesh(payload,count,vertices,faces,source_maps=audit['source_maps'])
        offset = relocations[1][1]
        for label, at in [('position',offset+128),('normal',offset+132),('normal mode',offset+29),
                          ('UV',offset+136),('directory',6)]:
            damaged = bytearray(payload)
            damaged[at] ^= 1
            with self.subTest(label=label), self.assertRaisesRegex(ValueError,'native mesh audit'):
                inspect_mesh(damaged,count,vertices,faces,source_maps=audit['source_maps'])
        with patch('mesh_source.inspect_mesh', side_effect=ValueError('audit marker')):
            with self.assertRaisesRegex(ValueError,'audit marker'):
                encode_mesh(vertices,faces)

    def test_reordered_triangles_keep_multiplicity_and_winding(self):
        vertices = [Vertex((i/37,(i%3)/7,0),uv=(i/128,0)) for i in range(99)]
        faces = [[i,i+1,i+2] for i in range(97)]
        audit = {}
        payload,count,_ = encode_mesh(vertices,faces,audit=audit)
        inspect_mesh(payload,count,vertices,faces[::-1],source_maps=audit['source_maps'])
        for wrong in [[[t[0],t[2],t[1]] for t in faces], faces[:-1]+[faces[0]]]:
            with self.assertRaisesRegex(ValueError,'triangle'):
                inspect_mesh(payload,count,vertices,wrong,source_maps=audit['source_maps'])

    def test_missing_meshoptimizer_runtime_has_no_greedy_fallback(self):
        from unittest.mock import patch
        vertices = [Vertex((i/32,0,0)) for i in range(99)]
        faces = [[i,i+1,i+2] for i in range(97)]
        with patch.dict('os.environ',{'NODE_EXE':'missing-meshoptimizer-node-executable'}):
            with self.assertRaisesRegex(RuntimeError,'Node.js'):
                encode_mesh(vertices,faces)


if __name__ == '__main__':
    unittest.main()
