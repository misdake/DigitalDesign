//! One shared game LCG; callers own the seed and exact consumption order.
use crate::dsl_rt::*;
pub fn next(seed: fix32) -> fix32 {
    fmul_lo32(seed, fix32::from_words(26125, 25)) + fix32::from_words(62303, 15470)
}
