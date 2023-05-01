use crate::cpu_v1::{CpuComponent, CpuComponentEmu};
use crate::{cpu_v1, input, input_w, mux2_w, unflatten2, unflatten3, Wire, Wires};

#[derive(Debug, Clone)]
pub struct CpuDecoderInput {
    pub inst: Wires<8>,
}
#[derive(Clone)]
pub struct CpuDecoderOutput {
    // data output
    pub imm: Wires<4>,

    // reg control
    pub reg0_addr: Wires<2>, // RegAddr
    pub reg1_addr: Wires<2>, // RegAddr
    pub reg0_write_enable: Wire,
    pub reg0_write_select: Wires<3>, // Reg0WriteSelect: alu out, mem out, bus out

    // alu control
    pub alu_op: Wires<4>,      // AluOp: &, |, ^, +
    pub alu0_select: Wires<4>, // Alu0Select: reg0, ~reg0, 0, imm
    pub alu1_select: Wires<4>, // Alu1Select: reg1, -1, 0, 1

    // mem control
    pub mem_addr_select: Wires<2>, // MemAddrSelect: imm, reg1
    pub mem_write_enable: Wire,
    pub mem_bank_write_enable: Wire,

    // jmp control
    pub jmp_op: Wires<6>,         // JmpOp: no_jmp, jmp, je, jl, jg, long
    pub jmp_src_select: Wires<2>, // JmpSrcSelect: imm, regA

    // bus control
    pub bus_enable: Wire,
}

#[allow(unused)]
#[repr(u8)]
enum RegAddr {
    A = 0,
    B = 1,
    C = 2,
    D = 3,
}
#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum Reg0WriteSelect {
    AluOut = 0,
    MemOut = 1,
    BusOut = 2,
}

#[repr(u8)]
pub enum AluOp {
    And = 0,
    Or = 1,
    Xor = 2,
    Add = 3,
}
#[repr(u8)]
pub enum Alu0Select {
    Reg0 = 0,
    Reg0Inv = 1,
    Zero = 2,
    Imm = 3,
}
#[repr(u8)]
pub enum Alu1Select {
    Reg1 = 0,
    NegOne = 1,
    Zero = 2,
    One = 3,
}

#[repr(u8)]
pub enum MemAddrSelect {
    Imm = 0,
    Reg1 = 1,
}

#[repr(u8)]
pub enum JmpOp {
    NoJmp = 0,
    Jmp = 1,
    Je = 2,
    Jl = 3,
    Jg = 4,
    Long = 5,
}
#[repr(u8)]
pub enum JmpSrcSelect {
    Imm = 0,
    Reg0 = 1,
}

pub struct CpuDecoder;
impl CpuComponent for CpuDecoder {
    type Input = CpuDecoderInput;
    type Output = CpuDecoderOutput;

