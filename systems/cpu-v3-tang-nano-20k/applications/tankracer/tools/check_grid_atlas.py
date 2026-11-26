"""Fixed-grid source mip choice, half-open UV ownership and repeatable conversion."""
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

from grid_atlas import build
from opaque_atlas import pack_levels, unpack


class GridAtlas(unittest.TestCase):
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
