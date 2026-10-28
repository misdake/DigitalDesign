"""Synthetic receipt-envelope checks, never actual CPU producer evidence."""
import copy
import hashlib
from pathlib import Path
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from producer_handoff import draw_archive


def envelope(root):
    profile = root/'default'; frame = profile/'frame-0'; frame.mkdir(parents=True)
    for name in ('header.bin', 'ranges.bin', 'commands.bin', 'objects.bin'):
        (frame/name).write_bytes(b'synthetic unit-test envelope')
    (frame/'producer.json').write_text('{"synthetic_test_only":true}')
    (profile/'producer.v3bin').write_bytes(b'not executable CPU code')
    (profile/'build.json').write_text('{"synthetic_test_only":true}')
    digest = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    item = dict(frame_index=0, header_sha256=digest(frame/'header.bin'),
                ranges_sha256=digest(frame/'ranges.bin'), commands_sha256=digest(frame/'commands.bin'),
                objects_sha256=digest(frame/'objects.bin'))
    receipt = dict(version=1, source_commit='unit-test-not-a-commit', profiles=[dict(profile='default',
                   manifest_id='synthetic-not-build-id', layout_command=[1,1],
                   program_sha256=digest(profile/'producer.v3bin'), frames=[item])],
                   evidence_files=[dict(path=p.relative_to(root).as_posix(), bytes=p.stat().st_size,
                                        sha256=digest(p)) for p in root.rglob('*') if p.is_file()])
    return frame, receipt


class DrawArchiveTests(unittest.TestCase):
    def test_full_scene_envelope_retains_real_nested_profile_and_ui(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp); frame,receipt=envelope(root/'full-scene')
            receipt['schema']='digitaldesign.cpu-capture-full-scene-delivery'
            for entry in receipt['evidence_files']:entry['path']='full-scene/'+entry['path']
            (frame/'ui.bin').write_bytes(b'OBS --.- synthetic unit bytes')
            digest=hashlib.sha256((frame/'ui.bin').read_bytes()).hexdigest()
            receipt['evidence_files'].append(dict(path='full-scene/default/frame-0/ui.bin',
                                                  bytes=(frame/'ui.bin').stat().st_size,sha256=digest))
            p=receipt['profiles'][0];p['producer_sha256']=p.pop('program_sha256')
            p['frames'][0]['ui_sha256']=digest
            # Full archives seal header/ranges via evidence_files, not duplicate
            # per-frame digest fields. Do not fabricate a legacy envelope.
            del p['frames'][0]['header_sha256'];del p['frames'][0]['ranges_sha256']
            profile,_=draw_archive(frame,root/'receipt.json',receipt,'unit-test-not-a-commit')
            self.assertEqual(profile,root/'full-scene/default')
            (frame/'ui.bin').write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError,'artifact changed'):
                draw_archive(frame,root/'receipt.json',receipt,'unit-test-not-a-commit')

    def test_sealed_envelope_and_changed_payload(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); frame, receipt = envelope(root)
            profile, entry = draw_archive(frame, root/'receipt.json', receipt, 'unit-test-not-a-commit')
            self.assertEqual(profile, root/'default')
            self.assertEqual(entry['layout_command'], [1,1])
            (frame/'objects.bin').write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'artifact changed'):
                draw_archive(frame, root/'receipt.json', receipt, 'unit-test-not-a-commit')

    def test_wrong_commit_and_unsafe_paths(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); frame, receipt = envelope(root)
            with self.assertRaisesRegex(ValueError, 'requested commit'):
                draw_archive(frame, root/'receipt.json', receipt, 'different')
            receipt['evidence_files'][0]['path'] = '../escape'
            with self.assertRaisesRegex(ValueError, 'Unsafe'):
                draw_archive(frame, root/'receipt.json', receipt, 'unit-test-not-a-commit')

    def test_missing_seal_and_inconsistent_occurrence_digest(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); frame, receipt = envelope(root)
            missing = copy.deepcopy(receipt)
            missing['evidence_files'] = [v for v in missing['evidence_files'] if not v['path'].endswith('/objects.bin')]
            with self.assertRaisesRegex(ValueError, 'required producer inputs'):
                draw_archive(frame, root/'receipt.json', missing, 'unit-test-not-a-commit')
            receipt['profiles'][0]['frames'][0]['header_sha256'] = '0'*64
            with self.assertRaisesRegex(ValueError, 'occurrence digest'):
                draw_archive(frame, root/'receipt.json', receipt, 'unit-test-not-a-commit')


if __name__ == '__main__':
    unittest.main()