    fn build(i: &CpuDecoderInput) -> CpuDecoderOutput {
        let inst = i.inst;
        let (imm, op4) = unflatten2::<4, 4>(inst);
        let (inst_reg0, inst_reg1, _) = unflatten3::<2, 2, 4>(inst);

        // io table: https://shimo.im/sheets/1lq7MRQe90I86Aew/Oj96h

        let b0 = inst.wires[7];
        let b1 = inst.wires[6];
        let b2 = inst.wires[5];
        let b3 = inst.wires[4];
        let b4 = inst.wires[3];
        let b5 = inst.wires[2];

        // 0b00 | 0b010
        let is_alu = !b0 & (!b1 | (b1 & !b2));

        let is_op_bus = op4.eq_const(0b0111);
        let is_op_bus_read = is_op_bus & (imm.eq_const(0)); // opcode 0 => read, >0 => exec
        let bus_enable = is_op_bus;

        // is_alu => inst_reg0, false => regA(0)
        let reg0_addr = mux2_w(Wires::parse_u8(0), inst_reg0, is_alu);
        // is_alu => inst_reg1, false => regB(1)
        let reg1_addr = mux2_w(Wires::parse_u8(1), inst_reg1, is_alu);
        // 0b00 | 0b010 | 0b100 | 0b01110000
        let reg0_write_enable = (!b0 & !b1) | (!b0 & b1 & !b2) | (b0 & !b1 & !b2) | is_op_bus_read;
        // AluOut, MemOut, BusOut
        let mut reg0_write_select = Wires::uninitialized();

        reg0_write_select.wires[Reg0WriteSelect::AluOut as usize] =
            (!b0 | (!b1 & !b2 & !b3)) & !is_op_bus;
        reg0_write_select.wires[Reg0WriteSelect::MemOut as usize] = b0 & (b1 | b2 | b3);
        reg0_write_select.wires[Reg0WriteSelect::BusOut as usize] = is_op_bus;

        let imm_all_0 = imm.all_0();

        let mut alu_op = Wires::uninitialized();
        let is_op_and = op4.eq_const(0b0001);
        let is_op_mov = op4.eq_const(0b0000);
        let is_op_or = op4.eq_const(0b0010);
        let is_op_xor = op4.eq_const(0b0011);
        let is_op_add = op4.eq_const(0b0100);
        let is_op_unary = op4.eq_const(0b0101);
        let is_op_inv = is_op_unary & (!b4 & !b5);
        let is_op_neg = is_op_unary & (!b4 & b5);
        let is_op_dec = is_op_unary & (b4 & !b5);
        let is_op_inc = is_op_unary & (b4 & b5);
        let is_op_load_imm = op4.eq_const(0b1000);
        let is_op_store_mem = op4.eq_const(0b1010);

        let is_alu_add = b0 | b1; // all other instructions to simplify TODO new reg0 write select type to improve latency
        alu_op.wires[AluOp::And as usize] = is_op_and;
        alu_op.wires[AluOp::Or as usize] = is_op_mov | is_op_or;
        alu_op.wires[AluOp::Xor as usize] = is_op_xor;
        alu_op.wires[AluOp::Add as usize] = is_alu_add;

        let is_reg0_inv = is_op_inv | is_op_neg;
        let is_reg0 = !b0 & !is_op_mov & !is_reg0_inv;
        let mut alu0_select = Wires::uninitialized();
        alu0_select.wires[Alu0Select::Zero as usize] = is_op_mov;
        alu0_select.wires[Alu0Select::Imm as usize] = is_op_load_imm;
        alu0_select.wires[Alu0Select::Reg0 as usize] = is_reg0;
        alu0_select.wires[Alu0Select::Reg0Inv as usize] = is_reg0_inv;

        let mut alu1_select = Wires::uninitialized();
        alu1_select.wires[Alu1Select::Zero as usize] = is_op_inv | is_op_load_imm;
        alu1_select.wires[Alu1Select::One as usize] = is_op_neg | is_op_inc;
        alu1_select.wires[Alu1Select::NegOne as usize] = is_op_dec;
        alu1_select.wires[Alu1Select::Reg1 as usize] = (!b0 & !b1) | is_op_add;

        let mut mem_addr_select = Wires::uninitialized();
        mem_addr_select.wires[MemAddrSelect::Imm as usize] = !imm_all_0;
        mem_addr_select.wires[MemAddrSelect::Reg1 as usize] = imm_all_0;

        let mem_write_enable = is_op_store_mem;
        let mem_bank_write_enable = inst.eq_const(0b01100011);

        let mut jmp_op = Wires::uninitialized();
        let is_op_jmp_long = op4.eq_const(0b1011);
        let is_op_jmp_offset = op4.eq_const(0b1100);
        let is_op_je_offset = op4.eq_const(0b1101);
        let is_op_jl_offset = op4.eq_const(0b1110);
        let is_op_jg_offset = op4.eq_const(0b1111);
        jmp_op.wires[JmpOp::NoJmp as usize] = (!b0 | !b1) & !is_op_jmp_long;
        jmp_op.wires[JmpOp::Jmp as usize] = is_op_jmp_offset;
        jmp_op.wires[JmpOp::Je as usize] = is_op_je_offset;
        jmp_op.wires[JmpOp::Jl as usize] = is_op_jl_offset;
        jmp_op.wires[JmpOp::Jg as usize] = is_op_jg_offset;
        jmp_op.wires[JmpOp::Long as usize] = is_op_jmp_long;
        let mut jmp_src_select = Wires::uninitialized();
        jmp_src_select.wires[JmpSrcSelect::Imm as usize] = !imm_all_0;
        jmp_src_select.wires[JmpSrcSelect::Reg0 as usize] = imm_all_0;

        CpuDecoderOutput {
            imm,

            reg0_addr,
            reg1_addr,
            reg0_write_enable,
            reg0_write_select,

            alu_op,
            alu0_select,
            alu1_select,
            mem_addr_select,
            mem_write_enable,
            mem_bank_write_enable,
            jmp_op,
            jmp_src_select,
            bus_enable,
        }
    }
}

