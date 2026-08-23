use super::{
    set_wire, ExecOp, PcSrc, WbSrc, EXEC_OP_WIDTH, FLAGS_WIDTH, PC_SRC_WIDTH, REG_INDEX_WIDTH,
    WB_SRC_WIDTH, WORD_WIDTH,
};
use crate::isa::{
    hilo_as_u16, imm4_nz, imm8_as_i16, imm_as_i16, Cond, Instruction, RA_REG, SP_REG,
};
use digital_design_circuit::{
    add_naive, flatten2, flatten3, input as input_wire, input_const, input_w, input_w_const,
    unflatten2, unflatten3, CircuitComponent, CircuitComponentEmu, CircuitWires, Wire, Wires,
    WiresU16, WiresU8,
};

#[derive(Clone)]
pub struct DecoderInput {
    pub instruction: Wires<WORD_WIDTH>,
    pub reset: Wire,
}

#[derive(Clone)]
pub struct DecoderOutput {
    pub source_a: Wires<REG_INDEX_WIDTH>,
    pub source_b: Wires<REG_INDEX_WIDTH>,
    pub destination: Wires<REG_INDEX_WIDTH>,
    pub immediate: Wires<WORD_WIDTH>,
    pub execute_operation: Wires<EXEC_OP_WIDTH>,

    pub register_write_enable: Wire,
    pub writeback_source: Wires<WB_SRC_WIDTH>,
    pub flags_write_enable: Wire,

    pub memory_read_enable: Wire,
    pub memory_write_enable: Wire,

    pub pc_source: Wires<PC_SRC_WIDTH>,
    pub condition_mask: Wires<FLAGS_WIDTH>,

    pub device_index: Wires<4>,
    pub device_channel: Wires<4>,
    pub device_read_enable: Wire,
    pub device_write_enable: Wire,

    pub halt_enable: Wire,
}

pub struct CpuDecoder;

fn any(values: &[Wire]) -> Wire {
    values
        .iter()
        .copied()
        .fold(input_const(0), |output, value| output | value)
}

fn constant_w<const W: usize>(value: u16) -> Wires<W> {
    Wires {
        wires: std::array::from_fn(|bit| input_const(((value >> bit) & 1) as u8)),
    }
}

fn combine<const W: usize>(cases: &[(Wire, Wires<W>)]) -> Wires<W> {
    cases
        .iter()
        .fold(input_w_const(0), |output, (selected, value)| {
            output | selected.expand() & *value
        })
}

fn combine_constants<const W: usize>(cases: &[(Wire, u16)]) -> Wires<W> {
    Wires {
        wires: std::array::from_fn(|bit| {
            any(&cases
                .iter()
                .filter(|(_, value)| value & (1 << bit) != 0)
                .map(|(selected, _)| *selected)
                .collect::<Vec<_>>())
        }),
    }
}

impl CircuitComponent for CpuDecoder {
    type Input = DecoderInput;
    type Output = DecoderOutput;

