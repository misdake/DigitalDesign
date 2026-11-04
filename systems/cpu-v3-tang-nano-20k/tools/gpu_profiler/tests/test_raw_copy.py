"""Byte/permission/version checks for the new command, separate from RTL."""
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import snapshot as s
from raw_copy import MemoryImage, descriptor
from memcpy_fixture import make


class RawCopyTests(unittest.TestCase):
    def test_old_abi_memcpy_is_rejected_before_prepare_or_execute(self):
        from replay import prepare, execute
        for command_abi in (1, 2):
            with self.subTest(command_abi=command_abi), tempfile.TemporaryDirectory() as folder:
                capture, _ = make(s.Abi(), command_abi=command_abi)
                self.assertTrue(capture.complete)
                out = Path(folder)/'output'
                message = rf'ABI{command_abi}.*opcode12'
                with self.assertRaisesRegex(s.SnapshotError, message):
                    prepare(capture, Path(folder)/'unused-rtl', out, 100000, 60)
                self.assertFalse(out.exists())
                with patch('replay.capture_run') as run:
                    with self.assertRaisesRegex(s.SnapshotError, message):
                        execute(capture, out/'unused-manifest.json')
                    run.assert_not_called()

    def test_legacy_commands_and_prepared_abi_must_match(self):
        from replay import validate_command_abi, execute
        from test_snapshot import fixture
        abi = s.Abi(); header, rows, blobs = fixture(abi)
        # An opcode-shaped word in an unrelated data range is not a command.
        blobs[1] = struct.pack('<I',0xc0000044)+blobs[1][4:]
        captures, report = s.decode_stream(s.encode_reference(header,rows,blobs,abi),abi)
        self.assertFalse(report['failures'])
        validate_command_abi(captures[0])
        capture, _ = make(abi)
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder)/'manifest.json'
            path.write_text('{"command_abi":1}')
            with patch('replay.capture_run') as run:
                with self.assertRaisesRegex(s.SnapshotError, 'command ABI mismatch'):
                    execute(capture,path)
                run.assert_not_called()

    def test_mutations_feed_later_commands_and_keep_preimages(self):
        capture, original = make(s.Abi())
        image = MemoryImage(capture)
        scratch = bytearray(4096)
        scratch[272*8:272*8+128] = capture.read(0x4d0000,128)
        for slot in (68,69,70): image.copy(descriptor(0xc0000000|slot,scratch,3))
        self.assertEqual(image.read(0x300000,1536),original)
        self.assertEqual(image.read(0x440000,512),original[:512])
        self.assertEqual(image.read(0x7ffe00,512),original[:512])
        self.assertEqual(image.read(0x440200,204800-512),b'\x34\x12'*(102400-256))
        self.assertEqual(capture.read(0x300000,1536),bytes([0x3c])*1536)
        with self.assertRaises(ValueError): image.copy((0x200000,0x4d0000,512))
        with self.assertRaises(ValueError): image.copy((0x100000,0x300000,512))

    def test_version_reserved_bits_overlap_and_extent_are_rejected(self):
        capture, _ = make(s.Abi()); scratch = bytearray(4096)
        scratch[272*8:272*8+128] = capture.read(0x4d0000,128)
        self.assertEqual(descriptor(0xc0000044,scratch,3), descriptor(0xc0000044,scratch,4))
        self.assertEqual(descriptor(0xc0000044,scratch,4), descriptor(0xc0000044,scratch,8))
        for abi in (0,1,2,5,6,9):
            with self.assertRaises(ValueError): descriptor(0xc0000044,scratch,abi)
        for value in (1<<39,0,0x3fff|(1<<28)):
            struct.pack_into('<Q',scratch,68*32,value)
            with self.assertRaises(ValueError): descriptor(0xc0000044,scratch,3)
        with self.assertRaises(ValueError): descriptor(0xc00000c4,scratch,3)
        with self.assertRaises(ValueError): descriptor(0xc0000050,scratch,3)

    def test_scene_reference_observes_color_copy_and_chained_raw_output(self):
        from scene_oracle import render
        from layout import COLUMN
        capture, original = make(s.Abi()); result = render(capture)
        raw = bytearray(b'\x34\x12'*102400);raw[:512] = original[:512]
        self.assertEqual(result.color.reshape(-1).tolist(),COLUMN.words(raw))
        self.assertEqual(len(result.stats['raw_copies']),3)


if __name__ == '__main__': unittest.main()