pub struct CpuDecoderEmu;
impl CpuComponentEmu<CpuDecoder> for CpuDecoderEmu {
    fn init_output() -> CpuDecoderOutput {
        CpuDecoderOutput {
            imm: input_w(),

            reg0_addr: input_w(),
            reg1_addr: input_w(),
            reg0_write_enable: input(),
            reg0_write_select: input_w(),

            alu_op: input_w(),
            alu0_select: input_w(),
            alu1_select: input_w(),
            mem_addr_select: input_w(),
            mem_write_enable: input(),
            mem_bank_write_enable: input(),
            jmp_op: input_w(),
            jmp_src_select: input_w(),
            bus_enable: input(),
        }
    }
    fn execute(input: &CpuDecoderInput, output: &CpuDecoderOutput) {
        use cpu_v1::isa::*;
        let inst = input.inst.get_u8();

        let reg0_bits: u8 = (inst & 0b00000011) >> 0;
        let reg1_bits: u8 = (inst & 0b00001100) >> 2;
        let imm: u8 = (inst & 0b00001111) >> 0;

        // alu_op2
        let mov = INST_MOV.match_opcode(inst);
        let and = INST_AND.match_opcode(inst);
        let or = INST_OR.match_opcode(inst);
        let xor = INST_XOR.match_opcode(inst);
        let add = INST_ADD.match_opcode(inst);
        // alu_op1
        let inv = INST_INV.match_opcode(inst);
        let neg = INST_NEG.match_opcode(inst);
        let inc = INST_INC.match_opcode(inst);
        let dec = INST_DEC.match_opcode(inst);
        // load store
        let load_imm = INST_LOAD_IMM.match_opcode(inst);
        let load_mem_imm = INST_LOAD_MEM.match_opcode(inst) && (imm != 0);
        let load_mem_reg = INST_LOAD_MEM.match_opcode(inst) && (imm == 0);
        let store_mem_imm = INST_STORE_MEM.match_opcode(inst) && (imm != 0);
        let store_mem_reg = INST_STORE_MEM.match_opcode(inst) && (imm == 0);
        // jmp
        let jmp_offset = INST_JMP_OFFSET.match_opcode(inst);
        let je_offset = INST_JE_OFFSET.match_opcode(inst);
        let jl_offset = INST_JL_OFFSET.match_opcode(inst);
        let jg_offset = INST_JG_OFFSET.match_opcode(inst);
        let jmp_long = INST_JMP_LONG.match_opcode(inst);
        // control TODO
        let set_mem_bank = INST_SET_MEM_BANK.match_opcode(inst);
        // bus
        let is_bus = INST_BUS.match_opcode(inst);
        let is_bus_read = INST_BUS_READ.match_opcode(inst);

        // immutable local variable => all output variables assigned once and only once.
        let reg0_addr: u8;
        let reg1_addr: u8;
        let reg0_write_enable: u8;
        let reg0_write_select: u8;
        let alu_op: u8;
        let alu0_select: u8;
        let alu1_select: u8;
        let mem_addr_select: u8;
        let mem_write_enable: u8;
        let mem_bank_write_enable: u8;
        let jmp_op: u8;
        let jmp_src_select: u8;
        let bus_enable: u8;

        let is_alu = mov || and || or || xor || add || inv || neg || inc || dec;
        let is_load_imm = load_imm;
        let is_load_mem = load_mem_imm || load_mem_reg;
        let is_store_mem = store_mem_imm || store_mem_reg;
        let is_jmp = jmp_offset || je_offset || jl_offset || jg_offset || jmp_long;
        let is_control = set_mem_bank; // TODO other control instructions

        if is_alu || is_load_imm {
            jmp_op = 1 << JmpOp::NoJmp as u8;
            jmp_src_select = 1 << JmpSrcSelect::Imm as u8;
            if is_load_imm {
                reg0_addr = RegAddr::A as u8;
                reg1_addr = RegAddr::B as u8; // not used
            } else {
                reg0_addr = reg0_bits;
                reg1_addr = reg1_bits;
            }
            reg0_write_enable = 1;
            reg0_write_select = 1 << Reg0WriteSelect::AluOut as u8;
            mem_addr_select = 0;
            mem_write_enable = 0;
            mem_bank_write_enable = 0;
            bus_enable = 0;

            let mut v_alu_op: u8 = 0;
            let mut v_alu0_select: u8 = 0;
            let mut v_alu1_select: u8 = 0;
            let mut set_alu = |op_match: bool, op: AluOp, alu0: Alu0Select, alu1: Alu1Select| {
                if op_match {
                    v_alu_op = 1 << op as u8;
                    v_alu0_select = 1 << alu0 as u8;
                    v_alu1_select = 1 << alu1 as u8;
                }
            };

            set_alu(mov, AluOp::Or, Alu0Select::Zero, Alu1Select::Reg1);
            set_alu(and, AluOp::And, Alu0Select::Reg0, Alu1Select::Reg1);
            set_alu(or, AluOp::Or, Alu0Select::Reg0, Alu1Select::Reg1);
            set_alu(xor, AluOp::Xor, Alu0Select::Reg0, Alu1Select::Reg1);
            set_alu(add, AluOp::Add, Alu0Select::Reg0, Alu1Select::Reg1);

            set_alu(inv, AluOp::Add, Alu0Select::Reg0Inv, Alu1Select::Zero);
            set_alu(neg, AluOp::Add, Alu0Select::Reg0Inv, Alu1Select::One);
            set_alu(inc, AluOp::Add, Alu0Select::Reg0, Alu1Select::One);
            set_alu(dec, AluOp::Add, Alu0Select::Reg0, Alu1Select::NegOne);
            set_alu(load_imm, AluOp::Add, Alu0Select::Imm, Alu1Select::Zero);

            alu_op = v_alu_op;
            alu0_select = v_alu0_select;
            alu1_select = v_alu1_select;
        } else if is_load_mem || is_store_mem {
            jmp_op = 1 << JmpOp::NoJmp as u8;
            jmp_src_select = 1 << JmpSrcSelect::Imm as u8;
            reg0_addr = RegAddr::A as u8;
            reg1_addr = RegAddr::B as u8;
            alu_op = 0;
            alu0_select = 0;
            alu1_select = 0;
            mem_bank_write_enable = 0;
            bus_enable = 0;

            if is_load_mem {
                reg0_write_enable = 1;
                reg0_write_select = 1 << Reg0WriteSelect::MemOut as u8;
                mem_write_enable = 0;
                if load_mem_imm {
                    mem_addr_select = 1 << MemAddrSelect::Imm as u8;
                } else {
                    mem_addr_select = 1 << MemAddrSelect::Reg1 as u8;
                }
            } else if is_store_mem {
                reg0_write_enable = 0;
                reg0_write_select = 0;
                mem_write_enable = 1;
                if store_mem_imm {
                    mem_addr_select = 1 << MemAddrSelect::Imm as u8;
                } else {
                    mem_addr_select = 1 << MemAddrSelect::Reg1 as u8;
                }
            } else {
                unreachable!()
            }
        } else if is_jmp {
            jmp_op = ((jmp_offset as u8) << (JmpOp::Jmp as u8))
                | ((je_offset as u8) << (JmpOp::Je as u8))
                | ((jl_offset as u8) << (JmpOp::Jl as u8))
                | ((jg_offset as u8) << (JmpOp::Jg as u8))
                | ((jmp_long as u8) << (JmpOp::Long as u8));
            reg0_addr = 0;
            reg1_addr = 0;
            reg0_write_enable = 0;
            reg0_write_select = 0;
            alu_op = 0;
            alu0_select = 0;
            alu1_select = 0;
            mem_addr_select = 0;
            mem_write_enable = 0;
            mem_bank_write_enable = 0;
            bus_enable = 0;

            jmp_src_select = if imm == 0 {
                1 << JmpSrcSelect::Reg0 as u8
            } else {
                1 << JmpSrcSelect::Imm as u8
            };
        } else if is_control {
            //TODO other control instructions

            // set_mem_bank
            reg0_addr = 0;
            reg1_addr = 0;
            reg0_write_enable = 0;
            reg0_write_select = 0;
            alu_op = 0;
            alu0_select = 0;
            alu1_select = 0;
            mem_addr_select = 0;
            mem_write_enable = 0;
            mem_bank_write_enable = 1;
            jmp_op = 1 << JmpOp::NoJmp as u8;
            jmp_src_select = 1 << JmpSrcSelect::Imm as u8;
            bus_enable = 0;
        } else if is_bus {
            reg0_addr = 0;
            reg1_addr = 1;
            reg0_write_enable = is_bus_read as u8;
            reg0_write_select = 1 << Reg0WriteSelect::BusOut as u8;
            alu_op = 0;
            alu0_select = 0;
            alu1_select = 0;
            mem_addr_select = 0;
            mem_write_enable = 0;
            mem_bank_write_enable = 0;
            jmp_op = 1 << JmpOp::NoJmp as u8;
            jmp_src_select = 1 << JmpSrcSelect::Imm as u8;
            bus_enable = 1;
        } else {
            unreachable!("unknown instruction")
        }

        output.imm.set_u8(imm);
        output.reg0_addr.set_u8(reg0_addr);
        output.reg1_addr.set_u8(reg1_addr);
        output.reg0_write_enable.set(reg0_write_enable);
        output.reg0_write_select.set_u8(reg0_write_select);
        output.alu_op.set_u8(alu_op);
        output.alu0_select.set_u8(alu0_select);
        output.alu1_select.set_u8(alu1_select);
        output.mem_addr_select.set_u8(mem_addr_select);
        output.mem_write_enable.set(mem_write_enable);
        output.mem_bank_write_enable.set(mem_bank_write_enable);
        output.jmp_op.set_u8(jmp_op);
        output.jmp_src_select.set_u8(jmp_src_select);
        output.bus_enable.set(bus_enable);
    }
}

