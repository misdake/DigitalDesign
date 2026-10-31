"""Explicit-record envelope/policy tests; no physical GPU qualification."""
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from layout import COLUMN, capture_description
import identity
import snapshot as s
from test_snapshot import fixture


def recorded(abi, target=True):
    header, rows, blobs = fixture(abi)
    def pair(name, value):
        at = abi['H_' + name]
        header[at:at+2] = [value & 65535, value >> 16]
    header[abi['H_VERSION']] = abi['EXPLICIT_VERSION']
    header[abi['H_FLAGS']] = abi['PUBLISHED'] | (abi['HAS_TARGET'] if target else 0)
    header[abi['H_FRAME']] = 0
    pair('COMMAND_BASE', 0x380000)
    pair('FRONT_BASE', 0)
    header[abi['H_FRONT_FORMAT']] = 0
    for name, value in [('COLOR_BASE',0x100000),('DEPTH_BASE',0x180000),('PLANE_BYTES',COLUMN.plane_bytes)]:
        pair(name, value if target else 0)
    if not target:
        header[abi['H_TARGET_FORMAT']] = 0
    return header


class RecordedTests(unittest.TestCase):
    def setUp(self):
        self.abi = s.Abi()

    def test_dynamic_bounds_and_presence_do_not_reinterpret_v1(self):
        header = recorded(self.abi)
        parsed = self.abi.header(header)
        self.assertTrue(parsed['has_target'])
        self.assertEqual(parsed['cb_base'], 0x380000)
        self.assertEqual(capture_description(parsed), COLUMN.recorded_description())
        header[self.abi['H_VERSION']] = 1
        header[self.abi['H_FLAGS']] = 1
        with self.assertRaises(s.SnapshotError):
            self.abi.header(header)

    def test_zero_target_and_front_addresses_are_real_when_present(self):
        header = recorded(self.abi)
        header[self.abi['H_COLOR_BASE']:self.abi['H_COLOR_BASE']+2] = [0,0]
        header[self.abi['H_FLAGS']] |= self.abi['HAS_FRONT']
        parsed = self.abi.header(header)
        self.assertEqual(parsed['color_base'], 0)
        self.assertTrue(parsed['has_front'])

    def test_target_absent_is_explicit_and_cannot_have_stale_fields(self):
        header = recorded(self.abi, False)
        self.assertFalse(self.abi.header(header)['has_target'])
        header[self.abi['H_COLOR_BASE']] = 128
        with self.assertRaises(s.SnapshotError):
            self.abi.header(header)

    def test_unknown_flags_or_inconsistent_optional_ranges_are_rejected(self):
        for field, value in [('FLAGS',19),('UI_BASE',128),('FRONT_BASE',128),('FRAME',1),('SUBMISSION',0)]:
            header = recorded(self.abi)
            header[self.abi['H_'+field]] = value
            with self.subTest(field=field), self.assertRaises(s.SnapshotError):
                self.abi.header(header)

    def test_physical_span_does_not_invent_a_256k_allocation(self):
        header = recorded(self.abi)
        at = self.abi['H_COLOR_BASE']
        header[at:at+2] = [0,0x7c]
        self.assertEqual(self.abi.header(header)['color_base'], 0x7c0000)
        header[at+1] = 0x7d
        with self.assertRaises(s.SnapshotError):
            self.abi.header(header)

    def test_v3_build_policy_has_no_fixed_catalog_or_required_static_asset(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root/'source').write_bytes(b'unit fixture, not firmware')
            spec = dict(files=[dict(name='source',path='source')],resources=[],options={},
                        toolchain={},source_commit='test',layout=COLUMN.recorded_description())
            (root/'spec.json').write_text(json.dumps(spec))
            build = identity.create_build(root/'spec.json',root/'build.json')
            self.assertEqual(build['version'],3)
            self.assertEqual(identity.build_layout(build),COLUMN)
            bad = copy.deepcopy(build)
            bad['version'] = 2
            with self.assertRaises(s.SnapshotError):
                identity.build_layout(bad)

    def test_v3_atlas_catalog_uses_allocation_extent_while_v2_stays_fixed(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root/'source').write_bytes(b'catalog policy fixture')
            spec = dict(files=[dict(name='source',path='source')],resources=[],options={},
                        toolchain={},source_commit='test',layout=COLUMN.recorded_description())
            (root/'spec.json').write_text(json.dumps(spec))
            build = identity.create_build(root/'spec.json',root/'build.json')
            for version, address, size, accepted in [(3,0x80000,768,True),
                    (3,0x80000,896,True),(3,0x80000,128,False),
                    (3,0x80000,257,False),(3,0x80080,768,False),
                    (2,0x500000,768,False),(2,0x500000,65536,True)]:
                candidate = copy.deepcopy(build)
                candidate.update(version=version,layout=COLUMN.recorded_description() if version==3 else COLUMN.description(),
                                 resources=[dict(name='atlas',kind='atlas',address=address,bytes=size)])
                candidate['manifest_id'] = identity.declared_id(candidate)
                (root/'candidate.json').write_text(json.dumps(candidate))
                with self.subTest(version=version, address=address, size=size):
                    if accepted:
                        identity.load_build(root/'candidate.json')
                    else:
                        with self.assertRaises(s.SnapshotError):
                            identity.load_build(root/'candidate.json')


@unittest.skipUnless(os.environ.get('GPU_API_CAPTURE_FIXTURE_DIR'), 'requires actual C producer fixtures')
class ProducerFixtures(unittest.TestCase):
    def test_actual_c_native_envelopes_roundtrip_and_preflight(self):
        files=list(Path(os.environ['GPU_API_CAPTURE_FIXTURE_DIR']).glob('*.json'))
        self.assertGreaterEqual(len(files),16)
        abi=s.Abi()
        for path in files:
            source=json.loads(path.read_text())
            rows=[source['ranges'][i:i+8] for i in range(0,len(source['ranges']),8)]
            raw=s.encode_reference(source['header'],rows,[bytes(b) for b in source['blobs']],abi)
            captures,report=s.decode_stream(raw,abi)
            with self.subTest(path=path.name):
                self.assertFalse(report['failures'])
                self.assertEqual(len(captures),1)
                preflight=identity.input_preflight(captures[0])
                self.assertEqual(preflight['layout'],COLUMN.recorded_description())
                self.assertFalse(preflight['build_identity_verified'])
                if path.name.startswith('memcpy'):
                    self.assertEqual(preflight['captured_plane_bytes'],0)
                if path.name.startswith('rtt'):
                    planes=[r for r in captures[0].ranges if r.tag==abi['TAG_COLOR'] and r.flags&abi['WRITE']]
                    self.assertEqual(len(planes),2)


@unittest.skipUnless(os.environ.get('GPU_API_UART_CAPTURE'), 'requires actual C serializer capture')
class UartFixture(unittest.TestCase):
    def test_real_v2_serializer_frames_crc_and_payloads(self):
        captures,report=s.decode_stream(Path(os.environ['GPU_API_UART_CAPTURE']).read_bytes(),s.Abi())
        self.assertFalse(report['failures'])
        self.assertEqual(len(captures),1)
        capture=captures[0]
        self.assertEqual((capture.header['token'],capture.header['submission']),(3,3))
        self.assertEqual(capture.header['capture_version'],2)
        self.assertFalse(capture.header['has_target'])
        self.assertEqual(capture.read(0x300000,1024),b''.join(((i*37)&65535).to_bytes(2,'little') for i in range(512)))
        identity.input_preflight(capture)


if __name__ == '__main__':
    unittest.main()
