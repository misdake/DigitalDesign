use crate::dsl_rt::*;

const N: u16 = 2048;
static DATA: [u16; 2048] = [0; 2048];

fn find(d: Array<u16>, target: u16) -> u16 {
    let mut lo: u16 = 0;
    let mut hi: u16 = N;
    while lo < hi {
        let mid = lo + ((hi - lo) >> 1);
        if d[mid] < target { lo = mid + 1; } else { hi = mid; }
    }
    if lo < N && d[lo] == target { lo } else { 0xffff }
}

fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    while i < N { d[i] = i << 1; i = i + 1; }
    i = 0;
    let mut checksum: u16 = 0;
    while i < 512 {
        let target = ((i << 5) + (i << 2) + i) & 0x0fff;
        let even = target & 0xfffe;
        let found = find(d, even);
        if found == 0xffff || d[found] != even { halt(0); }
        checksum = checksum ^ found;
        i = i + 1;
    }
    if checksum == 512 { halt(1); } else { halt(0); }
}
