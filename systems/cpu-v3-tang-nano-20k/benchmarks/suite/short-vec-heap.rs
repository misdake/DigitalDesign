// bench-max-cycles: 100000
// bench-expected-halt: 32788
// bench-tier: short
use crate::dsl_rt::*;
use crate::rcc_std::*;

// vec (heap-backed dynamic array) plus explicit malloc/free: allocate, fill,
// transform, and free in a loop; exact checksum of the retained totals.
fn main() {
    let mut total: u16 = 0;
    let mut round: u16 = 0;
    while round < 6 {
        let v = vec_new();
        let mut i: u16 = 0;
        while i < 12 {
            vec_push(v, (i ^ (round << 2)) & 63);
            i = i + 1;
        }
        let block = malloc(8);
        let mut j: u16 = 0;
        while j < 8 {
            block.write(j, vec_get(v, j) ^ 0x11);
            j = j + 1;
        }
        j = 0;
        while j < 8 {
            total = total ^ block.read(j);
            j = j + 1;
        }
        free(block);
        while vec_len(v) > 0 {
            total = total ^ vec_pop(v);
        }
        vec_free(v);
        round = round + 1;
    }
    halt(total);
}
