"""Exact block sharing and actual RCC cache-body checks, with bounded native runs."""
import argparse
import json
from pathlib import Path
import random
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import view_pvs

ROOT = Path(__file__).resolve().parents[5]
REAL = None


def native_cache(data, info, expected):
    """Compile the unchanged RCC cache body with guarded host pointer operations."""
    source = (Path(__file__).resolve().parents[1]/'rcc/pvs.rs').read_text()
    source = '\n'.join(line.replace('//!', '//') for line in source.splitlines()
                       if not line.startswith(('use ', 'mod ')))
    folder = ROOT/'target/tmp'
    folder.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=folder) as temporary:
        temporary = Path(temporary)
        packed, golden = temporary/'pvs.bin', temporary/'expected.bin'
        packed.write_bytes(data)
        golden.write_bytes(expected)
        constants = dict(MAP_PVS_MASK_BLOCKS=info['block_count'],
                         MAP_PVS_COARSE=int(info.get('encoding') == 'direct-chunk-masks'),
                         MAP_PVS_MASK_WORDS=info['mask_words'],
                         MAP_PVS_YAW_SECTORS=info['yaw_sectors'],
                         MAP_PVS_HEADING_FIRST_SECTOR=info['heading_first_sector'],
                         MAP_PVS_ROOT_FIRST_SECTOR=info['root_first_sector'],
                         MAP_PVS_BLOCK_FIRST_SECTOR=info['block_first_sector'],
                         MAP_PVS_ROOT_SHIFT=info['root_sector_shift'])
        fixture = '#![allow(dead_code, non_camel_case_types, unused_mut)]\n'
        fixture += f'const DATA: &[u8] = include_bytes!({json.dumps(packed.as_posix())});\n'
        fixture += f'const EXPECTED: &[u8] = include_bytes!({json.dumps(golden.as_posix())});\n'
        fixture += '\n'.join(f'const {k}: u16 = {v};' for k, v in constants.items())
        fixture += '\n' + HOST + '\n' + source + '\n' + MAIN
        path = temporary/'cache.rs'
        path.write_text(fixture)
        executable = temporary/'cache.exe'
        subprocess.run(['rustc', '--edition=2021', str(path), '-o', str(executable)],
                       check=True, capture_output=True, text=True, timeout=30)
        subprocess.run([str(executable)], check=True, capture_output=True, text=True, timeout=30)


def packed(keys, masks, width):
    result = view_pvs.pack(keys, masks, width)
    if result is None:
        raise AssertionError('fixture exceeded format capacity')
    data, info = result
    info.update(mask_words=width, yaw_sectors=view_pvs.YAW_SECTORS)
    expected = b''.join(masks[key] for key in keys)
    for key in range(len(keys)):
        assert struct.pack(f'<{width}H', *view_pvs.decode(data, info, key)) == masks[keys[key]]
    return data, info, expected


