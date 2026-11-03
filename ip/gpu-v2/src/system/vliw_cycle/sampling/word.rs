//! Explicit known bits for the selected 72/76-bit transport words.
use super::super::vertex::Bits;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Word {
    pub value: u128,
    pub known: u128,
}
impl Word {
    pub fn new(value: u128, width: u32) -> Self {
        let mask = (1u128 << width) - 1;
        Self {
            value: value & mask,
            known: mask,
        }
    }
    pub fn slice(self, lo: u32, width: u32) -> Bits {
        let mask = (1u128 << width) - 1;
        Bits {
            value: ((self.value >> lo) & mask) as u64,
            known: ((self.known >> lo) & mask) as u64,
        }
    }
    pub fn is_known(self, width: u32) -> bool {
        self.known & ((1u128 << width) - 1) == (1u128 << width) - 1
    }
    pub fn set(&mut self, lo: u32, width: u32, data: Bits) {
        let mask = ((1u128 << width) - 1) << lo;
        self.value = (self.value & !mask) | ((u128::from(data.value) << lo) & mask);
        self.known = (self.known & !mask) | ((u128::from(data.known) << lo) & mask);
    }
}
