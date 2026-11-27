"""Fixed-grid source mip choice, half-open UV ownership and repeatable conversion."""
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

from grid_atlas import build
from opaque_atlas import pack_levels, unpack


class GridAtlas(unittest.TestCase):
    def test_default_atlas_can_be_regenerated_from_the_archived_sources(self):
        assets = Path(__file__).resolve().parents[1]/'assets/england'
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)/'generated'
            build(assets/'scene-compact.zip', output, 512, 3, native_cells=True,
                  embed_rgba='textures/IGATE_ALPHA.bin', move_alpha_source='SCARE')
            with zipfile.ZipFile(output/'scene.zip') as generated, \
                 zipfile.ZipFile(assets/'scene-grid-atlas.zip') as checked_in:
                self.assertEqual(generated.namelist(), checked_in.namelist())
                for name in generated.namelist():
                    self.assertEqual(generated.read(name), checked_in.read(name), name)

    def test_rgba_view_has_its_own_grid_and_one_texel_tail(self):
        from texture_source import encode_pixels
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root/'source.zip'
            rgba = [(255,0,0,255) if x&1 else (0,255,0,0)
                    for y in range(128) for x in range(128)]
            rgba_levels = unpack(encode_pixels(128, rgba, 'Rgba4444', mipmaps=True), 128)
            scene = dict(materials=[
                dict(texture='rgb', size=64, encoding='gpu-column16', mipmaps=True, clamp=True),
                dict(texture='rgba', size=128, encoding='gpu-column16', mipmaps=False, clamp=True,
                     rgba=True, blend=True)], groups=[
                    dict(material=1, vertices=[[0,0,0,0,0],[1,0,0,1,0],[0,1,0,0,1]], triangles=[[0,1,2]])])
            with zipfile.ZipFile(source, 'w') as z:
                z.writestr('scene.json', json.dumps(scene))
                z.writestr('rgb', pack_levels({e: [0x1234]*(1 << (2*e)) for e in range(7)}))
                z.writestr('rgba', pack_levels({7: rgba_levels[7]}))
            build(source, root/'out', 256, 2, native_cells=True, embed_rgba='rgba')
            with zipfile.ZipFile(root/'out/scene.zip') as z:
                result = json.loads(z.read('scene.json'))
                levels = unpack(z.read('textures/OPAQUE_GRID.bin'), 256)
                self.assertNotIn('rgba', z.namelist())
            self.assertEqual([m['atlas_grid_log2'] for m in result['materials']], [2,1])
            self.assertEqual(result['materials'][0]['texture'], result['materials'][1]['texture'])
            r = result['opaque_atlas']['regions']['rgba']
            for e in range(1,9):
                lod = 8-e
                tile = 128 >> lod
                expected = rgba_levels[tile.bit_length()-1]
                x, y = r['x'] >> lod, r['y'] >> lod
                for row in range(tile):
                    self.assertEqual(levels[e][(y+row)*(1 << e)+x:(y+row)*(1 << e)+x+tile],
                                     expected[row*tile:(row+1)*tile])

    def test_native_cells_preserve_large_source_and_its_two_texel_tail(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root/'original.zip'
            scene = dict(version=1, materials=[
                dict(texture=name, size=size, encoding='gpu-column16', mipmaps=True, clamp=True)
                for name, size in [('small.bin', 64), ('large.bin', 128)]], groups=[
                    dict(material=1, vertices=[[0,0,0,0,0], [1,0,0,1,0], [0,1,0,0,1]],
                         triangles=[[0,1,2]])])
            native = {}
            with zipfile.ZipFile(source, 'w') as z:
                z.writestr('scene.json', json.dumps(scene))
                for name, size, base in [('small.bin', 64, 0x1000), ('large.bin', 128, 0x2000)]:
                    levels = {e: [base + e*512 + y*16+x
                                  for y in range(1 << e) for x in range(1 << e)]
                              for e in range(size.bit_length())}
                    native[name] = levels
                    z.writestr(name, pack_levels(levels))
            original = source.read_bytes()
            build(source, root/'a', 256, 2, native_cells=True)
            build(source, root/'b', 256, 2, native_cells=True)
            self.assertEqual(source.read_bytes(), original)
            self.assertEqual((root/'a/scene.zip').read_bytes(), (root/'b/scene.zip').read_bytes())
            with zipfile.ZipFile(root/'a/scene.zip') as z:
                result = json.loads(z.read('scene.json'))
                levels = unpack(z.read('textures/OPAQUE_GRID.bin'), 256)
            audit = result['opaque_atlas']
            self.assertEqual((audit['used_cells'], audit['empty_cells']), (5, 11))
            self.assertEqual([m['atlas_grid_log2'] for m in result['materials']], [2, 2])
            used = set()
            for name, region in audit['regions'].items():
                pitch = region['cell']
                footprint = {(region['x']+x, region['y']+y)
                             for y in range(0, pitch, 64) for x in range(0, pitch, 64)}
                self.assertFalse(footprint & used)
                used.update(footprint)
                self.assertEqual(pitch, region['size'])
                for e in range(2, 9):
                    lod = 8-e
                    tile = pitch >> lod
                    expected = native[name][tile.bit_length()-1]
                    x, y = region['x'] >> lod, region['y'] >> lod
                    for row in range(tile):
                        self.assertEqual(levels[e][(y+row)*(1 << e)+x:(y+row)*(1 << e)+x+tile],
                                         expected[row*tile:(row+1)*tile])
            self.assertEqual(audit['regions']['large.bin']['cell'] >> (8-2), 2)
            large = audit['regions']['large.bin']
            for vertex in result['groups'][0]['vertices']:
                for axis, origin in enumerate((large['x'], large['y'])):
                    self.assertGreaterEqual(vertex[axis+3], origin/256+2/65536)
                    self.assertLessEqual(vertex[axis+3], (origin+128)/256-2/65536)
            with self.assertRaisesRegex(ValueError, 'do not fit'):
                build(source, root/'too-small', 128, 1, native_cells=True)

    def test_native_mip_selection_uv_guard_and_source_retention(self):
        for side in (128, 256):
            with self.subTest(side=side), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = root/'original.zip'
                scene = dict(version=1, materials=[
                    dict(texture=name, size=size, encoding='gpu-column16', mipmaps=True, clamp=True)
                    for name,size in [('small.bin',64),('large.bin',128)]], groups=[
                        dict(material=0,vertices=[[0,0,0,0,0],[1,0,0,1,0],[0,1,0,0,1]],triangles=[[0,1,2]])])
                with zipfile.ZipFile(source,'w') as z:
                    z.writestr('scene.json',json.dumps(scene))
                    for name,size,base in [('small.bin',64,0x1000),('large.bin',128,0x2000)]:
                        z.writestr(name,pack_levels({i:[base+i]*(1 << (2*i)) for i in range(size.bit_length())}))
                original = source.read_bytes()
                build(source,root/'a',side,1)
                build(source,root/'b',side,1)
                self.assertEqual(source.read_bytes(),original)
                self.assertEqual((root/'a/scene.zip').read_bytes(),(root/'b/scene.zip').read_bytes())
                with zipfile.ZipFile(root/'a/scene.zip') as z:
                    result=json.loads(z.read('scene.json'))
                    levels=unpack(z.read('textures/OPAQUE_GRID.bin'),side)
                self.assertEqual([m['atlas_grid_log2'] for m in result['materials']],[1,1])
                region=result['opaque_atlas']['regions']['small.bin']
                for old,new in zip(scene['groups'][0]['vertices'],result['groups'][0]['vertices']):
                    self.assertEqual(old[:3],new[:3])
                    for axis,origin in enumerate((region['x'],region['y'])):
                        self.assertGreaterEqual(new[axis+3],origin/side+2/65536)
                        self.assertLessEqual(new[axis+3],(origin+region['cell'])/side-2/65536)
                for name,region in result['opaque_atlas']['regions'].items():
                    for exponent in range(1,side.bit_length()):
                        cell_side=(1 << exponent)//2
                        x=region['x']*(1 << exponent)//side
                        y=region['y']*(1 << exponent)//side
                        expected=(0x1000 if name=='small.bin' else 0x2000)+min(cell_side.bit_length()-1,region['size'].bit_length()-1)
                        for row in range(cell_side):
                            self.assertEqual(levels[exponent][(y+row)*(1 << exponent)+x:(y+row)*(1 << exponent)+x+cell_side],[expected]*cell_side)


if __name__ == '__main__':
    unittest.main()
