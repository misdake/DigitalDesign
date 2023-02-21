pub type InstBinaryType = u8;

pub type InstRegType = u8;
pub type InstImmType = u8;

pub struct InstBinary {
    pub binary: InstBinaryType,
    pub desc: &'static InstDesc,
}
impl InstBinary {}

pub struct InstOpcodeDesc4 {
    name: &'static str,
    bits: u8,
}
pub struct InstOpcodeDesc6 {
    name: &'static str,
    bits: u8,
}
pub struct InstOpcodeDesc8 {
    name: &'static str,
    bits: u8,
}
impl InstDesc {
    fn name(&self) -> &'static str {
        match &self {
            InstDesc::Op2(opcode, _, _) => opcode.name,
            InstDesc::Op1(opcode, _) => opcode.name,
            InstDesc::Op0i(opcode, _) => opcode.name,
            InstDesc::Op0(opcode) => opcode.name,
        }
    }
    fn opcode(&self) -> (u8, u8) {
        match &self {
            InstDesc::Op2(opcode, _, _) => (opcode.bits, 4),
            InstDesc::Op1(opcode, _) => (opcode.bits, 6),
            InstDesc::Op0i(opcode, _) => (opcode.bits, 4),
            InstDesc::Op0(opcode) => (opcode.bits, 8),
        }
    }
    fn match_opcode(&self, inst_value: InstBinaryType) -> bool {
        let (bits, len) = self.opcode();
        bits == (inst_value >> (8 - len))
    }
}

pub struct InstRegDesc {}
pub struct InstImmDesc {}

impl InstDesc {
    const fn op2(name: &'static str, opcode: u8) -> InstDesc {
        //TODO assert opcode max
        InstDesc::Op2(
            InstOpcodeDesc4 { name, bits: opcode },
            InstRegDesc {},
            InstRegDesc {},
        )
    }
    const fn op1(name: &'static str, opcode: u8) -> InstDesc {
        InstDesc::Op1(InstOpcodeDesc6 { name, bits: opcode }, InstRegDesc {})
    }
    const fn op0i(name: &'static str, opcode: u8) -> InstDesc {
        InstDesc::Op0i(InstOpcodeDesc4 { name, bits: opcode }, InstImmDesc {})
    }
    const fn op0(name: &'static str, opcode: u8) -> InstDesc {
        InstDesc::Op0(InstOpcodeDesc8 { name, bits: opcode })
    }

    pub fn parse(input: InstBinaryType) -> Option<InstBinary> {
        for inst_desc in ALL_INSTRUCTION_DESC {
            if inst_desc.match_opcode(input) {
                println!("{}", inst_desc.name());
                return Some(InstBinary {
                    binary: input,
                    desc: inst_desc,
                });
            }
        }
        None
    }
}

pub enum InstDesc {
    Op2(InstOpcodeDesc4, InstRegDesc, InstRegDesc),
    Op1(InstOpcodeDesc6, InstRegDesc),
    Op0i(InstOpcodeDesc4, InstImmDesc),
    Op0(InstOpcodeDesc8),
}

use paste::paste;
macro_rules! inst_op2 {
    ($name: ident, $opcode: expr) => {
        paste! {
            const [<INST_ $name:upper>]: InstDesc = InstDesc::op2(stringify!($name), $opcode);
            pub fn [<inst_ $name>](reg1: InstRegType, reg0: InstRegType) -> InstBinary {
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
            const [<INST_ $name:upper>]: InstDesc = InstDesc::op1(stringify!($name), $opcode);
            pub fn [<inst_ $name>](reg0: InstRegType) -> InstBinary {
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
            const [<INST_ $name:upper>]: InstDesc = InstDesc::op0i(stringify!($name), $opcode);
            pub fn [<inst_ $name>](imm: InstImmType) -> InstBinary {
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
            const [<INST_ $name:upper>]: InstDesc = InstDesc::op0(stringify!($name), $opcode);
            pub fn [<inst_ $name>](imm: InstImmType) -> InstBinary {
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
const ALL_INSTRUCTION_DESC: [&'static InstDesc; 30] = [
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
