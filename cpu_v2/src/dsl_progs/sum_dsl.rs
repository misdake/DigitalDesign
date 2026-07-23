//! rcc subset sample: sum 1..=10, halt with the result.
//! compiled by `compiler::frontend` tests; also valid host Rust.

use crate::dsl_rt::*;

fn main() {
    let mut sum: u16 = 0;
    for i in 1..=10u16 {
        sum += i;
    }
    halt(sum);
}
