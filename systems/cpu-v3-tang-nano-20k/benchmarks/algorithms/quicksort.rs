use crate::dsl_rt::*;

const N: u16 = 4096;
static DATA: [u16; 4096] = [0; 4096];

fn qsort(mut d: Array<u16>, lo: u16, hi: u16) {
    if lo < hi {
        let mid = (lo + hi) >> 1;
        let tmp = d[mid];
        d[mid] = d[hi];
        d[hi] = tmp;
        let pivot = d[hi];
        let mut i: u16 = lo;
        let mut j: u16 = lo;
        while j < hi {
            if d[j] < pivot {
                let swap = d[i];
                d[i] = d[j];
                d[j] = swap;
                i = i + 1;
            }
            j = j + 1;
        }
        let swap = d[i];
        d[i] = d[hi];
        d[hi] = swap;
        if lo < i { qsort(d, lo, i - 1); }
        if i < hi { qsort(d, i + 1, hi); }
    }
}
fn checksum(d: Array<u16>) -> u16 {
    let mut sum: u16 = 0;
    let mut i: u16 = 0;
    while i < N { sum = sum + d[i]; i = i + 1; }
    sum
}
fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    while i < N {
        d[i] = (i << 5) ^ (i >> 6) ^ (i << 11) ^ 0x9e37;
        i = i + 1;
    }
    let before = checksum(d);
    qsort(d, 0, N - 1);
    i = 1;
    while i < N {
        if d[i - 1] > d[i] { halt(0); }
        i = i + 1;
    }
    if before == checksum(d) { halt(1); } else { halt(0); }
}
