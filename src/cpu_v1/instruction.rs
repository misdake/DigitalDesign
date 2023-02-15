pub const INST_BIT_LENGTH: u8 = 8;
pub type InstBinaryType = u8;

pub type InstRegType = u8;
pub type InstImmType = u8;

pub struct InstBinary {
    binary: InstBinaryType,
    desc: &'static InstBinaryDesc,
}
impl InstBinary {}

pub enum InstEncoding {
    Op2,
    Op1,
    Op0i,
    Op0,
}
pub enum InstBitType {
    OpCode(u8),
    Reg,
    Imm,
    None,
}
pub struct InstBinaryDesc {
    pub name: &'static str,
    pub encoding: InstEncoding,
    pub opcode: InstBitDesc,
    pub param: [InstBitDesc; 2],
}
pub struct InstBitDesc {
    pub name: &'static str,
    pub bit_count: u8,
    pub bit_type: InstBitType,
}
impl InstBitDesc {
    const fn op(name: &'static str, opcode: u8, bit_count: u8) -> InstBitDesc {
        assert!((1 << bit_count) > (opcode as usize));
        InstBitDesc {
            name,
            bit_count,
            bit_type: InstBitType::OpCode(opcode),
        }
    }
    const fn reg0() -> InstBitDesc {
        InstBitDesc {
            name: "reg0",
            bit_count: 2,
            bit_type: InstBitType::Reg,
        }
    }
    const fn reg1() -> InstBitDesc {
        InstBitDesc {
            name: "reg1",
            bit_count: 2,
            bit_type: InstBitType::Reg,
        }
    }
    const fn imm() -> InstBitDesc {
        InstBitDesc {
            name: "imm",
            bit_count: 4,
            bit_type: InstBitType::Imm,
        }
    }
    const fn empty() -> InstBitDesc {
        InstBitDesc {
            name: "",
            bit_count: 0,
            bit_type: InstBitType::None,
        }
    }
}
impl InstBinaryDesc {
    // different encodings

    const fn op2(name: &'static str, opcode: u8) -> InstBinaryDesc {
        InstBinaryDesc {
            name,
            encoding: InstEncoding::Op2,
            opcode: InstBitDesc::op(name, opcode, 4),
            param: [InstBitDesc::reg1(), InstBitDesc::reg0()],
        }
    }
    const fn op1(name: &'static str, opcode: u8) -> InstBinaryDesc {
        InstBinaryDesc {
            name,
            encoding: InstEncoding::Op1,
            opcode: InstBitDesc::op(name, opcode, 6),
            param: [InstBitDesc::reg0(), InstBitDesc::empty()],
        }
    }
    const fn op0i(name: &'static str, opcode: u8) -> InstBinaryDesc {
        InstBinaryDesc {
            name,
            encoding: InstEncoding::Op0i,
            opcode: InstBitDesc::op(name, opcode, 4),
            param: [InstBitDesc::imm(), InstBitDesc::empty()],
        }
    }
    const fn op0(name: &'static str, opcode: u8) -> InstBinaryDesc {
        InstBinaryDesc {
            name,
            encoding: InstEncoding::Op0,
            opcode: InstBitDesc::op(name, opcode, 8),
            param: [InstBitDesc::empty(), InstBitDesc::empty()],
        }
    }

    pub fn parse(input_inst: InstBinaryType) -> Option<InstBinary> {
        //TODO optimize later
        for inst_desc in ALL_INSTRUCTION_DESC {
            let input_opcode = input_inst >> (INST_BIT_LENGTH - inst_desc.opcode.bit_count);

            match inst_desc.opcode.bit_type {
                InstBitType::OpCode(opcode) => {
                    if input_opcode == opcode {
                        return Some(InstBinary {
                            binary: input_inst,
                            desc: inst_desc,
                        });
                    }
                }
                _ => {}
            }
        }
        None
    }
    pub fn to_string(&self) {}
}

