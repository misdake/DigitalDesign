"""Independent far boundaries and versioned depth-range metadata."""
from pathlib import Path
import json
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from layout import COLUMN, depth_configuration, header_layout
import snapshot as s
from test_snapshot import fixture


class DepthShiftTests(unittest.TestCase):
    def test_materialized_baseline_abi4_loads_without_rewriting_original_metadata(self):
        abi = s.Abi()
        header, rows, blobs = fixture(abi)
        header[abi['H_COMMAND_ABI']] = 4
        result, report = s.decode_stream(s.encode_reference(header, rows, blobs, abi), abi)
        self.assertFalse(report['failures'])
        for old_hash in s.LEGACY_ABI4_SHA256:
            with tempfile.TemporaryDirectory() as folder:
                metadata = result[0].save(folder)
                metadata['abi_sha256'] = old_hash
                path = Path(folder)/'snapshot.json'
                path.write_text(json.dumps(metadata))
                original = path.read_bytes()
                loaded = s.load_snapshot(path, abi)
                self.assertEqual(loaded.manifest()['input_sha256'], metadata['input_sha256'])
                self.assertEqual(path.read_bytes(), original)
                metadata['descriptor_words'] = list(metadata['descriptor_words'])
                metadata['descriptor_words'][abi['H_COMMAND_ABI']] = 6
                path.write_text(json.dumps(metadata))
                with self.assertRaises(s.SnapshotError): s.load_snapshot(path, abi)

    def test_full_width_far_limits_and_legacy_native_bytes(self):
        for shift in range(4):
            for raw in (0, 1, 255, 65535, 63*65536,
                        (0xffff00 >> shift)-1, 0xffff00 >> shift,
                        200*65536-1, 200*65536):
                if raw*2**shift >= 1 << 24:
                    # API validation rejects these before packing. Overflow
                    # would overwrite the tag and no longer denotes this k.
                    continue
                valid = 0 < raw < 200*65536 and raw*2**shift < 0xffff00
                word = raw*2**shift | shift*2**24
                with self.subTest(shift=shift, raw=raw):
                    if valid:
                        self.assertEqual(depth_configuration(word, 6), (shift, raw*2**shift))
                        if shift == 0:
                            self.assertEqual(depth_configuration(word, 4), (0, raw))
                        else:
                            with self.assertRaises(ValueError): depth_configuration(word, 4)
                    else:
                        with self.assertRaises(ValueError): depth_configuration(word, 6)
        for word in (0x4000000, 0x2000001, 0xffffffff):
            with self.assertRaises(ValueError): depth_configuration(word, 6)

    def test_old_and_new_envelopes_round_trip_without_version_relabel(self):
        abi = s.Abi()
        for command in (4, 6):
            for range_version in (1, 2):
                header, rows, blobs = fixture(abi)
                header[abi['H_COMMAND_ABI']] = command
                header[abi['H_RANGE_VERSION']] = range_version
                result, report = s.decode_stream(s.encode_reference(header, rows, blobs, abi), abi)
                self.assertFalse(report['failures'])
                self.assertEqual(result[0].header['command_abi'], command)
                self.assertEqual(header_layout(result[0].header), COLUMN)

    def test_shift_range_requires_new_versions_and_depth_tag(self):
        abi = s.Abi()
        for command, version, tag, valid in (
                (6, 2, 'TAG_DEPTH', True), (4, 2, 'TAG_DEPTH', False),
                (6, 1, 'TAG_DEPTH', False), (6, 2, 'TAG_MESH', False)):
            header, rows, blobs = fixture(abi)
            header[abi['H_COMMAND_ABI']] = command
            header[abi['H_RANGE_VERSION']] = version
            rows[1][abi['R_TAG']] = abi[tag]
            rows[1][abi['R_FLAGS']] |= 2 << 5
            if valid:
                result, report = s.decode_stream(s.encode_reference(header, rows, blobs, abi), abi)
                self.assertFalse(report['failures'])
                self.assertEqual(result[0].ranges[1].flags >> 5, 2)
            else:
                result, report = s.decode_stream(s.encode_reference(header, rows, blobs, abi), abi)
                self.assertFalse(result)
                self.assertTrue(report['failures'])


if __name__ == '__main__': unittest.main()
