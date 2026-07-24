use super::*;
use crate::isa::{
    hilo_as_u16, imm4_nz, imm8_as_i16, imm_as_i16, Cond, Instruction, RA_REG, SP_REG,
};
use crate::semantics::{calc_flags, calc_flags_signed};
use digital_design_code::{
    input as input_wire, input_w, CircuitComponentEmu, CircuitWires, Wire, WiresU16, WiresU8,
};

const DATA_MEMORY_WORDS: usize = 1 << WORD_WIDTH;

fn set_wire(circuit: &mut CircuitWires, wire: Wire, value: bool) {
    wire.set(circuit, u8::from(value));
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

pub struct CpuInstMemoryEmu;

impl CircuitComponentEmu<CpuInstMemory> for CpuInstMemoryEmu {
    fn create(input: &InstMemoryInput) -> (Self, InstMemoryOutput) {
        let instruction = input_w();
        instruction.set_latency_external(input.address.get_max_latency_external() + 1);
        (Self, InstMemoryOutput { instruction })
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &InstMemoryInput,
        output: &InstMemoryOutput,
    ) {
        let address = input.address.get_u16(circuit) as usize;
        let instruction = input.image.get(address).copied().unwrap_or(0);
        output.instruction.set_u16(circuit, instruction);
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

pub struct CpuRegisterReadEmu;

impl CircuitComponentEmu<CpuRegisterRead> for CpuRegisterReadEmu {
    fn create(input: &RegisterReadInput) -> (Self, RegisterReadOutput) {
        let output = RegisterReadOutput {
            source_a: input_w(),
            source_b: input_w(),
        };
        let register_latency = input
            .regs
            .iter()
            .map(|reg| reg.get_max_latency_external())
            .max()
            .unwrap_or(0);
        let latency = register_latency
            .max(input.source_a.get_max_latency_external())
            .max(input.source_b.get_max_latency_external())
            + 1;
        output.source_a.set_latency_external(latency);
        output.source_b.set_latency_external(latency);
        (Self, output)
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &RegisterReadInput,
        output: &RegisterReadOutput,
    ) {
        let source_a = input.source_a.get_u8(circuit) as usize;
        let source_b = input.source_b.get_u8(circuit) as usize;
        output
            .source_a
            .set_u16(circuit, input.regs[source_a].get_u16(circuit));
        output
            .source_b
            .set_u16(circuit, input.regs[source_b].get_u16(circuit));
    }
}

pub struct CpuExecuteEmu;

impl CircuitComponentEmu<CpuExecute> for CpuExecuteEmu {
    fn create(input: &ExecuteInput) -> (Self, ExecuteOutput) {
        let output = ExecuteOutput {
            result: input_w(),
            flags: input_w(),
            memory_address: input_w(),
            memory_write: input_w(),
            pc_target: input_w(),
            device_write: input_w(),
            halt_signal: input_w(),
        };
        let latency = input
            .pc
            .get_max_latency_external()
            .max(input.source_a.get_max_latency_external())
            .max(input.source_b.get_max_latency_external())
            .max(input.immediate.get_max_latency_external())
            .max(input.operation.get_max_latency_external())
            + 1;
        output.result.set_latency_external(latency);
        output.flags.set_latency_external(latency);
        output.memory_address.set_latency_external(latency);
        output.memory_write.set_latency_external(latency);
        output.pc_target.set_latency_external(latency);
        output.device_write.set_latency_external(latency);
        output.halt_signal.set_latency_external(latency);
        (Self, output)
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &ExecuteInput,
        output: &ExecuteOutput,
    ) {
        let pc = input.pc.get_u16(circuit);
        let source_a = input.source_a.get_u16(circuit);
        let source_b = input.source_b.get_u16(circuit);
        let immediate = input.immediate.get_u16(circuit);
        let operation = ExecOp::from_raw(input.operation.get_u8(circuit));

        let mut result = 0;
        let mut flags = 0;
        let mut memory_address = 0;
        let mut pc_target = 0;

        match operation {
            ExecOp::Idle => {}
            ExecOp::PassA => {
                result = source_a;
                pc_target = source_a;
            }
            ExecOp::Inv => result = !source_a,
            ExecOp::Neg => result = (source_a as i16).wrapping_neg() as u16,
            ExecOp::NotZero => result = u16::from(source_a != 0),
            ExecOp::CountOnes => result = source_a.count_ones() as u16,
            ExecOp::Log2 => {
                result = if source_a == 0 {
                    0
                } else {
                    source_a.ilog2() as u16
                }
            }
            ExecOp::Lsl => result = source_a << immediate,
            ExecOp::Lsr => result = source_a >> immediate,
            ExecOp::Asr => result = ((source_a as i16) >> immediate) as u16,
            ExecOp::And => result = source_a & source_b,
            ExecOp::Or => result = source_a | source_b,
            ExecOp::Xor => result = source_a ^ source_b,
            ExecOp::Add => result = source_a.wrapping_add(source_b),
            ExecOp::Sub => result = source_a.wrapping_sub(source_b),
            ExecOp::AddImmediate => {
                result = source_a.wrapping_add(immediate);
                memory_address = result;
            }
            ExecOp::LoadHi => result = immediate | (source_a & 0x00ff),
            ExecOp::LoadLo => result = immediate,
            ExecOp::PcAdd => {
                result = pc.wrapping_add(immediate);
                pc_target = result;
            }
            ExecOp::CompareUnsigned => flags = calc_flags(source_a, source_b),
            ExecOp::CompareUnsignedImmediate => flags = calc_flags(source_a, immediate),
            ExecOp::CompareSigned => flags = calc_flags_signed(source_a, source_b),
            ExecOp::CompareSignedImmediate => flags = calc_flags_signed(source_a, immediate),
            ExecOp::CallRelative => {
                result = pc.wrapping_add(1);
                pc_target = pc.wrapping_add(immediate);
            }
            ExecOp::CallAbsolute => {
                result = pc.wrapping_add(1);
                memory_address = immediate;
            }
            ExecOp::CallRegister => {
                result = pc.wrapping_add(1);
                pc_target = source_a;
            }
            ExecOp::Max => unreachable!(),
        }

        output.result.set_u16(circuit, result);
        output.flags.set_u8(circuit, flags);
        output.memory_address.set_u16(circuit, memory_address);
        output.memory_write.set_u16(circuit, source_b);
        output.pc_target.set_u16(circuit, pc_target);
        output.device_write.set_u16(circuit, source_a);
        output.halt_signal.set_u16(circuit, source_a);
    }
}

pub struct CpuDataMemoryEmu {
    memory: Box<[u16]>,
    pending_write: Option<(usize, u16)>,
}

impl CircuitComponentEmu<CpuDataMemory> for CpuDataMemoryEmu {
    fn create(input: &DataMemoryInput) -> (Self, DataMemoryOutput) {
        let read_data = input_w();
        let latency = input
            .address
            .get_max_latency_external()
            .max(input.read_enable.get_latency_external())
            .max(input.write_enable.get_latency_external())
            .max(input.write_data.get_max_latency_external())
            + 1;
        read_data.set_latency_external(latency);
        (
            Self {
                memory: vec![0; DATA_MEMORY_WORDS].into_boxed_slice(),
                pending_write: None,
            },
            DataMemoryOutput { read_data },
        )
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &DataMemoryInput,
        output: &DataMemoryOutput,
    ) {
        let address = input.address.get_u16(circuit) as usize;
        let read_data = if input.read_enable.is_one(circuit) {
            self.memory[address]
        } else {
            0
        };
        output.read_data.set_u16(circuit, read_data);

        self.pending_write = input
            .write_enable
            .is_one(circuit)
            .then(|| (address, input.write_data.get_u16(circuit)));
    }

    fn clock(
        &mut self,
        _circuit: &mut CircuitWires,
        _input: &DataMemoryInput,
        _output: &DataMemoryOutput,
    ) {
        if let Some((address, value)) = self.pending_write.take() {
            self.memory[address] = value;
        }
    }
}

pub struct CpuWritebackEmu;

impl CircuitComponentEmu<CpuWriteback> for CpuWritebackEmu {
    fn create(input: &WritebackInput) -> (Self, WritebackOutput) {
        let output = WritebackOutput {
            regs: [(); REGISTER_COUNT].map(|_| input_w()),
        };
        let register_latency = input
            .regs
            .iter()
            .map(|reg| reg.get_max_latency_external())
            .max()
            .unwrap_or(0);
        let latency = register_latency
            .max(input.reset.get_latency_external())
            .max(input.destination.get_max_latency_external())
            .max(input.write_enable.get_latency_external())
            .max(input.source.get_max_latency_external())
            .max(input.execute_data.get_max_latency_external())
            .max(input.memory_data.get_max_latency_external())
            .max(input.device_data.get_max_latency_external())
            + 1;
        for reg in output.regs {
            reg.set_latency_external(latency);
        }
        (Self, output)
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &WritebackInput,
        output: &WritebackOutput,
    ) {
        let mut regs = input.regs.map(|reg| reg.get_u16(circuit));

        if input.reset.is_one(circuit) {
            regs.fill(0);
        } else if input.write_enable.is_one(circuit) {
            let data = match WbSrc::from_raw(input.source.get_u8(circuit)) {
                WbSrc::Execute => input.execute_data.get_u16(circuit),
                WbSrc::Memory => input.memory_data.get_u16(circuit),
                WbSrc::Device => input.device_data.get_u16(circuit),
            };
            regs[input.destination.get_u8(circuit) as usize] = data;
        }

        for (output, value) in output.regs.iter().zip(regs) {
            output.set_u16(circuit, value);
        }
    }
}

pub struct CpuControlFlowEmu;

impl CircuitComponentEmu<CpuControlFlow> for CpuControlFlowEmu {
    fn create(input: &ControlFlowInput) -> (Self, ControlFlowOutput) {
        let output = ControlFlowOutput {
            pc: input_w(),
            flags: input_w(),
            halted: input_wire(),
            halt_signal: input_w(),
        };
        let latency = input
            .pc
            .get_max_latency_external()
            .max(input.flags.get_max_latency_external())
            .max(input.halted.get_latency_external())
            .max(input.reset.get_latency_external())
            .max(input.flags_write_enable.get_latency_external())
            .max(input.flags_write.get_max_latency_external())
            .max(input.pc_source.get_max_latency_external())
            .max(input.condition_mask.get_max_latency_external())
            .max(input.pc_target.get_max_latency_external())
            .max(input.memory_target.get_max_latency_external())
            .max(input.halt_enable.get_latency_external())
            .max(input.halt_signal.get_max_latency_external())
            + 1;
        output.pc.set_latency_external(latency);
        output.flags.set_latency_external(latency);
        output.halted.set_latency_external(latency);
        output.halt_signal.set_latency_external(latency);
        (Self, output)
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &ControlFlowInput,
        output: &ControlFlowOutput,
    ) {
        let current_pc = input.pc.get_u16(circuit);
        let current_flags = input.flags.get_u8(circuit);

        let (pc, flags, halted, halt_signal) = if input.reset.is_one(circuit) {
            (0, 0, false, 0)
        } else if input.halted.is_one(circuit) || input.halt_enable.is_one(circuit) {
            (
                current_pc,
                current_flags,
                true,
                input.halt_signal.get_u16(circuit),
            )
        } else {
            let flags = if input.flags_write_enable.is_one(circuit) {
                input.flags_write.get_u8(circuit)
            } else {
                current_flags
            };
            let condition = input.condition_mask.get_u8(circuit);
            let branch_taken = current_flags & condition != 0;
            let pc = match PcSrc::from_raw(input.pc_source.get_u8(circuit)) {
                PcSrc::Next => current_pc.wrapping_add(1),
                PcSrc::Execute if branch_taken => input.pc_target.get_u16(circuit),
                PcSrc::Memory if branch_taken => input.memory_target.get_u16(circuit),
                PcSrc::Execute | PcSrc::Memory => current_pc.wrapping_add(1),
            };
            (pc, flags, false, 0)
        };

        output.pc.set_u16(circuit, pc);
        output.flags.set_u8(circuit, flags);
        set_wire(circuit, output.halted, halted);
        output.halt_signal.set_u16(circuit, halt_signal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimEnv;
    use digital_design_code::{build_circuit, input_w, Circuit};

    fn build_emu(instructions: &[Instruction]) -> (Circuit, CpuV2State, CpuV2Output, Wire) {
        let instruction_image: InstructionImage = instructions
            .iter()
            .map(Instruction::encode)
            .collect::<Vec<_>>()
            .into();

        let (circuit, (state, output, reset)) = build_circuit(|| {
            let state = CpuV2State::create();
            let reset = input_wire();
            let output = CpuV2EmuInstance::build(&CpuV2BuildInput {
                state: state.clone(),
                ports: CpuV2Input {
                    reset,
                    device_read: input_w(),
                },
                instruction_image,
            });
            (state, output, reset)
        });

        (circuit, state, output, reset)
    }

    fn assert_state_matches(circuit: &CircuitWires, state: &CpuV2State, reference: &SimEnv) {
        assert_eq!(state.pc.out.get_u16(circuit), reference.state.pc);
        assert_eq!(state.flags.out.get_u8(circuit), reference.state.flags);
        for (index, reg) in state.regs.iter().enumerate() {
            assert_eq!(
                reg.out.get_u16(circuit),
                reference.state.reg[index],
                "register r{index} differs"
            );
        }
    }

    #[test]
    fn emu_matches_sim_for_basic_program() {
        use Instruction::*;

        const MAX_CYCLES: usize = 16;
        let program = [
            load_lo(0, 5, 0),
            load_lo(0, 3, 1),
            add(0, 1, 2),
            load_lo(1, 0, 3),
            store_mem(3, 2, 0),
            load_mem(3, 0, 4),
            cmp_i(8, 4),
            je(0, 2),
            halt(0),
            halt(4),
        ];
        let (mut circuit, state, output, _) = build_emu(&program);
        let mut reference = SimEnv::new(&program);

        for _ in 0..MAX_CYCLES {
            let change = reference.eval();
            reference.commit(change);
            circuit.simulate();
            assert_state_matches(&circuit, &state, &reference);

            if let Some(signal) = change.halt {
                assert!(output.halted.is_one(&circuit));
                assert_eq!(output.halt_signal.get_u16(&circuit), signal);
                return;
            }
        }

        panic!("program did not halt within {MAX_CYCLES} cycles");
    }

    #[test]
    fn reset_clears_cpu_state() {
        use Instruction::*;

        let program = [load_lo(0, 7, 0), halt(0)];
        let (mut circuit, state, output, reset) = build_emu(&program);

        circuit.simulate();
        assert_eq!(state.regs[0].out.get_u16(&circuit), 7);
        assert_eq!(state.pc.out.get_u16(&circuit), 1);

        reset.set(&mut circuit, 1);
        circuit.simulate();
        assert_eq!(state.pc.out.get_u16(&circuit), 0);
        assert_eq!(state.flags.out.get_u8(&circuit), 0);
        assert!(!state.halted.out().is_one(&circuit));
        assert!(!output.halted.is_one(&circuit));
        for reg in state.regs {
            assert_eq!(reg.out.get_u16(&circuit), 0);
        }
    }
}