    fn build(input: &Self::Input) -> Self::Output {
        let (n0, n1, high) = unflatten3::<4, 4, 8>(input.instruction);
        let (n2, n3) = unflatten2::<4, 4>(high);
        let enabled = !input.reset;
        let op8 = |opcode: u8| enabled & n3.eq_const(opcode >> 4) & n2.eq_const(opcode & 0x0f);
        let op4 = |opcode: u8| enabled & n3.eq_const(opcode);

        let halt = op8(0x00);
        let mov = op8(0x01);
        let inv = op8(0x02);
        let neg = op8(0x03);
        let not0 = op8(0x04);
        let cnt1 = op8(0x05);
        let log2 = op8(0x06);
        let lsl = op8(0x08);
        let lsr = op8(0x09);
        let asr = op8(0x0a);
        let sp_add = op8(0x0c);
        let sp_sub = op8(0x0d);

        let and = op4(0x8);
        let or = op4(0x9);
        let xor = op4(0xa);
        let add = op4(0xb);
        let sub = op4(0xc);
        let addi = op4(0xd);
        let load_hi = op4(0x2);
        let load_lo = op4(0x3);
        let store_mem = op4(0x4);
        let load_mem = op4(0x5);
        let store_sp = op4(0x6);
        let load_sp = op4(0x7);

        let pc = op8(0x10);
        let jg = op8(0x11);
        let je = op8(0x12);
        let jge = op8(0x13);
        let jl = op8(0x14);
        let jne = op8(0x15);
        let jle = op8(0x16);
        let jmp = op8(0x17);
        let cmp_r = op8(0x18);
        let cmp_i = op8(0x19);
        let cmp_s = op8(0x1a);
        let cmp_si = op8(0x1b);
        let call_rel = op8(0x1c);
        let call_abs = op8(0x1d);
        let jmp_reg = op8(0x1e);
        let call_reg = op8(0x1f);
        let dev_recv = op4(0xe);
        let dev_send = op4(0xf);

        let unary = any(&[halt, mov, inv, neg, not0, cnt1, log2]);
        let shift = any(&[lsl, lsr, asr]);
        let binary = any(&[and, or, xor, add, sub]);
        let stack_adjust = sp_add | sp_sub;
        let stack_memory = store_sp | load_sp;
        let compare_register = cmp_r | cmp_s;
        let compare_immediate = cmp_i | cmp_si;
        let relative_jump = any(&[jg, je, jge, jl, jne, jle, jmp]);
        let register_jump = jmp_reg | call_reg;

        let source_a = combine(&[
            (unary, n1),
            (shift, n0),
            (stack_adjust | stack_memory, constant_w(SP_REG as u16)),
            (binary | addi | store_mem | load_mem, n2),
            (load_hi, n0),
            (compare_register | compare_immediate, n0),
            (register_jump, n1),
            (dev_send, n0),
        ]);
        let source_b = combine(&[(binary | store_mem | compare_register, n1), (store_sp, n0)]);
        let destination = combine(&[
            (
                any(&[
                    mov, inv, neg, not0, cnt1, log2, lsl, lsr, asr, and, or, xor, add, sub, addi,
                    load_hi, load_lo, load_mem, load_sp, pc, dev_recv,
                ]),
                n0,
            ),
            (stack_adjust, constant_w(SP_REG as u16)),
            (call_rel | call_abs | call_reg, constant_w(RA_REG as u16)),
        ]);

        let shift_immediate = n1.expand_unsigned::<WORD_WIDTH>();
        let stack_adjust_immediate = flatten2(n0, n1).expand_unsigned::<WORD_WIDTH>();
        let stack_sub_immediate =
            add_naive(!stack_adjust_immediate, constant_w::<WORD_WIDTH>(1)).sum;
        let stack_memory_immediate = flatten2(n1, n2).expand_unsigned::<WORD_WIDTH>();
        let addi_base = n1.expand_signed::<WORD_WIDTH>();
        let addi_increment = (!n1.wires[3]).expand::<WORD_WIDTH>() & constant_w::<WORD_WIDTH>(1);
        let addi_immediate = add_naive(addi_base, addi_increment).sum;
        let load_hi_immediate = flatten3(input_w_const::<8>(0), n1, n2);
        let load_lo_immediate = flatten2(n1, n2).expand_unsigned::<WORD_WIDTH>();
        let memory_offset = n0.expand_signed::<WORD_WIDTH>();
        let load_memory_offset = n1.expand_signed::<WORD_WIDTH>();
        let pc_immediate = n1.expand_signed::<WORD_WIDTH>();
        let jump_immediate = flatten2(n0, n1).expand_signed::<WORD_WIDTH>();
        let compare_unsigned_immediate = n1.expand_unsigned::<WORD_WIDTH>();
        let compare_signed_immediate = n1.expand_signed::<WORD_WIDTH>();
        let call_abs_immediate = flatten2(flatten2(n0, n1), constant_w::<8>(0xff));

        let immediate = combine(&[
            (shift, shift_immediate),
            (sp_add, stack_adjust_immediate),
            (sp_sub, stack_sub_immediate),
            (addi, addi_immediate),
            (load_hi, load_hi_immediate),
            (load_lo, load_lo_immediate),
            (store_mem, memory_offset),
            (load_mem, load_memory_offset),
            (stack_memory, stack_memory_immediate),
            (pc, pc_immediate),
            (relative_jump | call_rel, jump_immediate),
            (cmp_i, compare_unsigned_immediate),
            (cmp_si, compare_signed_immediate),
            (call_abs, call_abs_immediate),
        ]);

        let execute_operation = combine_constants(&[
            (mov | jmp_reg, ExecOp::PassA as u16),
            (inv, ExecOp::Inv as u16),
            (neg, ExecOp::Neg as u16),
            (not0, ExecOp::NotZero as u16),
            (cnt1, ExecOp::CountOnes as u16),
            (log2, ExecOp::Log2 as u16),
            (lsl, ExecOp::Lsl as u16),
            (lsr, ExecOp::Lsr as u16),
            (asr, ExecOp::Asr as u16),
            (and, ExecOp::And as u16),
            (or, ExecOp::Or as u16),
            (xor, ExecOp::Xor as u16),
            (add, ExecOp::Add as u16),
            (sub, ExecOp::Sub as u16),
            (
                addi | stack_adjust | store_mem | load_mem | stack_memory,
                ExecOp::AddImmediate as u16,
            ),
            (load_hi, ExecOp::LoadHi as u16),
            (load_lo, ExecOp::LoadLo as u16),
            (pc | relative_jump, ExecOp::PcAdd as u16),
            (cmp_r, ExecOp::CompareUnsigned as u16),
            (cmp_i, ExecOp::CompareUnsignedImmediate as u16),
            (cmp_s, ExecOp::CompareSigned as u16),
            (cmp_si, ExecOp::CompareSignedImmediate as u16),
            (call_rel, ExecOp::CallRelative as u16),
            (call_abs, ExecOp::CallAbsolute as u16),
            (call_reg, ExecOp::CallRegister as u16),
        ]);

        let register_write_enable = any(&[
            mov, inv, neg, not0, cnt1, log2, lsl, lsr, asr, sp_add, sp_sub, and, or, xor, add, sub,
            addi, load_hi, load_lo, load_mem, load_sp, pc, call_rel, call_abs, call_reg, dev_recv,
        ]);
        let writeback_source = combine_constants(&[
            (load_mem | load_sp, WbSrc::Memory as u16),
            (dev_recv, WbSrc::Device as u16),
        ]);
        let flags_write_enable = compare_register | compare_immediate;
        let memory_read_enable = load_mem | load_sp | call_abs;
        let memory_write_enable = store_mem | store_sp;
        let pc_source = combine_constants(&[
            (
                relative_jump | call_rel | register_jump,
                PcSrc::Execute as u16,
            ),
            (call_abs, PcSrc::Memory as u16),
        ]);
        let condition_mask = combine_constants(&[
            (jg, Cond::Greater as u16),
            (je, Cond::Equal as u16),
            (jge, Cond::GreaterEqual as u16),
            (jl, Cond::Less as u16),
            (jne, Cond::NotEqual as u16),
            (jle, Cond::LessEqual as u16),
            (
                jmp | call_rel | call_abs | register_jump,
                Cond::Always as u16,
            ),
        ]);

        DecoderOutput {
            source_a,
            source_b,
            destination,
            immediate,
            execute_operation,
            register_write_enable,
            writeback_source,
            flags_write_enable,
            memory_read_enable,
            memory_write_enable,
            pc_source,
            condition_mask,
            device_index: combine(&[(dev_recv | dev_send, n2)]),
            device_channel: combine(&[(dev_recv | dev_send, n1)]),
            device_read_enable: dev_recv,
            device_write_enable: dev_send,
            halt_enable: halt,
        }
    }
}

