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


if __name__ == '__main__':
    unittest.main()
