// bench-max-cycles: 100000
// bench-expected-halt: 25180
// bench-tier: short
use crate::dsl_rt::*;

// Classic FizzBuzz over 1..=64, but instead of printing, a per-line tag
// (fizz/buzz/fizzbuzz/number) is XOR-accumulated into a rotating checksum.
const N: u16 = 64;

fn main() {
    let mut cs: u16 = 0x5a5a;
    let mut c3: u16 = 2;
    let mut c5: u16 = 4;
    let mut i: u16 = 1;
    while i <= N {
        let tag = if c3 == 0 && c5 == 0 {
            0xf000
        } else {
            if c3 == 0 {
                0x0f00
            } else {
                if c5 == 0 { 0x00f0 } else { i }
            }
        };
        cs = cs ^ tag;
        cs = (cs << 1) | (cs >> 15);
        c3 = if c3 == 0 { 2 } else { c3 - 1 };
        c5 = if c5 == 0 { 4 } else { c5 - 1 };
        i = i + 1;
    }
    halt(cs);
}