#[cfg(test)]
use crate::cpu_v1::isa::*;
#[cfg(test)]
use std::fmt::Debug;

#[cfg(test)]
struct DecoderTestEnv {
    inst: Wires<8>,
    build1: CpuDecoderOutput,
    build2: CpuDecoderOutput,
}

#[cfg(test)]
fn test_result<T: PartialEq + Eq + Debug>(
    inst: InstBinary,
    env: &DecoderTestEnv,
    fields: impl Fn(&CpuDecoderOutput) -> T,
) {
    env.inst.set_u8(inst.binary);
    crate::simulate();
    let r1 = fields(&env.build1);
    let r2 = fields(&env.build2);
    assert_eq!(r1, r2, "{:08b} {}", inst.binary, inst.desc.name());
    println!("{:08b} {}, {:?}", inst.binary, inst.desc.name(), r1);
}

#[cfg(test)]
fn init_decoder() -> DecoderTestEnv {
    crate::clear_all();

    let input = CpuDecoderInput { inst: input_w() };

    DecoderTestEnv {
        inst: input.inst,
        build1: CpuDecoder::build(&input),
        build2: CpuDecoderEmu::build(&input),
    }
}

#[cfg(test)]
fn test_decoder_alu(inst: InstBinary, env: &DecoderTestEnv) {
    test_result(inst, &env, |o| {
        (
            o.reg0_addr.get_u8(),
            o.reg1_addr.get_u8(),
            o.reg0_write_enable.get(),
            o.reg0_write_select.get_u8(),
            o.alu_op.get_u8(),
            o.alu0_select.get_u8(),
            o.alu1_select.get_u8(),
            o.mem_write_enable.get(),
            o.jmp_op.get_u8(),
        )
    });
}
#[cfg(test)]
fn test_decoder_jmp(inst: InstBinary, env: &DecoderTestEnv) {
    test_result(inst, &env, |o| {
        (
            o.reg0_addr.get_u8(),
            o.mem_write_enable.get(),
            o.jmp_op.get_u8(),
            o.jmp_src_select.get_u8(),
        )
    });
}
#[cfg(test)]
fn test_decoder_load_mem(inst: InstBinary, env: &DecoderTestEnv) {
    test_result(inst, &env, |o| {
        (
            o.reg0_addr.get_u8(),
            o.reg1_addr.get_u8(),
            o.reg0_write_enable.get(),
            o.reg0_write_select.get_u8(),
            o.mem_addr_select.get_u8(),
            o.mem_write_enable.get(),
            o.jmp_op.get_u8(),
        )
    });
}
#[cfg(test)]
fn test_decoder_store_mem(inst: InstBinary, env: &DecoderTestEnv) {
    test_result(inst, &env, |o| {
        (
            o.reg0_addr.get_u8(),
            o.reg1_addr.get_u8(),
            o.reg0_write_enable.get(),
            o.mem_addr_select.get_u8(),
            o.mem_write_enable.get(),
            o.jmp_op.get_u8(),
        )
    });
}
#[cfg(test)]
fn test_decoder_special(inst: InstBinary, env: &DecoderTestEnv) {
    test_result(inst, &env, |o| {
        (
            o.reg0_addr.get_u8(),
            // o.reg1_addr.get_u8(),
            o.reg0_write_enable.get(),
            // o.mem_addr_select.get_u8(),
            // o.mem_write_enable.get(),
            o.mem_bank_write_enable.get(),
            o.jmp_op.get_u8(),
            o.bus_enable.get(),
        )
    });
}

