#![allow(unused)]

type Op4 = u8;
type Op8 = u8;
type Reg = u8;
type Flag = u8;
type Imm4 = u8;
type Flag4 = u8;
type Imm8Lo = u8;
type Imm8Hi = u8;

pub type RRRParam = (Reg, Reg, Reg);
pub type IRRParam = (Imm4, Reg, Reg);
pub type RRParam = (Reg, Reg);
pub type IIRParam = (Imm8Hi, Imm8Lo, Reg);
pub type IRParam = (Imm4, Reg);
pub type IIFParam = (Imm8Hi, Imm8Lo, Flag4);
pub type IFParam = (Imm4, Flag4);

#[derive(Copy, Clone)]
enum InstEncoded {
    RRR(&'static str, Op4, RRRParam),
    IRR(&'static str, Op4, IRRParam),
    RR(&'static str, Op8, RRParam),
    IIR(&'static str, Op4, IIRParam),
    IR(&'static str, Op8, IRParam),
    IIF(&'static str, Op4, IIFParam),
    IF(&'static str, Op8, IFParam),
}

// TODO impl Display for Instruction
// impl Display for InstEncoded {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         f.write_str(&match self {
//             InstEncoded::RRR(name, _, (reg2, reg1, reg0)) => {
//                 format!("r{reg0} <- r{reg1} {name} r{reg2}")
//             }
//             InstEncoded::RRI(name, _, (imm, reg1, reg0)) => {
//                 format!("r{reg0} <- r{reg1} {name} {imm:04b}({imm})")
//             }
//             InstEncoded::RR(name, _, (reg1, reg0)) => {
//                 format!("r{reg0} <- {name} r{reg1}")
//             }
//             InstEncoded::RII(name, _, (hi, lo, reg0)) => {
//                 format!("r{reg0} <- {name} {hi:04b} {lo:04b} ({})", (*hi << 4) | *lo)
//             }
//             InstEncoded::RI(name, _, (op, reg0)) => {
//                 format!("r{reg0} <- {name} op{op:04b}")
//             }
//             InstEncoded::FII(name, _, (imm, op)) => {
//                 format!("{name} op{op:04b} {imm:04b}({imm})")
//             }
//             InstEncoded::FI(name, _, (reg1, flag)) => {
//                 format!("{name} flag{flag:04b} r{reg1}")
//             }
//             InstEncoded::E02J(name, _, (hi, lo, flag)) => {
//                 format!(
//                     "{name} flag{flag:04b} {hi:04b} {lo:04b} ({})",
//                     (*hi << 4) | *lo
//                 )
//             }
//         })
//     }
// }

impl InstEncoded {
    fn to_binary(self) -> InstBinaryType {
        let r = match self {
            InstEncoded::RRR(_, op4, (reg2, reg1, reg0)) => {
                ((op4 as u32) << 12) | ((reg2 as u32) << 8) | ((reg1 as u32) << 4) | (reg0 as u32)
            }
            InstEncoded::IRR(_, op4, (imm, reg1, reg0)) => {
                ((op4 as u32) << 12) | ((imm as u32) << 8) | ((reg1 as u32) << 4) | (reg0 as u32)
            }
            InstEncoded::RR(_, op8, (reg1, reg0)) => {
                ((op8 as u32) << 8) | ((reg1 as u32) << 4) | (reg0 as u32)
            }
            InstEncoded::IIR(_, op4, (hi, lo, reg0)) => {
                ((op4 as u32) << 12) | ((hi as u32) << 8) | ((lo as u32) << 4) | (reg0 as u32)
            }
            InstEncoded::IR(_, op8, (op, reg0)) => {
                ((op8 as u32) << 8) | ((op as u32) << 4) | (reg0 as u32)
            }
            InstEncoded::IIF(_, op4, (hi, lo, flag)) => {
                ((op4 as u32) << 12) | ((hi as u32) << 8) | ((lo as u32) << 4) | (flag as u32)
            }
            InstEncoded::IF(_, op8, (reg1, flag)) => {
                ((op8 as u32) << 8) | ((reg1 as u32) << 4) | (flag as u32)
            }
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

fn match_rrr(binary: InstBinaryType, op4: u8) -> Option<RRRParam> {
    if op4 == part3(binary) {
        let reg2 = part2(binary);
        let reg1 = part1(binary);
        let reg0 = part0(binary);
        Some((reg2, reg1, reg0))
    } else {
        None
    }
}
fn match_irr(binary: InstBinaryType, op4: u8) -> Option<IRRParam> {
    if op4 == part3(binary) {
        let imm4 = part2(binary);
        let reg1 = part1(binary);
        let reg0 = part0(binary);
        Some((imm4, reg1, reg0))
    } else {
        None
    }
}
fn match_rr(binary: InstBinaryType, op8: u8) -> Option<RRParam> {
    if op8 == part32(binary) {
        let reg1 = part1(binary);
        let reg0 = part0(binary);
        Some((reg1, reg0))
    } else {
        None
    }
}
fn match_iir(binary: InstBinaryType, op4: u8) -> Option<IIRParam> {
    if op4 == part3(binary) {
        let hi = part2(binary);
        let lo = part1(binary);
        let reg0 = part0(binary);
        Some((hi, lo, reg0))
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

fn encode4444(op4: Op4, b2: u8, b1: u8, b0: u8) -> InstBinaryType {
    (((op4 as u32) << 12) | ((b2 as u32) << 8) | ((b1 as u32) << 4) | (b0 as u32)) as InstBinaryType
}
fn encode844(op8: Op8, b1: u8, b0: u8) -> InstBinaryType {
    (((op8 as u32) << 8) | ((b1 as u32) << 4) | (b0 as u32)) as InstBinaryType
}

macro_rules! inst_param_type {
    (R) => {
        Reg
    };
    (I) => {
        Imm
    };
    (F) => {
        Flag
    };
}

//TODO see impl
// macro_rules! define_isa2 {
//     ($enum_name:ident, $(($name: ident, $opcode: expr, $a:ident $b:ident $($c:ident)?),)+) => {
//         #[allow(non_camel_case_types)]
//         #[derive(Copy, Clone)]
//         pub enum $enum_name {
//             $(
//             $name( inst_param_type!($a), inst_param_type!($b) $(, inst_param_type!($c))? )
//             ),+
//         }
//         impl $enum_name {
//             fn encode(self) -> InstBinaryType {
//                 use $enum_name::*;
//                 match self {
//                     $(
//                     $name( $( paste!{[<b2$c>]}, )? b1, b0 ) => inst_encode!($opcode, $( paste!{[<b2$c>]}, )? b1, b0),
//                     ),+
//                 }
//             }
//         }
//     };
// }

//TODO encoding to megatype?

macro_rules! inst_enum {
    ($enum_name:ident, $( $inst_name:ident,  ,)+) => {
        #[allow(non_camel_case_types)]
        #[derive(Copy, Clone)]
        pub enum $enum_name {}
    };
}
//TODO define for each encoding megatype
macro_rules! inst_encode {
    ($val:expr, $name:ident, $op:expr, RRR) => {
        match &$val {
            $name(b2, b1, b0) => return encode4444($op, *b2, *b1, *b0),
            _ => {}
        }
    };
}
macro_rules! inst_parse {
    ($binary:expr, $name:ident, $op4:expr, RRR) => {
        if $op4 == part3($binary) {
            return Some($name(part2($binary), part1($binary), part0($binary)));
        }
    };
}
//TODO define for each encoding megatype
macro_rules! inst_string {
    ($val:expr, $name:ident, RRR, $format:expr) => {
        match &$val {
            $name(b2, b1, b0) => return format!($format, *b0, *b1, *b2),
            _ => {}
        }
    };
}

// define_isa2!(
//     InstX,
//     (add, 0b0000, RRR, "r{0} = r{1} + r{2}"),
// );
#[allow(non_camel_case_types)]
#[derive(Copy, Clone)]
pub enum InstX {
    add(Reg, Reg, Reg),
    addi(Reg, Imm4),
}
impl InstX {
    fn encode(self) -> InstBinaryType {
        use InstX::*;
        inst_encode!(self, add, 0b0000, RRR);
        //TODO more
        unreachable!()
    }
    fn parse(binary: InstBinaryType) -> Option<Self> {
        use InstX::*;
        inst_parse!(binary, add, 0b0000, RRR);
        //TODO more
        None
    }
    fn string(&self) -> String {
        use InstX::*;
        inst_string!(self, add, RRR, "r{0} = r{1} + r{2}");
        //TODO more
        unreachable!()
    }
}

use paste::*;
use std::fmt::{Display, Formatter};
define_isa!(
    Instruction,
    //TODO opcode
    (RRR, 0b0000, and),
    (RRR, 0b0000, or),
    (RRR, 0b0000, xor),
    (RRR, 0b0000, add),
    (RRR, 0b0000, sub),
    (IRR, 0b0000, addi),
    (IRR, 0b0000, shlu),
    (IRR, 0b0000, shru),
    (RR, 0b00000000, mov),
    (RR, 0b00000000, inv),
    (RR, 0b00000000, neg),
    (RR, 0b00000000, not0),
    (IIR, 0b0000, load_hi),
    (IIR, 0b0000, load_lo),
    //TODO more
);

// impl Default for Instruction {
//     fn default() -> Self {
//         Instruction::mov((0, 0))
//     }
// }
// impl Display for Instruction {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         self.to_encoded().fmt(f)
//     }
// }
// impl Instruction {
//     pub fn to_binary(self) -> InstBinaryType {
//         self.to_encoded().to_binary()
//     }
// }
