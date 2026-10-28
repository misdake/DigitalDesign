"""Small format, coverage and measurement-integrity regressions; no hardware."""
import binascii
import copy
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
import assets
import gallery
import report


def packet(kind, case, sample, us, token=42, flags=0):
    data = b'GBEN\x01' + struct.pack('<B6H', kind, case, sample, us, 1, flags, token)
    return data + struct.pack('<H', binascii.crc_hqx(data, 0xffff))


class Bench(unittest.TestCase):
    def test_full_scene_uses_identical_draws_with_and_without_clear(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            assets.generate_full_scene(root)
            manifest = json.loads((root/'assets.json').read_text())
            cases = {c['name']: c for c in manifest['cases']}
            without = cases['full_no_clear']; with_clear = cases['full_with_clear']
            self.assertEqual((without['meshlets'], without['triangles']), (24, 1141))
            self.assertEqual(cases['rabbit_no_clear']['triangles'], 948)
            self.assertEqual(cases['box_rgb_no_clear']['triangles'], 193)
            self.assertEqual([word for word in with_clear['commands'] if word >> 28 != 8],
                             without['commands'])
            self.assertEqual(sum(word >> 28 == 3 for word in without['commands']), 24)
            self.assertEqual([word for word in with_clear['commands'] if word >> 28 == 8],
                             [0x80000000, 0x8001ffff])

    def test_maximum_meshlet_roundtrip(self):
        record, points = assets.native_mesh([(0, 0, 2), (1, 0, 2), (0, 1, 2)],
                                             [(0, 1, 2)]*96, vertices=64)
        self.assertEqual(len(record), 1152)
        header = struct.unpack_from('<Q', record)[0]
        self.assertEqual(header & 127, 64)
        self.assertEqual((header >> 8) & 127, 96)
        self.assertEqual((header >> 24) & 127, 96)
        for i in range(32):
            word = struct.unpack_from('<Q', record, 128+768+i*8)[0]
            self.assertEqual(word >> 54, 0)
            for k in range(3):
                self.assertEqual((word >> (18*k)) & ((1 << 18)-1), 0 | 1 << 6 | 2 << 12)
        self.assertEqual(points[:3], [(0, 0, 2), (1, 0, 2), (0, 1, 2)])

    def test_full_coverage_and_backface_reference(self):
        full = [assets.screen_point(x, y) for x, y in [(-32, -32), (864, -32), (-32, 544)]]
        _, decoded = assets.native_mesh(full, [(0, 1, 2)])
        self.assertEqual(assets.coverage(decoded, [(0, 1, 2)]), (96000, 24000))
        self.assertEqual(assets.coverage(decoded, [(0, 2, 1)]), (0, 0))

    def test_packet_identity_crc_and_noise(self):
        good = packet(1, 7, 3, 12345)
        bad = bytearray(good); bad[11] ^= 1
        frames, counts = report.decode(b'noise'+bad+good+packet(1, 0, 0, 12, token=43), 42)
        self.assertEqual(len(frames), 1)
        self.assertEqual(frames[0]['us'], 12345)
        self.assertEqual(counts, dict(corrupt_packets=1, foreign_packets=1))

    def test_incomplete_and_saturated_samples_do_not_get_a_rate(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            manifest = dict(token=42, samples=3, clock_hz=54000000, width=400, height=240,
                cases=[dict(id=0, name='case', family='fill', vertices=3, triangles=1,
                            reference_pixels=96000, reference_quads=24000)])
            path = root/'assets.json'; path.write_text(json.dumps(manifest))
            capture = root/'capture.bin'; capture.write_bytes(packet(1, 0, 0, 65535, flags=1))
            with self.assertRaisesRegex(ValueError, 'Incomplete'):
                report.report(path, capture, root/'report')
            result = json.loads((root/'report/report.json').read_text())
            self.assertFalse(result['complete'])
            self.assertTrue(result['cases'][0]['saturated'])
            self.assertNotIn('nominal_cycles', result['cases'][0])

    def test_interval_statistics_are_reproducible(self):
        values = list(range(90, 121))
        result = report.stats(values)
        self.assertEqual(result, report.stats(values))
        self.assertEqual(result['median'], 105)
        self.assertEqual(result['mad'], 8)
        self.assertLessEqual(result['median_ci95'][0], 105)
        self.assertGreaterEqual(result['median_ci95'][1], 105)

    def test_old_gallery_and_reset_partial_runs_are_not_measurements(self):
        rows = [dict(kind=k, case=0) for k in [2, 0, 1, 0, 1, 3, 2, 2]]
        selected, end, complete = report.measurement_epoch(rows)
        self.assertTrue(complete)
        self.assertEqual([r['kind'] for r in selected], [0, 1, 3])
        self.assertEqual(end, 5)
        self.assertEqual([r['kind'] for r in rows[end+1:]], [2, 2])

    def test_new_partial_epoch_cannot_reuse_an_old_complete_epoch(self):
        rows = [dict(kind=k) for k in [0, 1, 3, 2, 0, 1]]
        selected, end, complete = report.measurement_epoch(rows)
        self.assertFalse(complete)
        self.assertIsNone(end)
        self.assertEqual([r['kind'] for r in selected], [0, 1])
        with self.assertRaisesRegex(ValueError, 'Repeated completion'):
            report.measurement_epoch([dict(kind=k) for k in [0, 3, 3]])

    def report_fixture(self, root, extra=b'', flags=0, marker_cases=1):
        manifest = dict(token=42, samples=1, clock_hz=54000000, width=400, height=240,
                        cases=[dict(id=0, name='case', family='fill', vertices=3, triangles=1,
                                    reference_pixels=96000, reference_quads=24000)])
        path = root/'assets.json'; path.write_text(json.dumps(manifest))
        raw = root/'uart.bin'
        raw.write_bytes(packet(0, marker_cases, 1, 0) + packet(5, 0, 0, 100) +
                        packet(1, 0, 0, 10, flags=flags) + packet(3, marker_cases, 1, 0) + extra)
        return path, raw

    def test_complete_epoch_has_exact_manifest_and_headless_flags(self):
        for flags, cases, reason in [(4, 1, 'Display-active'), (0, 2, 'marker')]:
            with self.subTest(flags=flags, cases=cases), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                manifest, raw = self.report_fixture(root, flags=flags, marker_cases=cases)
                with self.assertRaisesRegex(ValueError, reason):
                    report.report(manifest, raw, root/'report')

    def test_current_report_is_fresh_and_preserves_raw_intervals(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            manifest, raw = self.report_fixture(root)
            report.report(manifest, raw, root/'report')
            result = json.loads((root/'report/report.json').read_text())
            self.assertTrue(result['complete'])
            self.assertEqual(result['cases'][0]['us']['median'], 10)
            self.assertEqual(result['cases'][0]['nominal_cycles'], 540)
            self.assertEqual(result['cases'][0]['setup_us']['median'], 100)
            with self.assertRaisesRegex(ValueError, 'fresh report'):
                report.report(manifest, raw, root/'report')

    def test_report_refuses_a_reset_after_the_complete_run(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            manifest, raw = self.report_fixture(root, extra=packet(0, 1, 1, 0)+packet(1, 0, 0, 12))
            with self.assertRaisesRegex(ValueError, 'Incomplete'):
                report.report(manifest, raw, root/'report')

    def test_live_csv_maps_packet_completion_across_chunk_boundaries(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root/'uart.bin.timing.csv').write_text(
                'offset,count,utc\n0,12,2026-10-27T10:00:00.0000000Z\n'
                '12,28,2026-10-27T10:00:01.0000000Z\n')
            events = [dict(offset=0), dict(offset=20)]
            source = gallery.attach_timestamps(root, events, 40)
            self.assertEqual(source, 'uart.bin.timing.csv')
            self.assertEqual([r['utc'] for r in events],
                             [gallery.utc_seconds('2026-10-27T10:00:01Z')]*2)

    def test_timing_rejects_gaps_short_coverage_and_clock_reversal(self):
        rows = [('0,40,2026-10-27T10:00:00Z', 41),
                ('1,40,2026-10-27T10:00:00Z', 41),
                ('0,20,2026-10-27T10:00:01Z\n20,20,2026-10-27T10:00:00Z', 40)]
        for body, size in rows:
            with self.subTest(body=body), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                (root/'uart.bin.timing.csv').write_text('offset,count,utc\n'+body+'\n')
                with self.assertRaises(ValueError):
                    gallery.attach_timestamps(root, [dict(offset=0)], size)
        with self.assertRaisesRegex(ValueError, 'timezone'):
            gallery.utc_seconds('2026-10-27T10:00:00')

    def test_legacy_timing_is_offline_compatible_but_csv_has_priority(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root/'uart.bin.timing.json').write_text(json.dumps(dict(
                start_utc='2026-10-27T10:00:00.0000000Z',
                chunks=[dict(offset=0, count=20, seconds=1)])))
            events = [dict(offset=0)]
            self.assertEqual(gallery.attach_timestamps(root, events, 20), 'uart.bin.timing.json')
            self.assertEqual(events[0]['utc'], gallery.utc_seconds('2026-10-27T10:00:01Z'))
            (root/'uart.bin.timing.csv').write_text('wrong,header\n')
            with self.assertRaisesRegex(ValueError, 'CSV'):
                gallery.attach_timestamps(root, events, 20)


if __name__ == '__main__':
    unittest.main()
