use super::{
    ControlFlowInput, ControlFlowOutput, CpuV2BuildInput, CpuV2Output, DataMemoryInput,
    DataMemoryOutput, DecoderInput, DecoderOutput, ExecuteInput, ExecuteOutput, InstMemoryInput,
    InstMemoryOutput, RegisterReadInput, RegisterReadOutput, WritebackInput, WritebackOutput,
};
use digital_design_code::CircuitComponent;

/// Selects the implementation of each stage in the single-cycle data path.
///
/// The top-level builder will call these stages in data-flow order so gate
/// implementations and external-backed emulators can be mixed safely.
pub trait CpuV2Design {
    type InstMemory: CircuitComponent<Input = InstMemoryInput, Output = InstMemoryOutput>;
    type Decoder: CircuitComponent<Input = DecoderInput, Output = DecoderOutput>;
    type RegisterRead: CircuitComponent<Input = RegisterReadInput, Output = RegisterReadOutput>;
    type Execute: CircuitComponent<Input = ExecuteInput, Output = ExecuteOutput>;
    type DataMemory: CircuitComponent<Input = DataMemoryInput, Output = DataMemoryOutput>;
    type Writeback: CircuitComponent<Input = WritebackInput, Output = WritebackOutput>;
    type ControlFlow: CircuitComponent<Input = ControlFlowInput, Output = ControlFlowOutput>;

    fn build(input: &CpuV2BuildInput) -> CpuV2Output {
        let state = &input.state;
        let ports = &input.ports;

        let inst_memory = Self::InstMemory::build(&InstMemoryInput {
            address: state.pc.out,
            image: input.instruction_image.clone(),
        });

        let decoder = Self::Decoder::build(&DecoderInput {
            instruction: inst_memory.instruction,
            reset: ports.reset,
        });

        let register_read = Self::RegisterRead::build(&RegisterReadInput {
            regs: state.regs.map(|reg| reg.out),
            source_a: decoder.source_a,
            source_b: decoder.source_b,
        });

        let execute = Self::Execute::build(&ExecuteInput {
            pc: state.pc.out,
            source_a: register_read.source_a,
            source_b: register_read.source_b,
            immediate: decoder.immediate,
            operation: decoder.execute_operation,
        });

        let data_memory = Self::DataMemory::build(&DataMemoryInput {
            address: execute.memory_address,
            read_enable: decoder.memory_read_enable,
            write_enable: decoder.memory_write_enable,
            write_data: execute.memory_write,
        });

        let writeback = Self::Writeback::build(&WritebackInput {
            reset: ports.reset,
            regs: state.regs.map(|reg| reg.out),
            destination: decoder.destination,
            write_enable: decoder.register_write_enable,
            source: decoder.writeback_source,
            execute_data: execute.result,
            memory_data: data_memory.read_data,
            device_data: ports.device_read,
        });

        let control_flow = Self::ControlFlow::build(&ControlFlowInput {
            reset: ports.reset,
            pc: state.pc.out,
            flags: state.flags.out,
            halted: state.halted.out(),
            flags_write_enable: decoder.flags_write_enable,
            flags_write: execute.flags,
            pc_source: decoder.pc_source,
            condition_mask: decoder.condition_mask,
            pc_target: execute.pc_target,
            memory_target: data_memory.read_data,
            halt_enable: decoder.halt_enable,
            halt_signal: execute.halt_signal,
        });

        for (reg, next) in state.regs.iter().zip(writeback.regs) {
            reg.set_in(next);
        }
        state.pc.set_in(control_flow.pc);
        state.flags.set_in(control_flow.flags);
        state.halted.set_in(control_flow.halted);

        CpuV2Output {
            device_index: decoder.device_index,
            device_channel: decoder.device_channel,
            device_read_enable: decoder.device_read_enable,
            device_write_enable: decoder.device_write_enable,
            device_write: execute.device_write,
            halted: control_flow.halted,
            halt_signal: control_flow.halt_signal,
        }
    }
}
