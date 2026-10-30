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


if __name__ == '__main__':
    unittest.main()
