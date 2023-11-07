pub type InstBinaryType = u8;

fn match_op2(binary: InstBinaryType, op4: u8) -> Option<(RegisterIndex, RegisterIndex)> {
    if op4 == binary >> 4 {
        let reg1 = unsafe { std::mem::transmute((binary & 0b1100) >> 2) };
        let reg0 = unsafe { std::mem::transmute(binary & 0b0011) };
        Some((reg1, reg0))
    } else {
        None
    }
}
fn match_op1(binary: InstBinaryType, op6: u8) -> Option<RegisterIndex> {
    if op6 == binary >> 2 {
        let reg0 = unsafe { std::mem::transmute(binary & 0b11) };
        Some(reg0)
    } else {
        None
    }
}
fn match_op0i4(binary: InstBinaryType, op4: u8) -> Option<Imm4> {
    if op4 == binary >> 4 {
        let imm4 = unsafe { std::mem::transmute(binary & 0b1111) };
        Some(imm4)
    } else {
        None
    }
}
fn match_op0i3(binary: InstBinaryType, op5: u8) -> Option<Imm3> {
    if op5 == binary >> 3 {
        let imm3 = unsafe { std::mem::transmute(binary & 0b111) };
        Some(imm3)
    } else {
        None
    }
}
fn match_op0(binary: InstBinaryType, op8: u8) -> Option<()> {
    if op8 == binary {
        Some(())
    } else {
        None
    }
}

pub type Op2Param = (RegisterIndex, RegisterIndex);
pub type Op1Param = RegisterIndex;
pub type Op0i4Param = Imm4;
pub type Op0i3Param = Imm3;
pub type Op0Param = ();

#[derive(Copy, Clone)]
enum InstEncoded {
    Op2(&'static str, u8, (RegisterIndex, RegisterIndex)),
    Op1(&'static str, u8, RegisterIndex),
    Op0i4(&'static str, u8, Imm4),
    Op0i3(&'static str, u8, Imm3),
    Op0(&'static str, u8, ()),
}
impl InstEncoded {
    fn to_string(self) -> String {
        match self {
            InstEncoded::Op2(name, _, (reg1, reg0)) => {
                format!("{} r{} <- r{}", name, reg0 as u8, reg1 as u8)
            }
            InstEncoded::Op1(name, _, reg0) => format!("{} r{}", name, reg0 as u8),
            InstEncoded::Op0i4(name, _, imm4) => format!("{} 0b{:04b}({})", name, imm4, imm4),
            InstEncoded::Op0i3(name, _, imm3) => format!("{} 0b{:03b}({})", name, imm3, imm3),
            InstEncoded::Op0(name, _, _) => format!("{}", name),
        }
    }
    fn to_binary(self) -> InstBinaryType {
        match self {
            InstEncoded::Op2(_, opcode4, (reg1, reg0)) => {
                (opcode4 << 4) | ((reg1 as u8) << 2) | ((reg0 as u8) << 0)
            }
            InstEncoded::Op1(_, opcode6, reg0) => (opcode6 << 2) | ((reg0 as u8) << 0),
            InstEncoded::Op0i4(_, opcode4, imm4) => (opcode4 << 4) | (imm4 << 0),
            InstEncoded::Op0i3(_, opcode5, imm3) => (opcode5 << 3) | (imm3 << 0),
            InstEncoded::Op0(_, opcode8, _) => opcode8,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone)]
pub enum RegisterIndex {
    Reg0 = 0,
    Reg1,
    Reg2,
    Reg3,
}
pub type Imm3 = u8;
pub type Imm4 = u8;

use paste::paste;

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

define_isa!(
    Instruction,
    (Op2, 0b0000, mov),
    (Op2, 0b0001, and),
    (Op2, 0b0010, or),
    (Op2, 0b0011, xor),
    (Op2, 0b0100, add),
    (Op1, 0b010100, inv),
    (Op1, 0b010101, neg),
    (Op1, 0b010110, dec),
    (Op1, 0b010111, inc),
    (Op0i4, 0b1000, load_imm),
    (Op0i4, 0b1001, load_mem),
    (Op0i4, 0b1010, store_mem),
    (Op0i4, 0b1011, jmp_long),
    (Op0i4, 0b1100, jmp_offset),
    (Op0i4, 0b1101, jne_offset),
    (Op0i4, 0b1110, jl_offset),
    (Op0i4, 0b1111, jg_offset),
    (Op0, 0b01100000, reset),
    (Op0, 0b01100001, halt),
    (Op0, 0b01100010, sleep),
    (Op0, 0b01100011, set_mem_page),
    (Op0, 0b01100100, set_bus_addr0),
    (Op0, 0b01100101, set_bus_addr1),
    (Op0i3, 0b01110, bus0),
    (Op0i3, 0b01111, bus1),
);
impl Default for Instruction {
    fn default() -> Self {
        use RegisterIndex::*;
        Instruction::mov((Reg0, Reg0))
    }
}
impl Instruction {
    pub fn to_binary(self) -> InstBinaryType {
        self.to_encoded().to_binary()
    }
    pub fn to_string(self) -> String {
        self.to_encoded().to_string()
    }
}