#[derive(Clone)]
struct DecodedControl {
    source_a: u8,
    source_b: u8,
    destination: u8,
    immediate: u16,
    execute_operation: ExecOp,
    register_write_enable: bool,
    writeback_source: WbSrc,
    flags_write_enable: bool,
    memory_read_enable: bool,
    memory_write_enable: bool,
    pc_source: PcSrc,
    condition_mask: u8,
    device_index: u8,
    device_channel: u8,
    device_read_enable: bool,
    device_write_enable: bool,
    halt_enable: bool,
}

impl Default for DecodedControl {
    fn default() -> Self {
        Self {
            source_a: 0,
            source_b: 0,
            destination: 0,
            immediate: 0,
            execute_operation: ExecOp::Idle,
            register_write_enable: false,
            writeback_source: WbSrc::Execute,
            flags_write_enable: false,
            memory_read_enable: false,
            memory_write_enable: false,
            pc_source: PcSrc::Next,
            condition_mask: 0,
            device_index: 0,
            device_channel: 0,
            device_read_enable: false,
            device_write_enable: false,
            halt_enable: false,
        }
    }
}

impl DecodedControl {
    fn register(source_a: u8, source_b: u8, destination: u8, operation: ExecOp) -> Self {
        Self {
            source_a,
            source_b,
            destination,
            execute_operation: operation,
            register_write_enable: true,
            ..Self::default()
        }
    }

