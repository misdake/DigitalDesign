"""Column producer-catalog handoff; no real firmware or packing qualification."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from layout import ROW,COLUMN
from identity import create_build

source=Path(__file__).resolve().parents[3]/'examples/gpu_profiler_board/tools/assets.py'
spec=importlib.util.spec_from_file_location('diagnostic_asset_consumer',source)
consumer=importlib.util.module_from_spec(spec); spec.loader.exec_module(consumer)


class AssetLayoutTests(unittest.TestCase):
    def catalog(self,root):
        root.mkdir(); blob=bytes(range(256))*2
        (root/'texture.bin').write_bytes(blob)
        (root/'renderer_assets.rs').write_text('// Synthetic catalog consumer test, not a firmware program.\n')
        manifest=dict(capture_layout=COLUMN.description(),resources=[dict(
            file='texture.bin',address=0x4e0000,bytes=512,kind='texture',
            layout=COLUMN.texture_layout,sha256=hashlib.sha256(blob).hexdigest())])
        (root/'assets.json').write_text(json.dumps(manifest))
        return manifest

    def test_explicit_catalog_and_bound_rebuild_preserve_bytes(self):
        with tempfile.TemporaryDirectory() as temp,patch.dict(os.environ,{},clear=True):
            root=Path(temp); original=root/'producer'; self.catalog(original)
            prepared=root/'prepared'
            consumer.pack(prepared,False,False,COLUMN,original)
            self.assertEqual((original/'texture.bin').read_bytes(),(prepared/'texture.bin').read_bytes())
            text=(prepared/'renderer_assets.rs').read_text()
            self.assertEqual(text.count('GPU_PROFILER_AUTOMATIC_TEST'),1)
            resource=json.loads((prepared/'assets.json').read_text())['resources'][0]
            specification=dict(layout=COLUMN.description(),files=[dict(name='assets/'+p.name,path=str(p))
                for p in sorted(prepared.iterdir())],resources=[dict(name=resource['file'],**{
                k:v for k,v in resource.items() if k!='file'})],
                options=dict(automatic_test=False,full_scene=False),toolchain={},source_commit='synthetic')
            (root/'spec.json').write_text(json.dumps(specification)); create_build(root/'spec.json',root/'build.json')
            with patch.dict(os.environ,{'GPU_PROFILER_MANIFEST':str(root/'build.json')}):
                consumer.pack(root/'rebuilt')
            for p in prepared.iterdir(): self.assertEqual(p.read_bytes(),(root/'rebuilt'/p.name).read_bytes())

    def test_row_or_wrong_texture_catalog_cannot_be_relabelled(self):
        for mode in ('framebuffer','texture','hash'):
            with self.subTest(mode=mode),tempfile.TemporaryDirectory() as temp:
                root=Path(temp); original=root/'producer'; manifest=self.catalog(original)
                if mode=='framebuffer': manifest['capture_layout']=ROW.description()
                elif mode=='texture': manifest['resources'][0]['layout']=ROW.texture_layout
                else: (original/'texture.bin').write_bytes(bytes(512))
                (original/'assets.json').write_text(json.dumps(manifest))
                with self.assertRaises(ValueError): consumer.copy_column_catalog(original,root/'out')


if __name__=='__main__': unittest.main()
