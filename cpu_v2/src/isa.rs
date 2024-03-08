#![allow(unused)]

use cpu_macro::define_isa;

pub type InstBinaryType = u16;
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
    Instruction
    (and  0x0 ORRR "r{0} = r{1} & r{2}")
    (or   0x1 ORRR "r{0} = r{1} | r{2}")
    (xor  0x2 ORRR "r{0} = r{1} ^ r{2}")
    (add  0x3 ORRR "r{0} = r{1} + r{2}")
    (sub  0x4 ORRR "r{0} = r{1} - r{2}")
    (addi 0x5 OIRR "r{0} = r{1} + 0x{2:x}")
    (shlu 0x6 OIRR "r{0} = r{1} >> {2}")
    (shru 0x7 OIRR "r{0} = r{1} << {2}")
    (mov  0x80 OORR "r{0} = r{1}")
    (inv  0x81 OORR "r{0} = !r{1}")
    (neg  0x82 OORR "r{0} = -r{1}")
    (not0 0x83 OORR "r{0} = !!r{1}")
    (load_hi 0x9 OIIR "r{0}_hi = 0x{2:x}{1:x}")
    (load_lo 0xa OIIR "r{0}_lo = 0x{2:x}{1:x}")
}

#[test]
fn test_print() {
    let inst = load_hi(0x2, 0x1, 0);
    println!("inst: {}\nbinary: {:4x}", inst, inst.encode());
}