class PvsStorage(unittest.TestCase):
    def test_direct_chunk_masks_preserve_route_successors_and_cache_hits(self):
        # Cell zero queries segments 0/1; cell one queries 1/2. Partial
        # segments still consume their whole aligned metadata chunks.
        directory = sum(([start, 16]+[0]*10 for start in (0,32,64)), [])
        grid = dict(enabled=True)
        original = view_pvs.YAW_SECTORS
        with patch.object(view_pvs, 'build', return_value=(b'', dict(enabled=True, cells=2, mask_words=6))), \
             patch.object(view_pvs, 'decode', return_value=(65535,)*6):
            data, info = view_pvs.build_coarse([0]*(96*16), grid, .125,
                                               [0,2], [1,0,1,1], directory)
        self.assertEqual(view_pvs.YAW_SECTORS, original)
        expected = b''.join(struct.pack('<4H', value,0,0,0) for value in [3]*8+[6]*8)
        actual = b''.join(struct.pack('<4H', *view_pvs.decode(data,info,key)) for key in range(16))
        self.assertEqual(actual, expected)
        native_cache(data, info, expected)
        with patch.object(view_pvs, 'build', side_effect=ValueError('test failure')):
            with self.assertRaisesRegex(ValueError, 'test failure'):
                view_pvs.build_coarse([], grid, .125, [], [], [])
        self.assertEqual(view_pvs.YAW_SECTORS, original)

    def test_nearby_headings_and_cells_share_blocks_and_keep_dirty_tail_bits(self):
        width = 78
        masks = []
        for i in range(32):
            words = [0]*width
            words[8:16] = [65535]*8
            words[25:28] = [i, i*17, 65535-i]
            words[-1] = i  # All logical bits, including unused group bits, survive.
            masks.append(struct.pack(f'<{width}H', *words))
        keys = list(range(32))*4
        data, info, expected = packed(keys, masks, width)
        self.assertEqual(info['shared_heading_rows'], 1)
        self.assertLess(info['shared_blocks'], 32*info['block_count']//2)
        self.assertLess(len(data), len(masks)*256)
        self.assertEqual(packed(keys, masks, width)[0], data)
        native_cache(data, info, expected)

    def test_small_masks_uniform_blocks_and_arbitrary_entry_order(self):
        rng = random.Random(48213)
        for width in (2, 8, 10, 80):
            masks = [bytes(width*2), b'\xff'*(width*2)]
            masks += [struct.pack(f'<{width}H', *(rng.randrange(65536) for _ in range(width)))
                      for _ in range(9)]
            keys = [rng.randrange(len(masks)) for _ in range(64)]
            data, info, expected = packed(keys, masks, width)
            native_cache(data, info, expected)

    def test_large_root_directory_increases_address_unit_without_truncation(self):
        width = 78
        masks = []
        # Many distinct roots share a small leaf dictionary. Four-byte units
        # would overflow u16; the packer must choose eight-byte root units.
        for i in range(14000):
            words = []
            for shift in range(10):
                words.extend([(i >> shift) & 15]*8)
            masks.append(struct.pack(f'<{width}H', *words[:width]))
        keys = list(range(len(masks)))
        keys += [0]*(-len(keys) % 32)
        data, info, expected = packed(keys, masks, width)
        self.assertEqual(info['root_unit_bytes'], 8)
        native_cache(data, info, expected)

    def test_disabled_grid_preserves_the_existing_runtime_fallback(self):
        data, info = view_pvs.build([], {'enabled': False}, .125)
        self.assertFalse(info['enabled'])
        self.assertEqual(len(data), 128)
        self.assertGreater(info['block_count'], 0)

    def test_budget_failure_falls_back_without_publishing_partial_tables(self):
        grid = dict(enabled=True, columns=1, rows=1, step_q16=65536, origin_q16=[0, 0])
        with patch.object(view_pvs, 'BUDGET_BYTES', 128):
            data, info = view_pvs.build([0]*16, grid, .125, [True])
        self.assertFalse(info['enabled'])
        self.assertEqual(len(data), 128)
        with self.assertRaisesRegex(ValueError, 'complete heading rows'):
            view_pvs.pack([0], [bytes(4)], 2)

    def test_exact_reference_and_native_cache_on_exported_map(self):
        if REAL is None:
            self.skipTest('Supply --reference and --packed for the full map')
        before = REAL.reference/'tankracer-static-assets'
        after = REAL.packed/'tankracer-static-assets'
        old = json.loads((before/'assets.json').read_text())['view_pvs']
        info = json.loads((after/'assets.json').read_text())['view_pvs']
        for field in ('mask_words', 'cells', 'yaw_sectors'):
            self.assertEqual(info[field], old[field])
        original = (before/'map_view_pvs.bin').read_bytes()
        data = (after/'map_view_pvs.bin').read_bytes()
        count = old['cells']*old['yaw_sectors']
        expected = bytearray()
        for key in range(count):
            if old.get('encoding') in ('shared-blocks-v1', 'direct-chunk-masks'):
                raw = struct.pack(f"<{old['mask_words']}H", *view_pvs.decode(original, old, key))
            else:
                ref = struct.unpack_from('<H', original, key*2)[0]
                offset = (old['first_mask_sector'] + ref*old['stride_sectors'])*128
                raw = original[offset:offset + old['mask_words']*2]
            expected.extend(raw)
            self.assertEqual(struct.pack(f"<{old['mask_words']}H", *view_pvs.decode(data, info, key)), raw)
        native_cache(data, info, bytes(expected))


HOST = r'''
use std::cell::UnsafeCell;
use std::ops::{Index, IndexMut};
use std::sync::atomic::{AtomicUsize, Ordering};
static READS: AtomicUsize = AtomicUsize::new(0);
#[derive(Copy, Clone)]
struct Ptr { pointer: *mut u16, words: usize }
impl Ptr {
    fn add(self, n: i16) -> Self {
        assert!(n >= 0 && n as usize <= self.words);
        Self { pointer: unsafe { self.pointer.add(n as usize) }, words: self.words - n as usize }
    }
    unsafe fn as_u16_array(self) -> Array<u16> { Array { pointer: self.pointer, words: self.words } }
    unsafe fn write(self, n: i16, value: u16) {
        assert!(n >= 0 && (n as usize) < self.words);
        *self.pointer.add(n as usize) = value;
    }
}
#[derive(Copy, Clone)]
struct Array<T> { pointer: *mut T, words: usize }
impl Array<u16> {
    fn as_ptr(self) -> Ptr { Ptr { pointer: self.pointer, words: self.words } }
}
impl<T> Index<u16> for Array<T> {
    type Output = T;
    fn index(&self, n: u16) -> &T {
        assert!((n as usize) < self.words);
        unsafe { &*self.pointer.add(n as usize) }
    }
}
impl<T> IndexMut<u16> for Array<T> {
    fn index_mut(&mut self, n: u16) -> &mut T {
        assert!((n as usize) < self.words);
        unsafe { &mut *self.pointer.add(n as usize) }
    }
}
struct Buf<T, const N: usize>(UnsafeCell<[T; N]>);
impl<T, const N: usize> Buf<T, N> {
    fn new(data: [T; N]) -> Self { Self(UnsafeCell::new(data)) }
    fn as_array(&self) -> Array<T> { Array { pointer: self.0.get().cast::<T>(), words: N } }
}
impl<T, const N: usize> Index<u16> for Buf<T, N> {
    type Output = T;
    fn index(&self, n: u16) -> &T { assert!((n as usize) < N); unsafe { &(*self.0.get())[n as usize] } }
}
fn malloc(n: u16) -> Ptr {
    let data = Box::leak(vec![0xa55a_u16; n as usize].into_boxed_slice());
    Ptr { pointer: data.as_mut_ptr(), words: data.len() }
}
#[derive(Copy, Clone)]
struct fix32([u16; 2]);
impl fix32 {
    fn zero() -> Self { Self([0, 0]) }
    fn from_words(lo: u16, hi: u16) -> Self { Self([lo, hi]) }
}
#[derive(Copy, Clone)]
struct vec4([fix32; 4]);
impl vec4 {
    fn new(a: fix32, b: fix32, c: fix32, d: fix32) -> Self { Self([a,b,c,d]) }
    fn export(value: Self, destination: Ptr) {
        for i in 0..8 { unsafe { destination.write(i, value.0[i as usize/2].0[i as usize%2]); } }
    }
}
struct Asset { first_sector: u16 }
const MAP_VIEW_PVS: Asset = Asset { first_sector: 0 };
fn gpu_window_asset_read(mut destination: Array<u16>, sector: u16, word: u16, count: u16) -> bool {
    READS.fetch_add(1, Ordering::Relaxed);
    let start = (sector as usize*64 + word as usize)*2;
    assert!(count as usize <= destination.words && start + count as usize*2 <= DATA.len());
    for i in 0..count { let at = start+i as usize*2; destination[i] = u16::from_le_bytes([DATA[at],DATA[at+1]]); }
    true
}
'''

MAIN = r'''
fn main() {
    let count = EXPECTED.len()/(MAP_PVS_MASK_WORDS as usize*2);
    assert!(count > 0 && count <= 65535);
    let cache = new();
    let mut state = 48931_u32;
    for step in 0..count*2+10000 {
        let key = if step < count { step } else if step < count*2 { count*2-step-1 } else {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 16) as usize % count
        } as u16;
        assert!(load(cache, key));
        let mask = unsafe { cache.as_u16_array() };
        for i in 0..MAP_PVS_MASK_WORDS {
            let at = (key as usize*MAP_PVS_MASK_WORDS as usize+i as usize)*2;
            assert_eq!(mask[i], u16::from_le_bytes([EXPECTED[at],EXPECTED[at+1]]), "key {key}, word {i}");
        }
        let before = READS.load(Ordering::Relaxed);
        assert!(load(cache, key));
        assert_eq!(before, READS.load(Ordering::Relaxed), "same key reread external memory");
    }
}
'''


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--reference', type=Path)
    parser.add_argument('--packed', type=Path)
    args, remaining = parser.parse_known_args()
    if args.reference or args.packed:
        if not args.reference or not args.packed:
            parser.error('supply both --reference and --packed')
        REAL = args
    unittest.main(argv=[__file__, *remaining])
