use super::{
    set_wire, ExecOp, PcSrc, WbSrc, EXEC_OP_WIDTH, FLAGS_WIDTH, PC_SRC_WIDTH, REG_INDEX_WIDTH,
    WB_SRC_WIDTH, WORD_WIDTH,
};
use crate::isa::{
    hilo_as_u16, imm4_nz, imm8_as_i16, imm_as_i16, Cond, Instruction, RA_REG, SP_REG,
};
use digital_design_code::{
    input as input_wire, input_w, CircuitComponent, CircuitComponentEmu, CircuitWires, Wire, Wires,
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

impl CircuitComponent for CpuDecoder {
    type Input = DecoderInput;
    type Output = DecoderOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 decoder implementation")
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