    fn register_immediate(
        source_a: u8,
        destination: u8,
        immediate: u16,
        operation: ExecOp,
    ) -> Self {
        Self {
            immediate,
            ..Self::register(source_a, 0, destination, operation)
        }
    }

    fn relative_jump(condition: Cond, hi: u8, lo: u8) -> Self {
        Self {
            immediate: imm8_as_i16(hi, lo),
            execute_operation: ExecOp::PcAdd,
            pc_source: PcSrc::Execute,
            condition_mask: condition as u8,
            ..Self::default()
        }
    }
}

fn decode(instruction: Instruction) -> DecodedControl {
    use Instruction::*;

    match instruction {
        halt(source) => DecodedControl {
            source_a: source,
            halt_enable: true,
            ..DecodedControl::default()
        },
        mov(source, destination) => DecodedControl::register(source, 0, destination, ExecOp::PassA),
        inv(source, destination) => DecodedControl::register(source, 0, destination, ExecOp::Inv),
        neg(source, destination) => DecodedControl::register(source, 0, destination, ExecOp::Neg),
        not0(source, destination) => {
            DecodedControl::register(source, 0, destination, ExecOp::NotZero)
        }
        cnt1(source, destination) => {
            DecodedControl::register(source, 0, destination, ExecOp::CountOnes)
        }
        log2(source, destination) => DecodedControl::register(source, 0, destination, ExecOp::Log2),
        lsl(immediate, destination) => DecodedControl::register_immediate(
            destination,
            destination,
            immediate as u16,
            ExecOp::Lsl,
        ),
        lsr(immediate, destination) => DecodedControl::register_immediate(
            destination,
            destination,
            immediate as u16,
            ExecOp::Lsr,
        ),
        asr(immediate, destination) => DecodedControl::register_immediate(
            destination,
            destination,
            immediate as u16,
            ExecOp::Asr,
        ),
        sp_add(hi, lo) => DecodedControl::register_immediate(
            SP_REG,
            SP_REG,
            hilo_as_u16(hi, lo),
            ExecOp::AddImmediate,
        ),
        sp_sub(hi, lo) => DecodedControl::register_immediate(
            SP_REG,
            SP_REG,
            0u16.wrapping_sub(hilo_as_u16(hi, lo)),
            ExecOp::AddImmediate,
        ),
        and(source_a, source_b, destination) => {
            DecodedControl::register(source_a, source_b, destination, ExecOp::And)
        }
        or(source_a, source_b, destination) => {
            DecodedControl::register(source_a, source_b, destination, ExecOp::Or)
        }
        xor(source_a, source_b, destination) => {
            DecodedControl::register(source_a, source_b, destination, ExecOp::Xor)
        }
        add(source_a, source_b, destination) => {
            DecodedControl::register(source_a, source_b, destination, ExecOp::Add)
        }
        sub(source_a, source_b, destination) => {
            DecodedControl::register(source_a, source_b, destination, ExecOp::Sub)
        }
        addi(source, immediate, destination) => DecodedControl::register_immediate(
            source,
            destination,
            imm4_nz(immediate) as u16,
            ExecOp::AddImmediate,
        ),
        load_hi(hi, lo, destination) => DecodedControl::register_immediate(
            destination,
            destination,
            ((hi as u16) << 12) | ((lo as u16) << 8),
            ExecOp::LoadHi,
        ),
        load_lo(hi, lo, destination) => {
            DecodedControl::register_immediate(0, destination, hilo_as_u16(hi, lo), ExecOp::LoadLo)
        }
        store_mem(base, source, offset) => DecodedControl {
            source_a: base,
            source_b: source,
            immediate: imm_as_i16(offset),
            execute_operation: ExecOp::AddImmediate,
            memory_write_enable: true,
            ..DecodedControl::default()
        },
        load_mem(base, offset, destination) => DecodedControl {
            source_a: base,
            destination,
            immediate: imm_as_i16(offset),
            execute_operation: ExecOp::AddImmediate,
            register_write_enable: true,
            writeback_source: WbSrc::Memory,
            memory_read_enable: true,
            ..DecodedControl::default()
        },
        store_sp(hi, lo, source) => DecodedControl {
            source_a: SP_REG,
            source_b: source,
            immediate: hilo_as_u16(hi, lo),
            execute_operation: ExecOp::AddImmediate,
            memory_write_enable: true,
            ..DecodedControl::default()
        },
        load_sp(hi, lo, destination) => DecodedControl {
            source_a: SP_REG,
            destination,
            immediate: hilo_as_u16(hi, lo),
            execute_operation: ExecOp::AddImmediate,
            register_write_enable: true,
            writeback_source: WbSrc::Memory,
            memory_read_enable: true,
            ..DecodedControl::default()
        },
        pc(immediate, destination) => {
            DecodedControl::register_immediate(0, destination, imm_as_i16(immediate), ExecOp::PcAdd)
        }
        jg(hi, lo) => DecodedControl::relative_jump(Cond::Greater, hi, lo),
        je(hi, lo) => DecodedControl::relative_jump(Cond::Equal, hi, lo),
        jge(hi, lo) => DecodedControl::relative_jump(Cond::GreaterEqual, hi, lo),
        jl(hi, lo) => DecodedControl::relative_jump(Cond::Less, hi, lo),
        jne(hi, lo) => DecodedControl::relative_jump(Cond::NotEqual, hi, lo),
        jle(hi, lo) => DecodedControl::relative_jump(Cond::LessEqual, hi, lo),
        jmp(hi, lo) => DecodedControl::relative_jump(Cond::Always, hi, lo),
        cmp_r(source_b, source_a) => DecodedControl {
            source_a,
            source_b,
            execute_operation: ExecOp::CompareUnsigned,
            flags_write_enable: true,
            ..DecodedControl::default()
        },
        cmp_i(immediate, source_a) => DecodedControl {
            source_a,
            immediate: immediate as u16,
            execute_operation: ExecOp::CompareUnsignedImmediate,
            flags_write_enable: true,
            ..DecodedControl::default()
        },
        cmp_s(source_b, source_a) => DecodedControl {
            source_a,
            source_b,
            execute_operation: ExecOp::CompareSigned,
            flags_write_enable: true,
            ..DecodedControl::default()
        },
        cmp_si(immediate, source_a) => DecodedControl {
            source_a,
            immediate: imm_as_i16(immediate),
            execute_operation: ExecOp::CompareSignedImmediate,
            flags_write_enable: true,
            ..DecodedControl::default()
        },
        call_rel(hi, lo) => DecodedControl {
            destination: RA_REG,
            immediate: imm8_as_i16(hi, lo),
            execute_operation: ExecOp::CallRelative,
            register_write_enable: true,
            pc_source: PcSrc::Execute,
            condition_mask: Cond::Always as u8,
            ..DecodedControl::default()
        },
        call_abs(hi, lo) => DecodedControl {
            destination: RA_REG,
            immediate: 0xff00u16.wrapping_add(hilo_as_u16(hi, lo)),
            execute_operation: ExecOp::CallAbsolute,
            register_write_enable: true,
            memory_read_enable: true,
            pc_source: PcSrc::Memory,
            condition_mask: Cond::Always as u8,
            ..DecodedControl::default()
        },
        jmp_reg(source) => DecodedControl {
            source_a: source,
            execute_operation: ExecOp::PassA,
            pc_source: PcSrc::Execute,
            condition_mask: Cond::Always as u8,
            ..DecodedControl::default()
        },
        call_reg(source) => DecodedControl {
            source_a: source,
            destination: RA_REG,
            execute_operation: ExecOp::CallRegister,
            register_write_enable: true,
            pc_source: PcSrc::Execute,
            condition_mask: Cond::Always as u8,
            ..DecodedControl::default()
        },
        dev_recv(index, channel, destination) => DecodedControl {
            destination,
            register_write_enable: true,
            writeback_source: WbSrc::Device,
            device_index: index,
            device_channel: channel,
            device_read_enable: true,
            ..DecodedControl::default()
        },
        dev_send(index, channel, source) => DecodedControl {
            source_a: source,
            device_index: index,
            device_channel: channel,
            device_write_enable: true,
            ..DecodedControl::default()
        },
    }
}

