"""Shared snapshot backend checks with the actual native simulator binary."""
from pathlib import Path
from types import SimpleNamespace
import json
import os
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import simulator
import replay
from snapshot import Abi, SnapshotError, load_snapshot, decode_stream, encode_reference
from draw_replay import synthetic_snapshot
from test_draw_oracle import SyntheticCapture
from layout import COLUMN


class SeedTests(unittest.TestCase):
    def test_physical_aliases_preserve_write_facts_and_reject_conflicting_payloads(self):
        regions = [SimpleNamespace(index=i, address=128, length=2, flags=flags) for i, flags in enumerate((1, 2))]
        capture = SimpleNamespace(ranges=regions, data={0: b'\x12\x34', 1: b'\x12\x34'}, abi={'WRITE': 2})
        memory, access = simulator.seed(capture)
        self.assertEqual(memory[128:130], b'\x12\x34')
        self.assertEqual(access[128:130], b'\3\3')
        self.assertEqual(access[130], 0)
        capture.data[1] = b'\x56\x78'
        with self.assertRaisesRegex(SnapshotError, 'Conflicting captured physical alias'):
            simulator.seed(capture)


class NativeBackendTests(unittest.TestCase):
    def setUp(self):
        try:
            self.executable = simulator.executable_path(os.environ.get('GPU_SIM_EXE'))
        except SnapshotError as error:
            self.skipTest(str(error))
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def test_shared_replay_entry_returns_only_final_results_and_preserves_raw_rtl_reference(self):
        capture = (load_snapshot(Path(os.environ['GPU_SIM_RTL_REFERENCE']).parent/'snapshot', Abi())
                   if os.environ.get('GPU_SIM_RTL_REFERENCE') else synthetic_snapshot(COLUMN))
        capture.save(self.root/'input')
        result = replay.run(self.root/'input', None, self.root/'result', None, 30,
                            backend='sim', simulator=self.executable)
        self.assertTrue(result['complete'])
        self.assertEqual(result['backend'], 'sim')
        self.assertEqual(result['effects']['target'], [capture.header['color_base'], capture.header['depth_base']])
        self.assertIsNotNone(result['effects']['written_bytes'])
        self.assertEqual(result['input_sha256'], capture.manifest()['input_sha256'])
        for forbidden in ('cold_submission_cycles', 'clock_hz', 'metric_defs', 'tracks', 'commands'):
            self.assertNotIn(forbidden, result)
        self.assertFalse((self.root/'result/profile.json').exists())
        self.assertIn('digitaldesign.gpu-result', (self.root/'result/profiler.html').read_text())
        if os.environ.get('GPU_SIM_RTL_REFERENCE'):
            reference = Path(os.environ['GPU_SIM_RTL_REFERENCE'])
            manifest = json.loads((reference/'replay-manifest.json').read_text())
            self.assertEqual(manifest['input_sha256'], result['input_sha256'])
            for role in ('color', 'depth'):
                self.assertEqual((self.root/f'result/capture/{role}.bin').read_bytes(),
                                 (reference/f'capture/{role}.bin').read_bytes())

    def test_forbidden_framebuffer_write_is_not_published_as_a_successful_frame(self):
        capture = synthetic_snapshot(COLUMN)
        for region in capture.ranges:
            if region.address == capture.header['color_base']:
                object.__setattr__(region, 'flags', 1)
        with self.assertRaisesRegex(SnapshotError, 'writable bytes'):
            simulator.run(capture, self.root/'failure', {}, executable=self.executable, timeout=30)
        self.assertFalse((self.root/'failure/result.json').exists())
        self.assertFalse((self.root/'failure/profiler.html').exists())
        self.assertFalse(json.loads((self.root/'failure/failure.json').read_text())['complete'])

    def test_uncaptured_read_never_falls_back_to_valid_dense_bytes(self):
        capture = synthetic_snapshot(COLUMN)
        memory, access = simulator.seed(capture)
        address = capture.header['cb_base']
        self.assertTrue(access[address] & 1)
        access[address] = 0
        with patch.object(simulator, 'seed', return_value=(memory, access)):
            with self.assertRaisesRegex(SnapshotError, 'uncaptured'):
                simulator.run(capture, self.root/'missing-read', {}, executable=self.executable, timeout=30)
        self.assertFalse((self.root/'missing-read/result.json').exists())
        self.assertFalse((self.root/'missing-read/profiler.html').exists())
        self.assertFalse(json.loads((self.root/'missing-read/failure.json').read_text())['complete'])

    def test_work_limit_is_explicit_and_bounded(self):
        capture = synthetic_snapshot(COLUMN)
        with self.assertRaisesRegex(SnapshotError, 'work limit'):
            simulator.run(capture, self.root/'bounded', {}, executable=self.executable, maximum_work=1, timeout=30)
        self.assertFalse((self.root/'bounded/result.json').exists())

    def test_error_preserves_prior_writes_without_publishing_a_completed_frame(self):
        source = SyntheticCapture(COLUMN)
        source.commands[12] = 0xd0000000  # Reserved word after the completed color CLEAR.
        source.update_commands()
        capture = synthetic_snapshot(COLUMN, source)
        with self.assertRaisesRegex(SnapshotError, 'reserved instruction'):
            simulator.run(capture, self.root/'partial', {}, executable=self.executable, timeout=30)
        failure = json.loads((self.root/'partial/failure.json').read_text())
        memory = (self.root/'partial'/failure['partial_memory']).read_bytes()
        base = capture.header['color_base']
        self.assertEqual(memory[base:base+COLUMN.plane_bytes], b'\x34\x12'*(COLUMN.plane_bytes//2))
        self.assertFalse(failure['complete'])
        completion = json.loads((self.root/'partial'/failure['native_completion']).read_text())
        self.assertFalse(completion['complete'])
        self.assertEqual(completion['target'], [capture.header['color_base'], capture.header['depth_base']])
        self.assertEqual(completion['written_bytes'], [base, base+COLUMN.plane_bytes])
        self.assertFalse((self.root/'partial/result.json').exists())


@unittest.skipUnless(os.environ.get('GPU_API_CAPTURE_FIXTURE_DIR'), 'requires retained producer packets')
class RecordedPacketTests(unittest.TestCase):
    setUp = NativeBackendTests.setUp
    # Preserve the retained packet identity; these are not newly generated CPU captures.
    def simulate(self, case):
        source = json.loads((Path(os.environ['GPU_API_CAPTURE_FIXTURE_DIR'])/(case+'-false.json')).read_text())
        abi = Abi()
        rows = [source['ranges'][i:i+8] for i in range(0, len(source['ranges']), 8)]
        attempts, report = decode_stream(encode_reference(source['header'], rows, [bytes(b) for b in source['blobs']], abi), abi)
        self.assertFalse(report['failures'])
        capture = attempts[0]
        capture.save(self.root/'input')
        result = replay.run(self.root/'input', None, self.root/'result', None, 30, backend='sim', simulator=self.executable)
        return capture, result, self.root/'result'

    def test_retained_no_target_copy(self):
        capture, result, out = self.simulate('memcpy')
        self.assertFalse(result['has_target'])
        writes = [row for row in result['outputs'] if row['role'] == 'write-range']
        self.assertEqual(len(writes), 1)
        self.assertEqual((out/writes[0]['file']).read_bytes(), capture.read(0x300000, 1024))

    def test_retained_short_atlas_and_multiple_targets(self):
        for case, color, bases in [('ui_short', 0x07e0, (0x200000,)), ('ui_load_multi_target', 0xf800, (0x180000, 0x200000))]:
            with self.subTest(case=case):
                # Each replay requires a fresh output root.
                self.root = Path(self.temporary.name)/case
                self.root.mkdir()
                capture, result, out = self.simulate(case)
                writes = {row['address']: out/row['file'] for row in result['outputs'] if row['role'] == 'write-range'}
                for base in bases:
                    expected = bytearray(capture.read(base, COLUMN.plane_bytes))
                    for y in range(32, 48):
                        for x in range(224, 232):
                            at = COLUMN.offset(x, y)
                            expected[at:at+2] = color.to_bytes(2, 'little')
                    self.assertEqual(writes[base].read_bytes(), expected)
                for base in ((0x280000,) if case == 'ui_short' else (0x340000, 0x280000)):
                    self.assertEqual(writes[base].read_bytes(), capture.read(base, COLUMN.plane_bytes))