use paste::paste;
macro_rules! inst_op2 {
    ($name: ident, $opcode: expr) => {
        paste! {
            const [<INST_ $name:upper>]: InstBinaryDesc = InstBinaryDesc::op2(stringify!($name), $opcode);
            fn [<inst_ $name>](reg1: InstRegType, reg0: InstRegType) -> InstBinary {
                InstBinary {
                    binary: ($opcode << 4) | (reg1 << 2) | (reg0 << 0),
                    desc: &[<INST_ $name:upper>],
                }
            }
        }
    };
}
macro_rules! inst_op1 {
    ($name: ident, $opcode: expr) => {
        paste! {
            const [<INST_ $name:upper>]: InstBinaryDesc = InstBinaryDesc::op1(stringify!($name), $opcode);
            fn [<inst_ $name>](reg0: InstRegType) -> InstBinary {
                InstBinary {
                    binary: ($opcode << 2) | (reg0 << 0),
                    desc: &[<INST_ $name:upper>],
                }
            }
        }
    };
}
macro_rules! inst_op0i {
    ($name: ident, $opcode: expr) => {
        paste! {
            const [<INST_ $name:upper>]: InstBinaryDesc = InstBinaryDesc::op0i(stringify!($name), $opcode);
            fn [<inst_ $name>](imm: InstImmType) -> InstBinary {
                InstBinary {
                    binary: ($opcode << 4) | (imm << 0),
                    desc: &[<INST_ $name:upper>],
                }
            }
        }
    };
}
macro_rules! inst_op0 {
    ($name: ident, $opcode: expr) => {
        paste! {
            const [<INST_ $name:upper>]: InstBinaryDesc = InstBinaryDesc::op0(stringify!($name), $opcode);
            fn [<inst_ $name>](imm: InstImmType) -> InstBinary {
                InstBinary {
                    binary: ($opcode << 0),
                    desc: &[<INST_ $name:upper>],
                }
            }
        }
    };
}

//TODO test instruction space coverage
//TODO test instruction intersection
const ALL_INSTRUCTION_DESC: [&'static InstBinaryDesc; 30] = [
    &INST_MOV,
    &INST_AND,
    &INST_OR,
    &INST_XOR,
    &INST_ADD,
    &INST_NOT,
    &INST_NEG,
    &INST_INC,
    &INST_DEC,
    &INST_LOAD_IMM,
    &INST_LOAD_MEM_IMM,
    &INST_LOAD_MEM_REG,
    &INST_STORE_MEM_IMM,
    &INST_STORE_MEM_REG,
    &INST_JMP_OFFSET,
    &INST_JE_OFFSET,
    &INST_JL_OFFSET,
    &INST_JG_OFFSET,
    &INST_JMP_REG,
    &INST_JMP_LONG,
    &INST_RESET,
    &INST_HALT,
    &INST_EXTERNAL_SET_SIZE,
    &INST_EXTERNAL_SET_PALETTE,
    &INST_EXTERNAL_SET_POS,
    &INST_EXTERNAL_NEXT_POS,
    &INST_EXTERNAL_SET_COLOR,
    &INST_EXTERNAL_CLEAR,
    &INST_EXTERNAL_PRESENT,
    &INST_EXTERNAL_IS_DONE,
];

inst_op2!(mov, 0b0000);
inst_op2!(and, 0b0001);
inst_op2!(or, 0b0010);
inst_op2!(xor, 0b0011);
inst_op2!(add, 0b0100);

inst_op1!(not, 0b000000);
inst_op1!(neg, 0b000000);
inst_op1!(inc, 0b000000);
inst_op1!(dec, 0b000000);

inst_op0i!(load_imm, 0b0000);
inst_op0i!(load_mem_imm, 0b0000);
inst_op0!(load_mem_reg, 0b00000000);
inst_op0i!(store_mem_imm, 0b0000);
inst_op0!(store_mem_reg, 0b00000000);

inst_op0i!(jmp_offset, 0b0000);
inst_op0i!(je_offset, 0b0000);
inst_op0i!(jl_offset, 0b0000);
inst_op0i!(jg_offset, 0b0000);
inst_op1!(jmp_reg, 0b000000);
inst_op0i!(jmp_long, 0b0000);

inst_op0!(reset, 0b00000000);
inst_op0!(halt, 0b00000000);

inst_op0!(external_set_size, 0b00000000);
inst_op0!(external_set_palette, 0b00000000);
inst_op0!(external_set_pos, 0b00000000);
inst_op0!(external_next_pos, 0b00000000);
inst_op0!(external_set_color, 0b00000000);
inst_op0!(external_clear, 0b00000000);
inst_op0!(external_present, 0b00000000);
inst_op0!(external_is_done, 0b00000000);
