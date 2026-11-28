"""Check the allocator boundary used by frame-state comparisons."""
import unittest

from check_runtime_state import free_payload_ranges


class MeshArenaMask(unittest.TestCase):
    def arena(self):
        words = [0] * 16
        # Arena 2..16: free block 3..8, allocated 8..11, free 11..16.
        words[2] = 3
        words[3], words[4] = 5, 11
        words[8] = 3
        words[11], words[12] = 5, 0
        return words

    def test_allocated_bytes_and_free_headers_remain_observable(self):
        self.assertEqual(free_payload_ranges(self.arena(), 2, 14), [(5, 8), (13, 16)])

    def test_free_list_cycle_is_rejected(self):
        words = self.arena()
        words[12] = 3
        with self.assertRaisesRegex(ValueError, 'order'):
            free_payload_ranges(words, 2, 14)

    def test_block_cannot_hide_bytes_past_the_arena(self):
        words = self.arena()
        words[11] = 6
        with self.assertRaisesRegex(ValueError, 'exceeds'):
            free_payload_ranges(words, 2, 14)


if __name__ == '__main__':
    unittest.main()
