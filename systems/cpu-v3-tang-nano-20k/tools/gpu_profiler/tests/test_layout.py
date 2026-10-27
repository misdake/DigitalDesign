"""Independent block enumeration, synthetic versions and full-byte guard checks."""
import hashlib
import json
import os
from pathlib import Path
import struct
import sys
import tempfile
import unittest
import zlib

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from layout import ROW, COLUMN, declared_layout
import identity
import replay
import snapshot as s
from compare import compare
import test_identity as identity_fixtures


def oracle_plane(depth=False):
    """Serial column-of-blocks producer, independent of the decoder's formula."""
    data = bytearray()
    for column in range(25):
        for block in range(16):
            for row in range(16):
                for lane in range(16):
                    x, y = column * 16 + lane, block * 16 + row
                    value = (y * 400 + x) * 17 + 12345 if depth else (
                        ((x + y) % 32) * 2048 + ((x * 3 + y * 5) % 64) * 32 + (x * 7 + y) % 32)
                    data.extend(struct.pack('<H', value & 65535 if y < 240 else 0x5a5a))
    return bytes(data)


class LayoutTests(unittest.TestCase):
    create = identity_fixtures.IdentityTests.create

    @classmethod
    def setUpClass(cls):
        cls.abi = s.Abi(os.environ.get('GPU_CAPTURE_ABI_SOURCE', s.DEFAULT_ABI))

    def test_independent_all_color_depth_words_boundaries_and_padding(self):
        for depth in (False, True):
            physical = oracle_plane(depth)
            decoded = COLUMN.words(physical)
            expected = [(((y * 400 + x) * 17 + 12345) & 65535 if depth else
                        ((x + y) % 32) * 2048 + ((x * 3 + y * 5) % 64) * 32 + (x * 7 + y) % 32)
                        for y in range(240) for x in range(400)]
            self.assertEqual(decoded, expected)
            points = [COLUMN.coordinates(i) for i in range(0, 204800, 2)]
            self.assertEqual(points.count(None), 6400)
            self.assertEqual(len(set(p for p in points if p is not None)), 96000)
            for x, y in [(0, 0), (15, 7), (16, 8), (31, 15), (32, 16), (399, 239)]:
                self.assertEqual(COLUMN.coordinates(COLUMN.offset(x, y)), (x, y))
            self.assertNotEqual(ROW.words(physical[:192000]), expected)

    def test_png_decodes_every_logical_pixel_without_padding(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)/'color.png'
            replay.preview(oracle_plane(), path, linear=False, layout=COLUMN)
            data, at, image = path.read_bytes(), 8, bytearray()
            while at < len(data):
                size = int.from_bytes(data[at:at+4], 'big')
                if data[at+4:at+8] == b'IDAT':
                    image.extend(data[at+8:at+8+size])
                at += size + 12
            image = zlib.decompress(image)
            for y in range(240):
                self.assertEqual(image[y*1201], 0)
                for x in range(400):
                    value = ((x+y)%32, (x*3+y*5)%64, (x*7+y)%32)
                    expected = bytes(round(v * 255 / levels) for v, levels in zip(value, (31, 63, 31)))
                    at = y*1201+1+x*3
                    self.assertEqual(image[at:at+3], expected)

    def test_column_snapshot_identity_full_extent_and_materialization(self):
        with tempfile.TemporaryDirectory() as temp:
            capture, build, receipt = self.create(temp, layout=COLUMN)
            result = identity.preflight(capture, build, receipt)
            self.assertEqual(result['layout'], COLUMN.description())
            self.assertEqual(result['extents'][1]['length_bytes'], 262144)
            path = Path(temp)/'saved'; capture.save(path)
            restored = s.load_snapshot(path, self.abi)
            self.assertEqual(restored.manifest()['input_sha256'], capture.manifest()['input_sha256'])
            self.assertEqual(restored.header['layout_abi'], 2)

    def test_unknown_span_version_and_build_layout_mismatches_fail(self):
        for field, value in [('LAYOUT_ABI', 3), ('LAYOUT_ABI', 1), ('PLANE_BYTES', 192000)]:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as temp:
                capture, _, _ = self.create(temp, layout=COLUMN)
                header = list(capture.descriptor); at = self.abi['H_'+field]
                header[at] = value & 65535
                if field == 'PLANE_BYTES': header[at+1] = value >> 16
                with self.assertRaises(s.SnapshotError): self.abi.header(header)
        with self.assertRaises(ValueError): declared_layout({**COLUMN.description(), 'macro_pitch':25})
        with tempfile.TemporaryDirectory() as temp:
            capture, build, receipt = self.create(temp, layout=COLUMN)
            data = json.loads(build.read_text()); data['version']=1; del data['layout']
            data['manifest_id']=identity.declared_id(data); build.write_text(json.dumps(data))
            capture.header['manifest_id']=data['manifest_id']
            receipt_data=json.loads(receipt.read_text()); receipt_data['manifest_id']=data['manifest_id']
            receipt.write_text(json.dumps(receipt_data))
            with self.assertRaises(s.SnapshotError): identity.preflight(capture, build, receipt)

    def test_explicit_export_cannot_silently_relabel_a_different_gpu(self):
        body='function[8:0]column_ui_tile; clear_sector==1599 wire[11:0]macro_column='
        with tempfile.TemporaryDirectory() as temp:
            folder=Path(temp)
            with self.assertRaises(s.SnapshotError): replay.export_layout(folder,body)
            (folder/'layout.json').write_text(json.dumps(dict(schema=1,layout_abi=2,gpu_profile=COLUMN.profile)))
            self.assertEqual(replay.export_layout(folder,body),COLUMN)
            with self.assertRaises(s.SnapshotError): replay.export_layout(folder,'row framebuffer')
            with self.assertRaises(s.SnapshotError): replay.export_layout(folder,body.replace('wire[11:0]macro_column=','row texture'))

    def test_paused_rtt_command_abi_cannot_enter_as_pure_column(self):
        with tempfile.TemporaryDirectory() as temp:
            capture,build,_=self.create(temp,layout=COLUMN)
            header=list(capture.descriptor); header[self.abi['H_COMMAND_ABI']]=2
            with self.assertRaises(s.SnapshotError): self.abi.header(header)
            value=json.loads(build.read_text()); value['command_abi']=2
            value['manifest_id']=identity.declared_id(value); build.write_text(json.dumps(value))
            with self.assertRaises(s.SnapshotError): identity.load_build(build)
            source=self.abi.source.read_text().replace('GPU_CAPTURE_NATIVE_COMMAND_ABI: u16 = 1;',
                                                     'GPU_CAPTURE_NATIVE_COMMAND_ABI: u16 = 2;')
            path=Path(temp)/'old-rtt-abi.rs'; path.write_text(source)
            with self.assertRaises(s.SnapshotError): s.Abi(path)

    def test_physical_span_is_required_but_unreferenced_allocation_tail_is_optional(self):
        with tempfile.TemporaryDirectory() as temp:
            capture, build, receipt = self.create(temp, layout=COLUMN, guards=False)
            result=identity.preflight(capture,build,receipt)
            self.assertEqual(result['captured_plane_bytes'],204800)
            self.assertFalse(result['allocation_guards_required'])
            self.assertEqual(result['layout']['allocation_bytes'],262144)
        with tempfile.TemporaryDirectory() as temp:
            capture,_,_=self.create(temp,layout=COLUMN)
            rows=[list(r.words) for r in capture.ranges]
            color=next(r for r in capture.ranges if r.tag==self.abi['TAG_COLOR'])
            rows[color.index][self.abi['R_FLAGS']] |= self.abi['FULL_CLEAR']
            complete,report=s.decode_stream(s.encode_reference(capture.descriptor,rows,
                    [bytes(capture.data[r.index]) for r in capture.ranges],self.abi),self.abi)
            self.assertFalse(complete)
            self.assertIn('FULL_CLEAR',report['failures'][0]['error'])

    def test_missing_guard_and_row_texture_catalog_fail_before_replay(self):
        for mode in ('guard', 'texture'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temp:
                capture, build, receipt = self.create(temp, layout=COLUMN)
                if mode == 'guard':
                    row = next(r for r in capture.ranges if r.tag == self.abi['TAG_COLOR'])
                    capture.data[row.index] = capture.data[row.index][:204800]
                else:
                    data=json.loads(build.read_text()); data['resources'][0].update(kind='texture',layout=ROW.texture_layout)
                    data['manifest_id']=identity.declared_id(data); build.write_text(json.dumps(data))
                with self.assertRaises((ValueError, IndexError)): identity.preflight(capture, build, receipt)

    def test_adjacent_comparison_separates_pixels_padding_and_guards(self):
        with tempfile.TemporaryDirectory() as temp:
            a, _, _ = self.create(temp, layout=COLUMN)
            header=list(a.descriptor)
            for key, value in [('TOKEN',a.snapshot+1),('SUBMISSION',a.header['submission']+1),
                               ('FRONT_BASE',0x440000),('COLOR_BASE',0x400000)]:
                at=self.abi['H_'+key]; header[at:at+2]=[value&65535,value>>16]
            b=s.decode_stream(s.encode_reference(header,[r.words for r in a.ranges],
                              [bytes(a.data[r.index]) for r in a.ranges],self.abi),self.abi)[0][0]
            folder=Path(temp)/'replay'; (folder/'capture').mkdir(parents=True)
            meta=dict(complete=True,input_sha256=a.manifest()['input_sha256'],layout=COLUMN.description(),captured_plane_bytes=262144)
            for role in ('color','depth'):
                data=bytearray(262144)
                if role=='color':
                    data[COLUMN.offset(16,8)]=1; data[15*512]=2; data[204800]=3
                for name, raw in [(role,data[:204800]),(role+'-allocation',data)]:
                    (folder/'capture'/(name+'.bin')).write_bytes(raw)
                    meta[name.replace('-','_')+'_sha256']=hashlib.sha256(raw).hexdigest()
            (folder/'replay.json').write_text(json.dumps(meta))
            result=compare(a,b,folder,Path(temp)/'comparison.json')
            self.assertFalse(result['equal']); color=result['planes']['color']
            self.assertEqual((color['different_pixels'],color['different_padding_words'],color['different_guard_bytes']),(1,1,1))
            self.assertEqual((color['examples'][0]['x'],color['examples'][0]['y']),(16,8))


if __name__ == '__main__': unittest.main()
