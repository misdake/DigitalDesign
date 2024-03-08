#![allow(unused)]
use crate::isa::InstBinaryType;
use cpu_macro::define_isa;

pub type Reg = u8;
pub type Imm4 = u8;
pub type Flag4 = u8;

fn part3(binary: InstBinaryType) -> u8 {
    ((binary >> 12) & 0b1111) as u8
}
fn part2(binary: InstBinaryType) -> u8 {
    ((binary >> 8) & 0b1111) as u8
}
fn part1(binary: InstBinaryType) -> u8 {
    ((binary >> 4) & 0b1111) as u8
}
fn part0(binary: InstBinaryType) -> u8 {
    (binary & 0b1111) as u8
}

define_isa! {
    Isa
    (mov 0x12 OORR "r{0} = r{1}")
    (add 0x3 ORRR "r{0} = r{1} + r{2}")
}
