"""Bound production CLI/report paths; synthetic bytes, no RTL or browser."""
import contextlib
import io
import json
from pathlib import Path
import shutil
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import images
import profiler
import replay
import reporting as r
from snapshot import SnapshotError
import test_identity


class ReportingTests(unittest.TestCase):
    def setUp(self):
        scratch = ROOT.parents[3]/'target/reporting-tests'
        scratch.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=scratch)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root/'capture'
        self.source.mkdir()
        helper = test_identity.IdentityTests()
        helper.abi = replay.Abi(replay.DEFAULT_ABI)
        self.capture, _, _ = helper.create(self.root)
        self.capture.descriptor[self.capture.abi['H_TARGET_FORMAT']] = 0
        self.capture.header['target_format'] = 0
        for region in self.capture.ranges:
            if region.tag == self.capture.abi['TAG_COLOR']:
                self.capture.data[region.index][:2] = struct.pack('<H', 0x8410)
        self.snapshot = self.root/'snapshot'
        self.capture.save(self.snapshot)
        replay.seed_files(self.capture, self.source)
        words = struct.unpack('<4I', self.capture.read(self.capture.header['cb_base'], 16))
        events = ['FORMAT 2', f'SUBMIT 10 1 {self.capture.header["cb_base"]} 16',
                  f'IMAGE_TARGET 10 1 {self.capture.header["color_base"]} {self.capture.header["depth_base"]} 1',
                  'MEM_REQUEST 10 0 2490368 0 1',
                  *[f'MEM_DATA {cycle} 0 255 0' for cycle in range(11, 15)], 'MEM_COMPLETE 15 0 0']
        for i, word in enumerate(words):
            start = 10+i*2
            events += [f'CMD_EXEC {start+1} {i*4} {word:08x}',
                       f'CMD_END {start+2} {i*4} {start} {start+1} 0 {word:08x}']
        events += ['DONE 18 1 0 0 0 0 0', 'CAPTURE_END 18']
        (self.source/'events.txt').write_text('\n'.join(events)+'\n')
        header = (ROOT/'tests/fixtures/pingpong/load.csv').read_text().splitlines()[0]
        (self.source/'load.csv').write_text(header+'\n'+'\n'.join(
            str(cycle)+',0'*(len(header.split(','))-1) for cycle in range(10, 19))+'\n')
        metadata = dict(version=1, synthetic=True, clock_hz=54_000_000,
                        input_sha256=self.capture.manifest()['input_sha256'],
                        inputs_sha256={name:r.digest(self.source/name) for name in r.SEEDS})
        (self.source/'capture.json').write_text(json.dumps(metadata))
        for role in ('color', 'depth'):
            blob = self.capture.read(self.capture.header[role+'_base'], self.capture.header['plane_bytes'])
            (self.source/(role+'.bin')).write_bytes(blob)

    def build(self, name='report', **kwargs):
        return r.build_report(self.source, self.root/name, snapshot=self.snapshot, require_images=True, **kwargs)

    def test_complete_cli_default_display1_saved_inline_and_raw_preserved(self):
        before = {p.name:r.digest(p) for p in self.source.iterdir()}
        out = self.root/'cli'
        with patch.object(sys, 'argv', ['profiler.py', 'build', str(self.source), '--snapshot',
                                       str(self.snapshot), '--out', str(out), '--require-images']), \
             contextlib.redirect_stdout(io.StringIO()) as stream:
            profiler.main()
        summary = json.loads(stream.getvalue())
        self.assertEqual(summary['image_count'], 5)
        data = json.loads((out/'profile.json').read_text())
        self.assertFalse(data['images']['final']['preview']['capture_format_known'])
        self.assertEqual(data['images']['final']['format'], 0)
        self.assertEqual(data['images']['final']['preview']['preview_policy'], 'display1')
        self.assertIn(data['images']['final']['src'], (out/'profiler.html').read_text())
        self.assertEqual(before, {p.name:r.digest(p) for p in self.source.iterdir()})
        self.assertEqual(len(list((out/'images').glob('*.png'))), 5)

    def test_binding_repeat_and_render_copy_exact_png_without_second_lut(self):
        self.build()
        first = self.root/'report'
        r.build_report(self.source, self.root/'again', binding=first/'report-inputs.json', require_images=True)
        with patch.object(images, 'display_reference', side_effect=AssertionError('No second conversion')):
            r.render_report(first/'profile.json', self.root/'render', require_images=True)
        for png in (first/'images').glob('*.png'):
            self.assertEqual(png.read_bytes(), (self.root/'again/images'/png.name).read_bytes())
            self.assertEqual(png.read_bytes(), (self.root/'render/images'/png.name).read_bytes())

    def test_explicit_relocated_dependencies_rebuild_with_the_same_binding(self):
        self.build()
        moved = self.root/'relocated'
        shutil.copytree(self.source, moved/'capture')
        shutil.copytree(self.snapshot, moved/'snapshot')
        out = self.root/'relocated-report'
        with patch.object(sys, 'argv', ['profiler.py', 'build', str(moved/'capture'),
             '--snapshot', str(moved/'snapshot'), '--raw', str(moved/'capture'),
             '--binding', str(self.root/'report/report-inputs.json'), '--out', str(out), '--require-images']), \
             contextlib.redirect_stdout(io.StringIO()):
            profiler.main()
        self.assertEqual((out/'images/final.png').read_bytes(), (self.root/'report/images/final.png').read_bytes())
        record = json.loads((out/'report-inputs.json').read_text())
        self.assertEqual(Path(record['snapshot']['directory']), moved/'snapshot')
        self.assertEqual(Path(record['raw']['directory']), moved/'capture')

    def test_raw_policy_unknown_capture_field_and_unknown_modes(self):
        display = self.build()
        raw = self.build('raw', preview='raw')
        auto = self.build('capture-policy', preview='capture')
        self.assertEqual(raw['images']['final']['png_sha256'], auto['images']['final']['png_sha256'])
        self.assertNotEqual(raw['images']['final']['png_sha256'], display['images']['final']['png_sha256'])
        for kwargs in [dict(preview='mystery'), dict(capture_format='guessed')]:
            with self.assertRaises(SnapshotError): self.build('bad-mode', **kwargs)
        with self.assertRaises(SnapshotError): self.build()

    def test_timing_only_is_explicit_and_strict_mode_refuses(self):
        fixture = ROOT/'tests/fixtures/pingpong'
        data = r.build_report(fixture, self.root/'timing')
        self.assertFalse(data['images']['available'])
        self.assertIn('timing-only', data['images']['reason'])
        with self.assertRaisesRegex(SnapshotError, 'No frozen snapshot'):
            r.build_report(fixture, self.root/'strict', require_images=True)
        self.assertFalse((self.root/'strict/profiler.html').exists())

    def test_partial_diagnostics_keep_explicit_unavailable_images(self):
        events = self.source/'events.txt'
        events.write_text(events.read_text().replace('CAPTURE_END 18', 'BOUNDED_STOP 18'))
        data = r.build_report(self.source, self.root/'partial', snapshot=self.snapshot, allow_partial=True)
        self.assertTrue(data['partial'])
        self.assertFalse(data['images']['available'])
        self.assertIn('Incomplete timing capture', data['images']['reason'])

    def test_generated_report_replacement_archives_exact_files_and_refuses_unknown_content(self):
        self.build()
        out = self.root/'report'
        hashes = {str(p.relative_to(out)):r.digest(p) for p in out.rglob('*') if p.is_file()}
        (out/'personal.txt').write_text('Keep this file')
        with self.assertRaisesRegex(SnapshotError, 'unknown report content'):
            self.build(replace_report=True)
        self.assertEqual((out/'personal.txt').read_text(), 'Keep this file')
        (out/'personal.txt').unlink()
        with self.assertRaisesRegex(SnapshotError, 'Unknown preview'):
            self.build(replace_report=True, preview='invalid')
        self.assertEqual(hashes, {str(p.relative_to(out)):r.digest(p) for p in out.rglob('*') if p.is_file()})
        self.build(replace_report=True)
        mapping = json.loads((out/'report-replacement.json').read_text())
        archive = Path(mapping['archive_directory'])
        self.assertEqual(archive.parent, out.parent)
        self.assertEqual(mapping['files'], hashes)
        self.assertEqual(hashes, {str(p.relative_to(archive)):r.digest(p) for p in archive.rglob('*') if p.is_file()})
        self.assertTrue((out/'profiler.html').exists())

    def test_wrong_seed_snapshot_trace_raw_and_sidecar_are_refused(self):
        self.build()
        binding = self.root/'report/report-inputs.json'
        for name in ['memory.hex', 'events.txt', 'load.csv', 'color.bin', 'depth.bin']:
            path = self.source/name
            original = path.read_bytes()
            path.write_bytes(original+b'bad')
            try:
                with self.assertRaises((SnapshotError, ValueError)):
                    r.build_report(self.source, self.root/'bad', binding=binding, require_images=True)
            finally: path.write_bytes(original)
        snapshot_json = self.snapshot/'snapshot.json'
        original = snapshot_json.read_bytes()
        snapshot_json.write_bytes(original+b' ')
        with self.assertRaises(SnapshotError):
            r.build_report(self.source, self.root/'wrong-snapshot', binding=binding, require_images=True)
        snapshot_json.write_bytes(original)
        record = json.loads(binding.read_text())
        record['display_reference']['table_sha256'] = '0'*64
        bad = self.root/'bad-binding.json'
        bad.write_text(json.dumps(record))
        with self.assertRaisesRegex(SnapshotError, 'LUT dependency'):
            r.build_report(self.source, self.root/'wrong-lut', binding=bad, require_images=True)
        # Without a previous binding, raw content is still checked against history.
        for role in ('color', 'depth'):
            path = self.source/(role+'.bin')
            original = path.read_bytes()
            path.write_bytes(bytes([original[0]^1])+original[1:])
            with self.assertRaisesRegex(SnapshotError, 'differs from same-run raw'):
                self.build('wrong-'+role)
            self.assertFalse((self.root/('wrong-'+role)/'profiler.html').exists())
            path.write_bytes(original)

    def test_unbound_wrong_snapshot_seed_and_command_stream_are_refused(self):
        seed = self.source/'memory.hex'
        original = seed.read_bytes()
        seed.write_bytes(original.replace(b'8410', b'8411', 1))
        metadata_path = self.source/'capture.json'
        metadata = json.loads(metadata_path.read_text())
        metadata['inputs_sha256']['memory.hex'] = r.digest(seed)
        metadata_path.write_text(json.dumps(metadata))
        with self.assertRaisesRegex(SnapshotError, 'Snapshot seed'):
            self.build('bad-seed')
        seed.write_bytes(original)
        metadata['inputs_sha256']['memory.hex'] = r.digest(seed)
        metadata_path.write_text(json.dumps(metadata))
        events = self.source/'events.txt'
        original = events.read_text()
        events.write_text(original.replace('00000000', '00000001'))
        with self.assertRaisesRegex(SnapshotError, 'Trace commands differ'):
            self.build('bad-command')
        events.write_text(original)
        metadata['input_sha256'] = 'f'*64
        metadata_path.write_text(json.dumps(metadata))
        with self.assertRaisesRegex(SnapshotError, 'Snapshot input identity differs'):
            self.build('bad-snapshot')

    def test_strict_sim_receive_is_refused_before_a_physical_port_is_opened(self):
        import snapshot_cli
        with patch('snapshot_cli.receive_uart') as receive, patch.object(sys, 'argv',
             ['snapshot_cli.py', 'receive', '--port', 'TEST-NOT-A-DEVICE', '--seconds', '1',
              '--out', str(self.root/'never-opened'), '--backend', 'sim', '--require-images']):
            with self.assertRaisesRegex(SnapshotError, 'needs automatic RTL'):
                snapshot_cli.main()
        receive.assert_not_called()

    def test_capture_and_replay_use_shared_report_path_without_extra_run(self):
        out = self.root/'fresh'
        def captured(_manifest, destination, *_args):
            shutil.copytree(self.source, destination)
        with patch('capture.run', side_effect=captured) as runner, \
             patch.object(sys, 'argv', ['profiler.py', 'capture', str(self.root/'manifest.json'),
                '--out', str(out), '--snapshot', str(self.snapshot), '--require-images']), \
             contextlib.redirect_stdout(io.StringIO()):
            profiler.main()
        runner.assert_called_once()
        self.assertTrue((out/'profile/images/final.png').exists())
        replay_out = self.root/'replay'
        replay_out.mkdir()
        self.capture.save(replay_out/'snapshot')
        manifest = replay_out/'replay-manifest.json'
        manifest.write_text(json.dumps(dict(command_abi=self.capture.header['command_abi'],
                                            captured_plane_bytes=self.capture.header['plane_bytes'])))
        def replayed(_manifest, destination):
            captured(_manifest, destination)
            (destination/'run.log').write_text('REPLAY_COMPLETE cycles=18 initialized_reads=checked\n')
            for role in ('color', 'depth'):
                shutil.copyfile(destination/(role+'.bin'), destination/(role+'-allocation.bin'))
        with patch('replay.capture_run', side_effect=replayed) as runner:
            result = replay.execute(self.capture, manifest, require_images=True)
        runner.assert_called_once()
        self.assertTrue(result['command_images']['available'])
        self.assertEqual((replay_out/'color.png').read_bytes(), (replay_out/'images/final.png').read_bytes())


if __name__ == '__main__':
    unittest.main()
