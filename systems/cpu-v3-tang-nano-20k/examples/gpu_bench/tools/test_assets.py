"""Small allocation contracts; complete workload execution is a separate qualification."""
import unittest
import assets
from gpu_layout import asset_regions, validate_asset_range


class AssetBanks(unittest.TestCase):
    def test_exact_bank_boundaries_and_reserved_holes(self):
        regions = asset_regions()
        self.assertEqual(regions, [(0x4e0000, 0x7df800), (0x100000, 0x400000)])
        self.assertEqual(sum(end - first for first, end in regions), 6 * 1024 * 1024 - 2048)
        for first, end in regions:
            validate_asset_range(first, end - first)
            validate_asset_range(end - 128, 128)
            for address, length in [(first - 128, 256), (end - 128, 256), (first, 0), (first, -1)]:
                with self.assertRaises(ValueError):
                    validate_asset_range(address, length)
        for address in [0, 0xe0000, 0x400000, 0x480000, 0x4c0000, 0x4d0000,
                        0x7df800, 0x7e0000, 0x7f0000, 0x800000]:
            with self.assertRaises(ValueError):
                validate_asset_range(address, 128)
        with self.assertRaises(ValueError):
            validate_asset_range(0x3fff80, 0x4e0080 - 0x3fff80)

    def test_whole_allocations_and_failed_requests_do_not_mutate_banks(self):
        arena = assets.Arena([(0x1000, 0x1200), (0x2000, 0x2400)])
        self.assertEqual(arena.put(bytes(300)), 0x1000)
        self.assertEqual(arena.put(bytes(600)), 0x2000)
        before = [(first, end, bytes(data)) for first, end, data in arena.banks]
        for data in [b'', bytes(1025)]:
            with self.assertRaises(ValueError):
                arena.put(data)
            self.assertEqual(before, [(first, end, bytes(data)) for first, end, data in arena.banks])
        self.assertEqual(arena.put(bytes(256)), 0x2300)
        self.assertEqual(len(arena.banks[0][2]), 300)


if __name__ == '__main__':
    unittest.main()
