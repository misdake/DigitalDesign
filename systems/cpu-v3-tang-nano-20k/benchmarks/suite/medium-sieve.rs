// bench-max-cycles: 2000000
// bench-expected-halt: 1
// bench-tier: medium
use crate::dsl_rt::*;

const N: u16 = 2000;
static COMPOSITE: [u16; 2000] = [0; 2000];

fn main() {
    let mut composite = COMPOSITE.as_array();
    let mut p: u16 = 2;
    while p < 45 {
        if composite[p] == 0 {
            let mut multiple: u16 = p * p;
            while multiple < N {
                composite[multiple] = 1;
                multiple = multiple + p;
            }
        }
        p = p + 1;
    }
    let mut count: u16 = 0;
    p = 2;
    while p < N {
        if composite[p] == 0 { count = count + 1; }
        p = p + 1;
    }
    if count == 303 { halt(1); } else { halt(0); }
}
