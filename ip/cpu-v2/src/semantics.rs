use crate::{FLAGS_EQUAL, FLAGS_GREATER, FLAGS_LESS};

#[rustfmt::skip]
pub fn calc_flags(x: u16, y: u16) -> u8 {
    let mut flags = 0;
    if x > y { flags |= FLAGS_GREATER; }
    if x == y { flags |= FLAGS_EQUAL; }
    if x < y { flags |= FLAGS_LESS; }
    flags
}

pub fn calc_flags_signed(x: u16, y: u16) -> u8 {
    calc_flags(x ^ 0x8000, y ^ 0x8000)
}
