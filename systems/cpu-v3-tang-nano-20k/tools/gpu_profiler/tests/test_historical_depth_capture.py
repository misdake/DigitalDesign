"""Historical import never re-enables retired numerical execution."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from layout import COMMAND_ABI, fixed_far
import snapshot as s
import replay
import simulator
from test_snapshot import fixture


class HistoricalDepthCaptureTests(unittest.TestCase):
    def test_fixed_far_accepts_old_and_current_bytes_and_rejects_retired_tags(self):
        for command in (4, COMMAND_ABI):
            for raw in (1, 255, 65535, 31*65536, 200*65536-1):
                self.assertEqual(fixed_far(raw, command), raw)
            for raw in (0, 200*65536, 0x01010000, 0x02010000, 0x03010000):
                with self.assertRaises(ValueError): fixed_far(raw, command)

    def test_archived_definitions_load_without_changing_metadata_or_hashes(self):
        current = s.Abi()
        for command, name in ((4, 'gpu_capture_abi4.rs'), (6, 'gpu_capture_abi6.rs')):
            source = Path(__file__).with_name('fixtures')/name
            original_source = source.read_bytes()
            for crlf in (False, True):
                with tempfile.TemporaryDirectory() as folder:
                    definition = Path(folder)/'abi.rs'
                    definition.write_bytes(original_source.replace(b'\r\n', b'\n').replace(b'\n', b'\r\n') if crlf else original_source)
                    archived = s.Abi(definition)
                    header, rows, blobs = fixture(archived)
                    header[archived['H_COMMAND_ABI']] = command
                    header[archived['H_RANGE_VERSION']] = 1 if command == 4 else 2
                    if command == 6:
                        rows[1][archived['R_TAG']] = archived['TAG_DEPTH']
                        rows[1][archived['R_FLAGS']] |= 64
                    captures, report = s.decode_stream(s.encode_reference(header, rows, blobs, archived), archived)
                    self.assertFalse(report['failures'])
                    output = Path(folder)/'snapshot'
                    metadata = captures[0].save(output)
                    snapshot_path = output/'snapshot.json'
                    before = snapshot_path.read_bytes()
                    loaded = s.load_snapshot(output, current)
                    self.assertEqual(loaded.manifest()['abi_sha256'], archived.sha256)
                    self.assertEqual(loaded.manifest()['input_sha256'], metadata['input_sha256'])
                    self.assertEqual(snapshot_path.read_bytes(), before)
                    self.assertEqual(source.read_bytes(), original_source)
                    if command == 6:
                        with self.assertRaisesRegex(s.SnapshotError, 'import-only'):
                            replay.validate_command_abi(loaded)
                        with self.assertRaisesRegex(s.SnapshotError, 'import-only'):
                            simulator.run(loaded, Path(folder)/'result', {})
                        self.assertFalse((Path(folder)/'result').exists())

    def test_current_producer_disallows_historical_precision_flags_and_range_version(self):
        abi = s.Abi()
        for flags, version in ((64, 1), (64, 2), (0, 2)):
            header, rows, blobs = fixture(abi)
            header[abi['H_COMMAND_ABI']] = COMMAND_ABI
            header[abi['H_RANGE_VERSION']] = version
            rows[1][abi['R_TAG']] = abi['TAG_DEPTH']
            rows[1][abi['R_FLAGS']] |= flags
            captures, report = s.decode_stream(s.encode_reference(header, rows, blobs, abi), abi)
            self.assertFalse(captures)
            self.assertTrue(report['failures'])


if __name__ == '__main__': unittest.main()