#[test]
fn test_decoder() {
    let env = init_decoder();

    test_decoder_alu(inst_mov(0, 0), &env);
    test_decoder_alu(inst_and(1, 2), &env);
    test_decoder_alu(inst_or(3, 0), &env);
    test_decoder_alu(inst_xor(2, 1), &env);
    test_decoder_alu(inst_add(3, 0), &env);
    test_decoder_alu(inst_inv(0), &env);
    test_decoder_alu(inst_neg(1), &env);
    test_decoder_alu(inst_dec(2), &env);
    test_decoder_alu(inst_inc(3), &env);
    test_decoder_alu(inst_load_imm(9), &env);

    test_decoder_load_mem(inst_load_mem(15), &env);
    test_decoder_load_mem(inst_load_mem(0), &env);

    test_decoder_store_mem(inst_store_mem(15), &env);
    test_decoder_store_mem(inst_store_mem(0), &env);

    test_decoder_jmp(inst_jmp_long(15), &env);
    test_decoder_jmp(inst_jmp_long(0), &env);
    test_decoder_jmp(inst_jmp_offset(14), &env);
    test_decoder_jmp(inst_jmp_offset(0), &env);
    test_decoder_jmp(inst_je_offset(13), &env);
    test_decoder_jmp(inst_je_offset(0), &env);
    test_decoder_jmp(inst_jl_offset(12), &env);
    test_decoder_jmp(inst_jl_offset(0), &env);
    test_decoder_jmp(inst_jg_offset(11), &env);
    test_decoder_jmp(inst_jg_offset(0), &env);

    //TODO control
    test_decoder_special(inst_set_mem_bank(), &env);

    test_decoder_special(inst_bus(0), &env);
    test_decoder_special(inst_bus(1), &env);
}
