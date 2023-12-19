#![allow(unused)]

type Op4 = u8;
type Op8 = u8;
type Reg = u8;
type Imm4 = u8;
type Imm8Lo = u8;
type Imm8Hi = u8;

pub type E30Param = (Reg, Reg, Reg);
pub type E21Param = (Imm4, Reg, Reg);
pub type E20Param = (Reg, Reg);
//TODO more

#[derive(Copy, Clone)]
enum InstEncoded {
    E30(&'static str, Op4, E30Param),
    E21(&'static str, Op4, E21Param),
    E20(&'static str, Op8, E20Param),
    //TODO more
}

impl Display for InstEncoded {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            InstEncoded::E30(name, _, (reg2, reg1, reg0)) => {
                format!("r{reg0} <- r{reg1} {name} r{reg2}")
            }
            InstEncoded::E21(name, _, (imm4, reg1, reg0)) => {
                format!("r{reg0} <- r{reg1} {name} r{imm4}")
            }
            InstEncoded::E20(name, _, (reg1, reg0)) => {
                format!("r{reg0} <- {name} r{reg1}")
            } //TODO more
        })
    }
}

impl InstEncoded {
    fn to_binary(self) -> InstBinaryType {
        let r = match self {
            InstEncoded::E30(_, op4, (reg2, reg1, reg0)) => {
                ((op4 as u32) << 12) | ((reg2 as u32) << 8) | ((reg1 as u32) << 4) | (reg0 as u32)
            }
            InstEncoded::E21(_, op4, (imm4, reg1, reg0)) => {
                ((op4 as u32) << 12) | ((imm4 as u32) << 8) | ((reg1 as u32) << 4) | (reg0 as u32)
            }
            InstEncoded::E20(_, op8, (reg1, reg0)) => {
                ((op8 as u32) << 8) | ((reg1 as u32) << 4) | (reg0 as u32)
            } //TODO more
        };
        r as InstBinaryType
    }
}

pub type InstBinaryType = u16;

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
fn part12(binary: InstBinaryType) -> u8 {
    (part1(binary) << 4) | part2(binary)
}
fn part10(binary: InstBinaryType) -> u8 {
    (part1(binary) << 4) | part0(binary)
}
fn part32(binary: InstBinaryType) -> u8 {
    (part3(binary) << 4) | part2(binary)
}

fn match_e30(binary: InstBinaryType, op4: u8) -> Option<E30Param> {
    if op4 == part3(binary) {
        let reg2 = part2(binary);
        let reg1 = part1(binary);
        let reg0 = part0(binary);
        Some((reg2, reg1, reg0))
    } else {
        None
    }
}
fn match_e21(binary: InstBinaryType, op4: u8) -> Option<E21Param> {
    if op4 == part3(binary) {
        let imm4 = part2(binary);
        let reg1 = part1(binary);
        let reg0 = part0(binary);
        Some((imm4, reg1, reg0))
    } else {
        None
    }
}
fn match_e20(binary: InstBinaryType, op8: u8) -> Option<E20Param> {
    if op8 == part32(binary) {
        let reg1 = part1(binary);
        let reg0 = part0(binary);
        Some((reg1, reg0))
    } else {
        None
    }
}

macro_rules! define_isa {
    ($enum_name:ident, $(($encoding: ident, $opcode: expr, $name: ident),)*) => {
        paste! {
            #[allow(non_camel_case_types)]
            #[derive(Copy, Clone)]
            pub enum $enum_name {
                $($name( [<$encoding Param>] )),+
            }

            impl $enum_name {
                fn to_encoded(self) -> InstEncoded {
                    use $enum_name::*;
                    match self {
                        $($name(param) => InstEncoded::$encoding(stringify!($name), $opcode, param)),+
                    }
                }
                pub fn parse(binary: InstBinaryType) -> Option<Self> {
                    use $enum_name::*;
                    $(if let Some(param) = [<match_$encoding:lower>](binary, $opcode) { return Some($name(param)); })+
                    None
                }
            }
        }
    };
}

use paste::*;
use std::fmt::{Display, Formatter};
define_isa!(
    Instruction,
    //TODO opcode
    (E30, 0b0001, and),
    (E30, 0b0010, or),
    (E30, 0b0011, xor),
    (E30, 0b0100, add),
    (E20, 0b0000, mov),
    // (E20, 0b0000, inv),
    // (E20, 0b0000, neg),
    // (E20, 0b0000, inc),
    //TODO more
);

impl Default for Instruction {
    fn default() -> Self {
        Instruction::mov((0, 0))
    }
}
impl Display for Instruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.to_encoded().fmt(f)
    }
}
impl Instruction {
    pub fn to_binary(self) -> InstBinaryType {
        self.to_encoded().to_binary()
    }
}
