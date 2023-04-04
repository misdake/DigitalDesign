pub type InstBinaryType = u8;

pub type InstRegType = u8;
pub type InstImmType = u8;

#[derive(Copy, Clone)]
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
    pub fn name(&self) -> &'static str {
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
    pub fn match_opcode(&self, inst_value: InstBinaryType) -> bool {
        let (bits, len) = self.opcode();
        bits == (inst_value >> (8 - len))
    }
}

pub struct InstRegDesc {}
pub struct InstImmDesc {}

impl InstDesc {
    const fn op2(name: &'static str, opcode: u8) -> InstDesc {
        assert!(opcode < (1 << 4));
        InstDesc::Op2(
            InstOpcodeDesc4 { name, bits: opcode },
            InstRegDesc {},
            InstRegDesc {},
        )
    }
    const fn op1(name: &'static str, opcode: u8) -> InstDesc {
        assert!(opcode < (1 << 6));
        InstDesc::Op1(InstOpcodeDesc6 { name, bits: opcode }, InstRegDesc {})
    }
    const fn op0i(name: &'static str, opcode: u8) -> InstDesc {
        assert!(opcode < (1 << 4));
        InstDesc::Op0i(InstOpcodeDesc4 { name, bits: opcode }, InstImmDesc {})
    }
    const fn op0(name: &'static str, opcode: u8) -> InstDesc {
        InstDesc::Op0(InstOpcodeDesc8 { name, bits: opcode })
    }

    #[allow(unused)]
    pub fn parse(input: InstBinaryType) -> Option<InstBinary> {
        for inst_desc in ALL_INSTRUCTION_DESC {
            if inst_desc.match_opcode(input) {
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
    ($opcode: expr, $name: ident) => {
        paste! {
            #[allow(unused)]
            pub const [<INST_ $name:upper>]: InstDesc = InstDesc::op2(stringify!($name), $opcode);
            #[allow(unused)]
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
    ($opcode: expr, $name: ident) => {
        paste! {
            #[allow(unused)]
            pub const [<INST_ $name:upper>]: InstDesc = InstDesc::op1(stringify!($name), $opcode);
            #[allow(unused)]
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
    ($opcode: expr, $name: ident) => {
        paste! {
            #[allow(unused)]
            pub const [<INST_ $name:upper>]: InstDesc = InstDesc::op0i(stringify!($name), $opcode);
            #[allow(unused)]
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
    ($opcode: expr, $name: ident) => {
        paste! {
            #[allow(unused)]
            pub const [<INST_ $name:upper>]: InstDesc = InstDesc::op0(stringify!($name), $opcode);
            #[allow(unused)]
            pub fn [<inst_ $name>](imm: InstImmType) -> InstBinary {
                InstBinary {
                    binary: ($opcode << 0),
                    desc: &[<INST_ $name:upper>],
                }
            }
        }
    };
}

#[allow(unused)]
const ALL_INSTRUCTION_DESC: &'static [&'static InstDesc] = &[
    &INST_MOV,
    &INST_AND,
    &INST_OR,
    &INST_XOR,
    &INST_ADD,
    &INST_INV,
    &INST_NEG,
    &INST_DEC,
    &INST_INC,
    &INST_LOAD_IMM,
    &INST_LOAD_MEM,
    &INST_STORE_MEM,
    &INST_JMP_OFFSET,
    &INST_JE_OFFSET,
    &INST_JL_OFFSET,
    &INST_JG_OFFSET,
    &INST_JMP_LONG,
    // TODO control
    // TODO external
];

// binary op
inst_op2!(0b0000, mov);
inst_op2!(0b0001, and);
inst_op2!(0b0010, or);
inst_op2!(0b0011, xor);
inst_op2!(0b0100, add);
// unary op
inst_op1!(0b010100, inv);
inst_op1!(0b010101, neg);
inst_op1!(0b010110, dec);
inst_op1!(0b010111, inc);
// TODO control
inst_op0!(0b01100000, reset);
inst_op0!(0b01100001, halt);
inst_op0!(0b01100010, sleep);
inst_op0!(0b01100011, set_mem_bank);
inst_op0!(0b01100100, select_external);
// TODO external
inst_op0i!(0b0111, external);
// load store
inst_op0i!(0b1000, load_imm);
inst_op0i!(0b1001, load_mem);
inst_op0i!(0b1010, store_mem);
// jmp
inst_op0i!(0b1011, jmp_long);
inst_op0i!(0b1100, jmp_offset);
inst_op0i!(0b1101, je_offset);
inst_op0i!(0b1110, jl_offset);
inst_op0i!(0b1111, jg_offset);
