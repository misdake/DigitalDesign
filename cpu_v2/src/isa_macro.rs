use cpu_macro::define_isa;
use crate::isa::InstBinaryType;

// define_isa! {
//     Isa
//     (mov 0b0001 ORRR "r{0} = r{1}")
//     (add 0b0001 ORRR "r{0} = r{1} + r{2}")
// }

//TODO ?
type Op4 = u8;
type Op8 = u8;
type Reg = u8;
type Flag = u8;
type Imm4 = u8;
type Flag4 = u8;
type Imm8Lo = u8;
type Imm8Hi = u8;

#[allow(non_camel_case_types)] //TODO allow
pub enum Isa {
    mov(Reg, Reg, Reg),
    add(Reg, Reg, Reg),
}

impl Isa {
    pub fn encode(&self) -> u16 {
        use Isa::*; //TODO Isa
        match self {
            mov(reg2, reg1, reg0) => (0b0001 as u16 << 12) | (reg2 << 8) | (reg1 << 4) | (reg0 << 0), //TODO as u16
            add(reg2, reg1, reg0) => (0b0001 << 12) | (reg2 << 8) | (reg1 << 4) | (reg0 << 0),
        }
    }
    pub fn parse(inst: InstBinaryType) -> Self {
        use Isa::*; //TODO Isa
        if part3(inst) == 0b0001 {
            return mov(part2(inst), part1(inst), part0(inst));
        }
        if part3(inst) == 0b0001 { //TODO (inst)
            return add(part2(inst), part1(inst), part0(inst));
        }
        unreachable!()
    }
    //TODO display
}
//TODO constructor

//TODO fn
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