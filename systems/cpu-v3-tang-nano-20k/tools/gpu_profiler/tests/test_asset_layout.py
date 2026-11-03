"""Catalog/prepare metadata checks; synthetic exports do not qualify hardware."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from layout import COLUMN, COMMAND_ABI
from identity import create_build, load_build

source=Path(__file__).resolve().parents[3]/'examples/gpu_profiler_board/tools/assets.py'
spec=importlib.util.spec_from_file_location('diagnostic_asset_consumer',source)
consumer=importlib.util.module_from_spec(spec); spec.loader.exec_module(consumer)


class AssetLayoutTests(unittest.TestCase):
    def test_standard_full_scene_prepare_produces_typed_catalog(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp); hardware=root/'hardware'; exported=root/'system'; replay=root/'replay'
            marker='// Synthetic layout fixture, never compiled or programmed.\nfunction[8:0]column_ui_tile; clear_sector==1599 wire[11:0]macro_column=\n'
            for folder in [hardware/'src',exported/'src',replay]: folder.mkdir(parents=True)
            for index in range(32):
                name='gpu.v' if index==0 else f'synthetic-{index:02}.txt'
                text=marker if index==0 else f'Identity-only synthetic input {index}\n'
                for folder in [hardware/'src',exported/'src']: (folder/name).write_text(text)
            bitstream=hardware/'impl/pnr/cpu_v3_system.fs'
            bitstream.parent.mkdir(parents=True);bitstream.write_bytes(b'Synthetic identity only')
            (replay/'gpu.v').write_text(marker)
            (replay/'rtl.json').write_text(json.dumps(['gpu.v']))
            (replay/'layout.json').write_text(json.dumps(dict(schema=1,layout_abi=2,gpu_profile='production')))
            primitive=root/'primitive.v';primitive.write_text('// Synthetic identity only\n')
            output=root/'prepared';env=os.environ.copy();env.pop('GPU_PROFILER_MANIFEST',None)
            result=subprocess.run([sys.executable,'-B',str(source.with_name('prepare.py')),
                '--out',str(output),'--hardware',str(hardware),'--system-export',str(exported),
                '--replay-export',str(replay),'--primitive',str(primitive),'--full-scene'],
                env=env,capture_output=True,text=True,timeout=25)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            build=load_build(output/'build.json')
            catalog=json.loads((output/'assets/assets.json').read_text())
            self.assertEqual(catalog['capture_layout'],COLUMN.description())
            self.assertEqual(build['version'],3)
            self.assertEqual(build['layout'],COLUMN.recorded_description())
            self.assertEqual(build['command_abi'],COMMAND_ABI)
            self.assertTrue(build['options']['full_scene'])
            self.assertEqual({r['kind'] for r in build['resources']},{'mesh','texture','atlas'})
            spans=[]
            for resource in build['resources']:
                path=output/'assets'/resource['name']
                self.assertEqual(path.stat().st_size,resource['bytes'])
                self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(),resource['sha256'])
                first=resource['address'];last=first+resource['bytes']
                self.assertEqual(first%128,0);self.assertEqual(resource['bytes']%128,0)
                self.assertLessEqual(COLUMN.description()['memory_map']['assets'][0],first)
                self.assertLessEqual(last,COLUMN.description()['memory_map']['assets'][1])
                if resource['kind']=='texture': self.assertEqual(resource['layout'],COLUMN.texture_layout)
                spans.append((first,last))
            spans.sort();self.assertTrue(all(a[1]<=b[0] for a,b in zip(spans,spans[1:])))

    def test_short_cases_bind_assets_modes_and_native_specialization(self):
        with tempfile.TemporaryDirectory() as temp, patch.dict(os.environ, {}, clear=True):
            root=Path(temp)
            for case in ('rtt','memcpy'):
                out=root/case
                consumer.pack(out, short_case=case)
                catalog=json.loads((out/'assets.json').read_text())
                self.assertEqual(catalog['profiler_short_case'],case)
                self.assertEqual(catalog['capture_layout'],COLUMN.description())
                self.assertIn(f'GPU_PROFILER_SHORT_CASE:u16={1 if case=="rtt" else 2};',
                              (out/'renderer_assets.rs').read_text())
                if case=='rtt': self.assertEqual(catalog['borrowed']['color'],0x600000)
            with self.assertRaises(ValueError): consumer.pack(root/'bad',full_scene=True,short_case='rtt')
            helper=source.with_name('native_sources.py')
            spec=importlib.util.spec_from_file_location('native_diagnostic_test',helper)
            module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
            shared=source.parents[3]/'rcc'
            module.materialize(shared,root/'native')
            identity=json.loads((root/'native/specialization.json').read_text())
            self.assertEqual([m['module'] for m in identity['modules'] if m['changed']],['gpu_capture.rs'])
            first=(root/'native/gpu_capture.rs').read_bytes()
            module.materialize(shared,root/'native')
            self.assertEqual(first,(root/'native/gpu_capture.rs').read_bytes())
            self.assertIn(b'profiling_native_tail(cb)',first)

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
                if mode=='framebuffer': manifest['capture_layout']={**COLUMN.description(),'abi':1}
                elif mode=='texture': manifest['resources'][0]['layout']='pot-row-macro8x8'
                else: (original/'texture.bin').write_bytes(bytes(512))
                (original/'assets.json').write_text(json.dumps(manifest))
                with self.assertRaises(ValueError): consumer.copy_column_catalog(original,root/'out')


if __name__=='__main__': unittest.main()
