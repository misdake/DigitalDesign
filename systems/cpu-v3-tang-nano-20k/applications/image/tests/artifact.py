"""Boot artifact contracts against the actual export, not a parallel compiler."""
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[3]/'tools'))
from application_artifact import Artifact, canonical, digest
import hardware


SOURCE = Path(sys.argv.pop(1)).resolve()
OUTPUT = Path(sys.argv.pop(1)).resolve()
OUTPUT.mkdir(parents=True, exist_ok=True)


class Contracts(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix=self._testMethodName+'-', dir=OUTPUT))
        shutil.copytree(SOURCE, self.root, dirs_exist_ok=True)
        self.path = self.root/'application.build.json'
        self.manifest = json.loads(self.path.read_text(encoding='utf-8'))

    def save(self, reseal=False):
        if reseal:
            self.manifest.pop('artifact_id')
            self.manifest['artifact_id'] = digest(canonical(self.manifest))
        self.path.write_text(json.dumps(self.manifest), encoding='utf-8')

    def test_relocated_sources_and_symbols_are_self_contained(self):
        read_bytes, read_text = Path.read_bytes, Path.read_text
        def local_bytes(path):
            self.assertTrue(path.resolve().is_relative_to(self.root), path)
            return read_bytes(path)
        def local_text(path, *args, **kwargs):
            self.assertTrue(path.resolve().is_relative_to(self.root), path)
            return read_text(path, *args, **kwargs)
        with patch.object(Path, 'read_bytes', local_bytes), patch.object(Path, 'read_text', local_text):
            artifact = Artifact(self.root)
            main = artifact.symbol('main')
            self.assertEqual(main['physical_word'], (main['segment'] << 16) | main['pc'])
            self.assertIn(main['file'], artifact.manifest['source_map'])
            name = next(iter(artifact.globals))
            value = artifact.symbol(name, 'global')
            self.assertEqual(value['physical_word'], (value['dseg_page'] << 14) | (value['logical_word'] & 0x3fff))
            with self.assertRaisesRegex(ValueError, 'unknown function'):
                artifact.symbol('missing.application.symbol')

    def test_changed_code_is_rejected(self):
        path = self.root/self.manifest['code']
        data = bytearray(path.read_bytes()); data[0] ^= 1; path.write_bytes(data)
        with self.assertRaisesRegex(ValueError, 'file identity'):
            Artifact(self.root)

    def test_changed_source_snapshot_is_rejected(self):
        source = next(iter(self.manifest['source_map'].values()))
        path = self.root/source['file']; path.write_bytes(path.read_bytes()+b'\n')
        with self.assertRaisesRegex(ValueError, 'file identity'):
            Artifact(self.root)

    def test_changed_origin_is_rejected(self):
        source = next(s for s in self.manifest['source_map'].values() if s['origin'] is not None)
        source['origin']['location']['path'] = 'wrong-origin.rs'
        self.save()
        with self.assertRaisesRegex(ValueError, 'manifest identity'):
            Artifact(self.root)
        self.save(reseal=True)
        with self.assertRaisesRegex(ValueError, 'source origin'):
            Artifact(self.root)

    def test_package_and_segment_description_must_agree(self):
        self.manifest['target']['code_segment'] ^= 1
        self.save(reseal=True)
        with self.assertRaisesRegex(ValueError, 'package/description command mismatch'):
            Artifact(self.root)

    def test_file_locator_cannot_escape_export(self):
        identity = self.manifest['files'].pop(self.manifest['code'])
        self.manifest['files']['../outside.bin'] = identity
        self.save(reseal=True)
        with self.assertRaisesRegex(ValueError, 'relative file locator'):
            Artifact(self.root)

    def test_listing_matches_executable_even_after_inventory_reseal(self):
        artifact = Artifact(self.root)
        self.assertTrue(artifact.instructions('main'))
        path = self.root/'application.asm'
        import re
        text = path.read_text()
        match = re.search(r'\s+[0-9a-f]+: ([0-9a-f]{4})', text)
        self.assertIsNotNone(match)
        word = f'{int(match[1], 16)^1:04x}'
        path.write_text(text[:match.start(1)]+word+text[match.end(1):])
        raw = path.read_bytes()
        self.manifest['files']['application.asm'] = dict(bytes=len(raw), sha256=digest(raw))
        self.save(reseal=True)
        with self.assertRaisesRegex(ValueError, 'listing/code mismatch'):
            Artifact(self.root).instructions('main')

    def test_unique_calls_bind_real_execute_and_prefixed_return_words(self):
        artifact = Artifact(self.root)
        destinations = {value['pc']: name for name, value in artifact.functions.items()}
        checked = 0
        for caller in artifact.functions:
            instructions = artifact.instructions(caller)
            for instruction in instructions:
                if not instruction['text'].startswith('jalrel '):
                    continue
                target = (instruction['pc']+instruction['words']+int(instruction['text'].split()[1])) & 65535
                if target not in destinations:
                    continue
                count = sum(i['text'].startswith('jalrel ') and
                            (i['pc']+i['words']+int(i['text'].split()[1])) & 65535 == target
                            for i in instructions)
                if count != 1:
                    with self.assertRaisesRegex(ValueError, 'expected one'):
                        artifact.call(caller, destinations[target])
                    continue
                site = artifact.call(caller, destinations[target])
                following = next(i for i in instructions if i['pc'] == site['return_pc'])
                self.assertEqual(site['execute_pc'], instruction['pc']+instruction['words']-1)
                self.assertEqual(site['return_execute_pc'], following['pc']+following['words']-1)
                checked += 1
        self.assertGreater(checked, 0)

    def test_deployment_binds_real_package_and_rejects_changed_image(self):
        fs = self.root/'fixture.fs'
        fs.write_text('// protocol fixture, no physical fit claim\n00000001\n')
        inspection = dict(schema='digitaldesign.gowin-audit', version=1, project='cpu_v3_system',
                          logic_top='CpuV3System', bitstream=str(fs), descriptor_sha256='fixture',
                          flash_capacity_bytes=8*1024*1024)
        out = self.root/'deployment'
        with patch.object(hardware, 'audit', return_value=inspection):
            built = hardware.assemble(self.root, self.root, out, 'unused')
            self.assertEqual(hardware.verify(out, self.root, 'unused')[0]['deployment_id'], built['deployment_id'])
            image = out/'complete-flash.bin'
            raw = bytearray(image.read_bytes()); raw[-1] ^= 1; image.write_bytes(raw)
            with self.assertRaisesRegex(ValueError, 'changed deployment file'):
                hardware.verify(out, self.root, 'unused')


if __name__ == '__main__':
    unittest.main(verbosity=2)