pub struct CpuDecoderEmu;

impl CircuitComponentEmu<CpuDecoder> for CpuDecoderEmu {
    fn create(input: &DecoderInput) -> (Self, DecoderOutput) {
        let output = DecoderOutput {
            source_a: input_w(),
            source_b: input_w(),
            destination: input_w(),
            immediate: input_w(),
            execute_operation: input_w(),
            register_write_enable: input_wire(),
            writeback_source: input_w(),
            flags_write_enable: input_wire(),
            memory_read_enable: input_wire(),
            memory_write_enable: input_wire(),
            pc_source: input_w(),
            condition_mask: input_w(),
            device_index: input_w(),
            device_channel: input_w(),
            device_read_enable: input_wire(),
            device_write_enable: input_wire(),
            halt_enable: input_wire(),
        };

        let latency = input
            .instruction
            .get_max_latency_external()
            .max(input.reset.get_latency_external())
            + 1;
        output.source_a.set_latency_external(latency);
        output.source_b.set_latency_external(latency);
        output.destination.set_latency_external(latency);
        output.immediate.set_latency_external(latency);
        output.execute_operation.set_latency_external(latency);
        output.register_write_enable.set_latency_external(latency);
        output.writeback_source.set_latency_external(latency);
        output.flags_write_enable.set_latency_external(latency);
        output.memory_read_enable.set_latency_external(latency);
        output.memory_write_enable.set_latency_external(latency);
        output.pc_source.set_latency_external(latency);
        output.condition_mask.set_latency_external(latency);
        output.device_index.set_latency_external(latency);
        output.device_channel.set_latency_external(latency);
        output.device_read_enable.set_latency_external(latency);
        output.device_write_enable.set_latency_external(latency);
        output.halt_enable.set_latency_external(latency);

        (Self, output)
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &DecoderInput,
        output: &DecoderOutput,
    ) {
        let control = if input.reset.is_one(circuit) {
            DecodedControl::default()
        } else {
            decode(Instruction::parse(input.instruction.get_u16(circuit)))
        };

        output.source_a.set_u8(circuit, control.source_a);
        output.source_b.set_u8(circuit, control.source_b);
        output.destination.set_u8(circuit, control.destination);
        output.immediate.set_u16(circuit, control.immediate);
        output
            .execute_operation
            .set_u8(circuit, control.execute_operation as u8);
        set_wire(
            circuit,
            output.register_write_enable,
            control.register_write_enable,
        );
        output
            .writeback_source
            .set_u8(circuit, control.writeback_source as u8);
        set_wire(
            circuit,
            output.flags_write_enable,
            control.flags_write_enable,
        );
        set_wire(
            circuit,
            output.memory_read_enable,
            control.memory_read_enable,
        );
        set_wire(
            circuit,
            output.memory_write_enable,
            control.memory_write_enable,
        );
        output.pc_source.set_u8(circuit, control.pc_source as u8);
        output
            .condition_mask
            .set_u8(circuit, control.condition_mask);
        output.device_index.set_u8(circuit, control.device_index);
        output
            .device_channel
            .set_u8(circuit, control.device_channel);
        set_wire(
            circuit,
            output.device_read_enable,
            control.device_read_enable,
        );
        set_wire(
            circuit,
            output.device_write_enable,
            control.device_write_enable,
        );
        set_wire(circuit, output.halt_enable, control.halt_enable);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_circuit::{build_circuit, input, input_w, Circuit, CircuitWires};

    #[derive(Debug, Eq, PartialEq)]
    struct DecoderSnapshot {
        source_a: u8,
        source_b: u8,
        destination: u8,
        immediate: u16,
        execute_operation: u8,
        register_write_enable: bool,
        writeback_source: u8,
        flags_write_enable: bool,
        memory_read_enable: bool,
        memory_write_enable: bool,
        pc_source: u8,
        condition_mask: u8,
        device_index: u8,
        device_channel: u8,
        device_read_enable: bool,
        device_write_enable: bool,
        halt_enable: bool,
    }

    fn snapshot(circuit: &CircuitWires, output: &DecoderOutput) -> DecoderSnapshot {
        DecoderSnapshot {
            source_a: output.source_a.get_u8(circuit),
            source_b: output.source_b.get_u8(circuit),
            destination: output.destination.get_u8(circuit),
            immediate: output.immediate.get_u16(circuit),
            execute_operation: output.execute_operation.get_u8(circuit),
            register_write_enable: output.register_write_enable.is_one(circuit),
            writeback_source: output.writeback_source.get_u8(circuit),
            flags_write_enable: output.flags_write_enable.is_one(circuit),
            memory_read_enable: output.memory_read_enable.is_one(circuit),
            memory_write_enable: output.memory_write_enable.is_one(circuit),
            pc_source: output.pc_source.get_u8(circuit),
            condition_mask: output.condition_mask.get_u8(circuit),
            device_index: output.device_index.get_u8(circuit),
            device_channel: output.device_channel.get_u8(circuit),
            device_read_enable: output.device_read_enable.is_one(circuit),
            device_write_enable: output.device_write_enable.is_one(circuit),
            halt_enable: output.halt_enable.is_one(circuit),
        }
    }

    struct DecoderTestEnv {
        circuit: Circuit,
        instruction: Wires<WORD_WIDTH>,
        reset: Wire,
        gates: DecoderOutput,
        emu: DecoderOutput,
    }

    fn create_env() -> DecoderTestEnv {
        let (circuit, (instruction, reset, gates, emu)) = build_circuit(|| {
            let instruction = input_w();
            let reset = input();
            let input = DecoderInput { instruction, reset };
            let gates = CpuDecoder::build(&input);
            let emu = CpuDecoderEmu::build(&input);
            (instruction, reset, gates, emu)
        });
        DecoderTestEnv {
            circuit,
            instruction,
            reset,
            gates,
            emu,
        }
    }

    fn assert_instructions_match(env: &mut DecoderTestEnv, instructions: &[Instruction]) {
        for instruction in instructions {
            env.instruction
                .set_u16(&mut env.circuit, instruction.encode());
            env.circuit.simulate();
            assert_eq!(
                snapshot(&env.circuit, &env.gates),
                snapshot(&env.circuit, &env.emu),
                "{instruction}"
            );
        }
    }

    #[test]
    fn decodes_register_and_alu_instructions() {
        use Instruction::*;

        let mut env = create_env();
        assert_instructions_match(
            &mut env,
            &[
                halt(12),
                mov(3, 9),
                inv(4, 8),
                neg(5, 7),
                not0(6, 6),
                cnt1(7, 5),
                log2(8, 4),
                lsl(3, 2),
                lsr(4, 1),
                asr(15, 0),
                sp_add(0xa, 0x5),
                sp_sub(0x1, 0x2),
                and(1, 2, 3),
                or(4, 5, 6),
                xor(7, 8, 9),
                add(10, 11, 12),
                sub(12, 11, 10),
                addi(9, 0xf, 8),
                load_hi(0xa, 0xb, 7),
                load_lo(0xc, 0xd, 6),
            ],
        );
    }

    #[test]
    fn decodes_memory_and_stack_instructions() {
        use Instruction::*;

        let mut env = create_env();
        assert_instructions_match(
            &mut env,
            &[
                store_mem(3, 4, 0xf),
                load_mem(5, 0x7, 6),
                store_sp(0xa, 0xb, 7),
                load_sp(0xc, 0xd, 8),
            ],
        );
    }

    #[test]
    fn decodes_control_call_compare_and_device_instructions() {
        use Instruction::*;

        let mut env = create_env();
        assert_instructions_match(
            &mut env,
            &[
                pc(0xf, 3),
                jg(0x8, 0x1),
                je(0x0, 0x2),
                jge(0x7, 0xf),
                jl(0xf, 0xe),
                jne(0x1, 0x0),
                jle(0xe, 0xd),
                jmp(0x0, 0x4),
                cmp_r(2, 3),
                cmp_i(0xf, 4),
                cmp_s(5, 6),
                cmp_si(0x8, 7),
                call_rel(0xf, 0xc),
                call_abs(0xa, 0xb),
                jmp_reg(8),
                call_reg(9),
                dev_recv(2, 3, 4),
                dev_send(5, 6, 7),
            ],
        );
    }

    #[test]
    fn reset_suppresses_all_decoder_controls() {
        use Instruction::*;

        let mut env = create_env();
        env.instruction
            .set_u16(&mut env.circuit, store_mem(3, 4, 5).encode());
        env.reset.set(&mut env.circuit, 1);
        env.circuit.simulate();
        assert_eq!(
            snapshot(&env.circuit, &env.gates),
            snapshot(&env.circuit, &env.emu)
        );
        assert_eq!(
            snapshot(&env.circuit, &env.gates),
            DecoderSnapshot {
                source_a: 0,
                source_b: 0,
                destination: 0,
                immediate: 0,
                execute_operation: ExecOp::Idle as u8,
                register_write_enable: false,
                writeback_source: WbSrc::Execute as u8,
                flags_write_enable: false,
                memory_read_enable: false,
                memory_write_enable: false,
                pc_source: PcSrc::Next as u8,
                condition_mask: 0,
                device_index: 0,
                device_channel: 0,
                device_read_enable: false,
                device_write_enable: false,
                halt_enable: false,
            }
        );
    }
}
