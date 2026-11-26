"""Source mip isolation, geometry preservation and reproducible atlas candidates."""
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

from opaque_atlas import build, layout, pixels, unpack, pack_levels


class OpaqueAtlas(unittest.TestCase):
    def test_source_levels_and_clamped_cell_edges_are_preserved(self):
        source = {i: [0xf800 + i] * (1 << (2 * i)) for i in range(7)}
        sources = {'red': dict(size=64, levels=source)}
        regions = layout(sources, 256)
        image = pixels(sources, regions, 256)
        decoded = unpack(pack_levels(image), 256)
        self.assertEqual(decoded, image)
        for lod in range(7):
            side = 256 >> lod
            cell = 128 >> lod
            # Even the deepest source 1x1 is replicated over its complete 2x2 cell.
            self.assertTrue(all(image[8-lod][y * side + x] == 0xf806-lod
                                for y in range(cell) for x in range(cell)))

    def test_layout_rejects_overflow_without_aliasing_cells(self):
        sources = {str(i): dict(size=64) for i in range(5)}
        with self.assertRaisesRegex(ValueError, 'do not fit'):
            layout(sources, 256)

    def test_conversion_preserves_positions_colour_vertices_and_source_archive(self):
        texture = pack_levels({i: [0x7bef] * (1 << (2*i)) for i in range(7)})
        scene = dict(version=1, materials=[dict(texture='t.bin', size=64, encoding='gpu-column16',
                     mipmaps=True, clamp=True, tint=[120,180,200,255]),dict(tint=[255]*4)], groups=[
            dict(material=0,vertices=[[0,0,0,0,0],[1,0,0,1,0],[0,1,0,0,1]],triangles=[[0,1,2]]),
            dict(material=1,vertices=[[0,0,1,10,20,30],[1,0,1,40,50,60],[0,1,1,70,80,90]],triangles=[[0,1,2]])])
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root/'source.zip'
            with zipfile.ZipFile(source,'w') as z:
                z.writestr('scene.json',json.dumps(scene))
                z.writestr('t.bin',texture)
            original = source.read_bytes()
            build(source,root/'first',256)
            build(source,root/'second',256)
            self.assertEqual((root/'first/scene.zip').read_bytes(),(root/'second/scene.zip').read_bytes())
            self.assertEqual(source.read_bytes(),original)
            with zipfile.ZipFile(root/'first/scene.zip') as z:
                converted=json.loads(z.read('scene.json'))
                self.assertEqual(converted['groups'][1],scene['groups'][1])
                self.assertTrue(converted['materials'][1]['vertex_color'])
                for old,new in zip(scene['groups'][0]['vertices'],converted['groups'][0]['vertices']):
                    self.assertEqual(old[:3],new[:3])
                    self.assertEqual(new[3:],[(32+v*64)/256 for v in old[3:]])
                self.assertNotIn('t.bin',z.namelist())
                self.assertEqual(converted['opaque_atlas']['regions']['t.bin']['isolated_mip_through'],6)


if __name__ == '__main__':
    unittest.main()
