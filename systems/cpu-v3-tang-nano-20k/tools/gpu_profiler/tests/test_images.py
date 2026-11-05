"""Synthetic cache lifetimes and overlapping command ownership, without RTL."""
from pathlib import Path
import struct
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from images import History
from layout import COLUMN
from snapshot import SnapshotError


class ImageHistoryTests(unittest.TestCase):
    def history(self):
        h = History(bytearray(524288), bytearray(b'\1')*524288, COLUMN)
        h.epoch = 1
        h.state(0, 1, 0, 1, 0, 262144)
        return h

    def pixel(self, h, owner, epoch=1, x=0):
        plane, _ = h.plane(0, epoch, owner)
        return struct.unpack_from('<H', plane, COLUMN.offset(x, 0))[0]

    def test_younger_overwrite_and_eviction_preserve_older_completion_image(self):
        h = self.history()
        h.cache(1, 0, 0, 0, 0xf800, 1)
        h.cache(2, 0, 0, 0, 0x001f, 2)
        self.assertEqual(self.pixel(h, 1), 0xf800)
        h.bus(3, 0, 0x001f)
        h.state(0, 0, 0, 1, 0, 262144)
        self.assertEqual(self.pixel(h, 1), 0xf800)
        self.assertEqual(self.pixel(h, 2), 0x001f)

    def test_repeated_full_slot_snapshot_preserves_dirty_word_provenance(self):
        h = self.history()
        h.cache(1, 0, 0, 0, 0x1234, 7)
        h.state(0, 1, 0, 1, 0, 262144)
        h.state(1, 1, 1, 1, 0, 262144)
        h.bus(3, 0, 0x1234)
        h.state(0, 0, 0, 1, 0, 262144)
        self.assertEqual(self.pixel(h, 7), 0x1234)
        self.assertEqual(self.pixel(h, 6), 0)
        with self.assertRaisesRegex(SnapshotError, 'provenance'):
            h.bus(4, 2, 0x5678)
        h.state(0, 1, 0, 1, 0, 262144)
        h.epoch=2
        with self.assertRaisesRegex(SnapshotError,'stale'):
            h.state(0, 1, 0, 1, 0, 262144)

    def test_invalid_wrong_epoch_and_reused_slot_cannot_overlay_old_pixels(self):
        h = self.history()
        h.cache(1, 0, 0, 0, 0xf800, 1)
        self.assertEqual(self.pixel(h, 1, epoch=2), 0)
        h.state(0, 0, 0, 1, 0, 262144)
        h.state(0, 1, 1, 1, 0, 262144)
        h.cache(2, 0, 0, 0, 0x001f, 2)
        self.assertEqual(self.pixel(h, 2), 0)
        self.assertEqual(self.pixel(h, 2, x=16), 0x001f)
        h.epoch = 2
        with self.assertRaisesRegex(SnapshotError, 'invalid'):
            h.cache(3, 0, 0, 0, 1, 3)
        h.state(1, 1, 0, 2, 262144, 0)
        h.cache(4, 1, 0, 32, 0x07e0, 4)
        plane, _ = h.plane(262144, 2, 4)
        self.assertEqual(struct.unpack_from('<H', plane, 0)[0], 0x07e0)
        self.assertEqual(self.pixel(h, 4, epoch=2), 0)

    def test_direct_clear_and_copy_are_versioned_without_stealing_dirty_cache_owner(self):
        h = self.history()
        h.bus(1, 0, 0xf800, direct_owner=1)
        h.bus(2, 0, 0x07e0, direct_owner=2)
        self.assertEqual(self.pixel(h, 1), 0xf800)
        self.assertEqual(self.pixel(h, 2), 0x07e0)
        h.cache(3, 0, 0, 0, 0x001f, 3)
        with self.assertRaisesRegex(SnapshotError, 'latest valid'):
            h.bus(4, 0, 0xf800)

    def test_uncaptured_state_and_unexplained_writeback_are_refused(self):
        h = History(bytearray(524288), bytearray(524288), COLUMN)
        with self.assertRaisesRegex(SnapshotError, 'uncaptured'):
            h.plane(0, None, 0)
        with self.assertRaisesRegex(SnapshotError, 'provenance'):
            h.bus(1, 0, 123)

    def test_empty_draw_between_live_draws_has_no_premature_image(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        import tempfile
        import images
        profile=dict(origin=0,end=12,partial=False,
            commands=[dict(id=10,opcode=3,meshlet=1,submission=1,pc=0,execute=1,complete_cycle=8),
                      dict(id=11,opcode=3,meshlet=2,submission=1,pc=4,execute=2,complete_cycle=4),
                      dict(id=12,opcode=3,meshlet=3,submission=1,pc=8,execute=5,complete_cycle=10)],
            meshlets=[dict(id=1,command=10,quads=1),dict(id=2,command=11,quads=0),
                      dict(id=3,command=12,quads=1)],
            submissions=[dict(id=1,start=0,end=12)],memory=dict(available=True,transactions=[]))
        capture=SimpleNamespace(header=dict(color_base=0,depth_base=262144,target_format=1),
            ranges=[SimpleNamespace(address=0,length=COLUMN.plane_bytes,index=0)],
            data={0:bytes(COLUMN.plane_bytes)})
        with tempfile.TemporaryDirectory() as tmp, patch.object(images,'MEMORY_BYTES',524288), \
             patch.object(images,'header_layout',return_value=COLUMN):
            root=Path(tmp); events=root/'events.log'
            events.write_text('IMAGE_TARGET 0 1 0 262144 1\n'
                'IMAGE_CACHE_STATE 1 0 1 0 1 0 262144\n'
                'IMAGE_CACHE_WRITE 3 0 0 0 63488 1\n'
                'IMAGE_CACHE_WRITE 7 0 0 0 31 3\n')
            result=images.build(capture,profile,events,root/'images')
            self.assertEqual(set(result['commands']),{'10','12'})
            self.assertFalse((root/'images/command-000011.png').exists())
            self.assertEqual(result['commands']['10']['cycle'],8)
            red=bytearray(COLUMN.plane_bytes);struct.pack_into('<H',red,0,0xf800)
            blue=bytearray(COLUMN.plane_bytes);struct.pack_into('<H',blue,0,0x001f)
            import hashlib
            self.assertEqual(result['commands']['10']['plane_sha256'],hashlib.sha256(red).hexdigest())
            self.assertEqual(result['commands']['12']['plane_sha256'],hashlib.sha256(blue).hexdigest())
            images.validate(result,profile)
            del result['commands']['12']
            with self.assertRaisesRegex(ValueError,'Incomplete'):images.validate(result,profile)


class DisplayPreviewTests(unittest.TestCase):
    def temp_root(self):
        root = Path(__file__).resolve().parents[5] / 'target/profiler-display-tests'
        root.mkdir(parents=True, exist_ok=True)
        return root

    def png_pixels(self, path):
        import zlib
        data = Path(path).read_bytes()
        at, compressed, chunks = 8, bytearray(), []
        while at < len(data):
            length, = struct.unpack_from('>I', data, at)
            kind, payload = data[at+4:at+8], data[at+8:at+8+length]
            chunks.append(kind)
            if kind == b'IDAT': compressed.extend(payload)
            at += length + 12
        raw = zlib.decompress(compressed)
        self.assertEqual(len(raw), 240 * 1201)
        self.assertTrue(all(raw[y*1201] == 0 for y in range(240)))
        return b''.join(raw[y*1201+1:(y+1)*1201] for y in range(240)), chunks

    def test_all_65536_pixels_match_exported_owning_rust_function(self):
        import re
        import subprocess
        import tempfile
        import images
        source = Path(__file__).resolve().parents[3] / 'src/display.rs'
        text = source.read_text()
        table = re.search(r'pub const LINEAR6_TO_SRGB8:.*?\];', text, re.S)[0]
        start = text.index('pub fn linear_rgb565_to_srgb888(')
        function = text[start:text.index('\n}', start)+2]
        with tempfile.TemporaryDirectory(dir=self.temp_root()) as temp:
            root = Path(temp)
            code = table + '\n' + function + r"""
fn main(){use std::io::Write;let mut out=std::fs::File::create(std::env::args().nth(1).unwrap()).unwrap();
for pixel in 0..=u16::MAX{let(r,g,b)=linear_rgb565_to_srgb888(pixel);out.write_all(&[r,g,b]).unwrap();}}
"""
            (root/'oracle.rs').write_text(code)
            executable = root / ('oracle.exe' if sys.platform == 'win32' else 'oracle')
            subprocess.run(['rustc', '--edition=2021', '-O', str(root/'oracle.rs'), '-o', str(executable)], check=True, timeout=30, capture_output=True)
            subprocess.run([str(executable), str(root/'rgb.bin')], check=True, timeout=10)
            expected = (root/'rgb.bin').read_bytes()
            actual = bytes(channel for pixel in range(65536) for channel in images.display_rgb565(pixel))
            self.assertEqual(actual, expected)

    def test_build_override_and_direct_export_share_conversion_without_changing_capture(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        import tempfile
        import images
        plane = struct.pack('<H', 0x8410) * (COLUMN.plane_bytes // 2)
        capture = SimpleNamespace(header=dict(color_base=0,depth_base=262144,target_format=0),
            ranges=[SimpleNamespace(address=0,length=COLUMN.plane_bytes,index=0)],data={0:plane})
        profile = dict(origin=0,end=4,partial=False,commands=[dict(id=1,opcode=7,submission=1,pc=0,execute=1,complete_cycle=3)],
            meshlets=[],submissions=[dict(id=1,start=0,end=4)],memory=dict(available=True,transactions=[]))
        with tempfile.TemporaryDirectory(dir=self.temp_root()) as temp, patch.object(images,'MEMORY_BYTES',524288), patch.object(images,'header_layout',return_value=COLUMN):
            root=Path(temp);events=root/'events.txt';events.write_text('IMAGE_TARGET 0 1 0 262144 1\n')
            first=images.build(capture,profile,events,root/'first',display_preview=True,capture_format_known=False)
            again=images.build(capture,profile,events,root/'again',display_preview=True,capture_format_known=False)
            images.preview(plane,root/'direct.png',True)
            self.assertEqual((root/'first/final.png').read_bytes(),(root/'direct.png').read_bytes())
            self.assertEqual((root/'first/command-000001.png').read_bytes(),(root/'direct.png').read_bytes())
            self.assertEqual((root/'first/final.png').read_bytes(),(root/'again/final.png').read_bytes())
            self.assertEqual(first['final']['png_sha256'],again['final']['png_sha256'])
            self.assertEqual(capture.header['target_format'],0)
            self.assertEqual(first['final']['format'],0)
            self.assertEqual(first['final']['preview']['display_format'],1)
            self.assertTrue(first['final']['preview']['requested_override'])
            self.assertFalse(first['final']['preview']['capture_format_known'])
            images.validate(first,profile)

    def test_raw_plane_to_png_is_exact_once_and_repeatable(self):
        import hashlib
        import tempfile
        import images
        plane = bytearray(COLUMN.plane_bytes)
        expected = bytearray()
        for y in range(240):
            for x in range(400):
                pixel = (y*400+x) & 65535
                struct.pack_into('<H', plane, COLUMN.offset(x,y), pixel)
                expected.extend(images.display_rgb565(pixel))
        original = hashlib.sha256(plane).digest()
        with tempfile.TemporaryDirectory(dir=self.temp_root()) as temp:
            root = Path(temp)
            images.preview(plane, root/'first.png', True)
            images.preview(plane, root/'again.png', True)
            pixels, chunks = self.png_pixels(root/'first.png')
            self.assertEqual(pixels, expected)
            self.assertIn(b'sRGB', chunks)
            self.assertEqual((root/'first.png').read_bytes(), (root/'again.png').read_bytes())
            self.assertEqual(hashlib.sha256(plane).digest(), original)
            images.preview(plane, root/'raw.png', False)
            self.assertNotEqual((root/'raw.png').read_bytes(), (root/'first.png').read_bytes())


if __name__ == '__main__':
    unittest.main()
