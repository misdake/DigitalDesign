//! Representative single-file rcc benchmark suite: broad scalar coverage,
//! CRC-16, and in-place recursive quicksort.

use crate::dsl_rt::*;

static BASIC_RESULT: u16 = 0;
static CRC_WORDS: [u16; 12] = [
    0x1234, 0xabcd, 0x0001, 0xffff, 0x55aa, 0x0f0f, 0x8001, 0x2468, 0x1357, 0xbeef, 0xcafe, 0x0102,
];
static SORT_DATA: [u16; 16] = [42, 7, 19, 3, 88, 1, 55, 34, 13, 5, 21, 8, 2, 77, 11, 6];

#[allow(clippy::collapsible_else_if)]
fn clamp_i16(x: i16, low: i16, high: i16) -> i16 {
    if x < low {
        low
    } else {
        if x > high {
            high
        } else {
            x
        }
    }
}

#[allow(clippy::manual_rotate)]
fn mix(x: u16, y: u16) -> u16 {
    let value = (x + y) ^ 0x5a5a;
    (value << 3) | (value >> 13)
}

fn benchmark_basics() -> u16 {
    let values: [u16; 8] = [3, 14, 15, 9, 26, 5, 35, 8];
    let mut data = values.as_array();
    let mut acc: u16 = 0x1234;

    for i in 0..8u16 {
        let value = data[i];
        acc = mix(acc, value);
        if (value & 1) != 0 && i < 6 {
            acc ^= i << 2;
        } else {
            acc += i;
        }
    }

    let mut n: u16 = 3;
    while n > 0 {
        acc += n;
        n -= 1;
    }

    let signed = clamp_i16((-17i16) >> 1, -8, 7);
    let ones = cnt1(acc);
    let highest = log2(acc | 1);
    data[0u16] = acc;

    let result = data[0u16] ^ (signed as u16) ^ ones ^ highest;
    addr_of(&BASIC_RESULT).write(0, result);
    BASIC_RESULT
}

fn crc16_word(crc: u16, word: u16) -> u16 {
    let mut value = crc ^ word;
    for _bit in 0..16u16 {
        if (value & 0x8000) != 0 {
            value = (value << 1) ^ 0x1021;
        } else {
            value <<= 1;
        }
    }
    value
}

fn benchmark_crc16() -> u16 {
    let mut crc: u16 = 0xffff;
    for i in 0..12u16 {
        crc = crc16_word(crc, CRC_WORDS.as_array()[i]);
    }
    crc
}

fn swap(mut data: Array<u16>, a: u16, b: u16) {
    let left = data[a];
    let right = data[b];
    data[a] = right;
    data[b] = left;
}

fn partition(data: Array<u16>, low: u16, high: u16) -> u16 {
    let pivot = data[high];
    let mut store = low;
    let mut scan = low;
    while scan < high {
        let value = data[scan];
        if value <= pivot {
            swap(data, store, scan);
            store += 1;
        }
        scan += 1;
    }
    swap(data, store, high);
    store
}

fn quick_sort(data: Array<u16>, low: u16, high: u16) {
    if low < high {
        let pivot = partition(data, low, high);
        if pivot > 0 {
            quick_sort(data, low, pivot - 1);
        }
        quick_sort(data, pivot + 1, high);
    }
}

fn benchmark_quicksort() -> u16 {
    let data = SORT_DATA.as_array();
    quick_sort(data, 0, 15);

    let mut checksum: u16 = 0;
    let mut disorder: u16 = 0;
    let mut previous = data[0u16];
    for i in 0..16u16 {
        let current = data[i];
        if i > 0 && previous > current {
            disorder = 0xffff;
        }
        checksum = (checksum << 1) ^ current;
        previous = current;
    }
    checksum ^ disorder
}

fn main() {
    let basics = benchmark_basics();
    let crc16 = benchmark_crc16();
    let quicksort = benchmark_quicksort();
    halt(basics ^ crc16 ^ quicksort);
}
