use super::{set_wire, PcSrc, FLAGS_WIDTH, PC_SRC_WIDTH, WORD_WIDTH};
use digital_design_circuit::{
    add_naive, input as input_wire, input_const, input_w, input_w_const, mux2, mux2_w,
    CircuitComponent, CircuitComponentEmu, CircuitWires, Wire, Wires, WiresU16, WiresU8,
};

#[derive(Clone)]
pub struct ControlFlowInput {
    pub reset: Wire,
    pub pc: Wires<WORD_WIDTH>,
    pub flags: Wires<FLAGS_WIDTH>,
    pub halted: Wire,

    pub flags_write_enable: Wire,
    pub flags_write: Wires<FLAGS_WIDTH>,

    pub pc_source: Wires<PC_SRC_WIDTH>,
    pub condition_mask: Wires<FLAGS_WIDTH>,
    pub pc_target: Wires<WORD_WIDTH>,
    pub memory_target: Wires<WORD_WIDTH>,

    pub halt_enable: Wire,
    pub halt_signal: Wires<WORD_WIDTH>,
}

#[derive(Clone)]
pub struct ControlFlowOutput {
    pub pc: Wires<WORD_WIDTH>,
    pub flags: Wires<FLAGS_WIDTH>,
    pub halted: Wire,
    pub halt_signal: Wires<WORD_WIDTH>,
}

pub struct CpuControlFlow;

impl CircuitComponent for CpuControlFlow {
    type Input = ControlFlowInput;
    type Output = ControlFlowOutput;

    fn build(input: &Self::Input) -> Self::Output {
        let condition_matches = (input.flags & input.condition_mask)
            .wires
            .iter()
            .copied()
            .fold(input_const(0), |matched, bit| matched | bit);
        let pc_from_execute = input.pc_source.eq_const(PcSrc::Execute as u8);
        let pc_from_memory = input.pc_source.eq_const(PcSrc::Memory as u8);
        let branch_taken = condition_matches & (pc_from_execute | pc_from_memory);
        let branch_target = mux2_w(input.pc_target, input.memory_target, pc_from_memory);
        let next_pc = add_naive(input.pc, Wires::<WORD_WIDTH>::parse_u16(1)).sum;
        let running_pc = mux2_w(next_pc, branch_target, branch_taken);
        let halt = input.halted | input.halt_enable;
        let held_pc = mux2_w(running_pc, input.pc, halt);
        let flags_write = mux2_w(input.flags, input.flags_write, input.flags_write_enable);
        let held_flags = mux2_w(flags_write, input.flags, halt);
        let zero_word = input_w_const(0);
        let zero_flags = input_w_const(0);

        ControlFlowOutput {
            pc: mux2_w(held_pc, zero_word, input.reset),
            flags: mux2_w(held_flags, zero_flags, input.reset),
            halted: mux2(halt, input_const(0), input.reset),
            halt_signal: mux2_w(zero_word, input.halt_signal, halt & !input.reset),
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
    use digital_design_circuit::{build_circuit, input, input_w};

    #[test]
    fn advances_branches_halts_and_resets() {
        let (
            mut circuit,
            (
                reset,
                pc,
                flags,
                halted,
                pc_source,
                condition_mask,
                pc_target,
                halt_enable,
                halt_signal,
                output,
            ),
        ) = build_circuit(|| {
            let reset = input();
            let pc = input_w();
            let flags = input_w();
            let halted = input();
            let pc_source = input_w();
            let condition_mask = input_w();
            let pc_target = input_w();
            let halt_enable = input();
            let halt_signal = input_w();
            let output = CpuControlFlow::build(&ControlFlowInput {
                reset,
                pc,
                flags,
                halted,
                flags_write_enable: input(),
                flags_write: input_w(),
                pc_source,
                condition_mask,
                pc_target,
                memory_target: input_w(),
                halt_enable,
                halt_signal,
            });
            (
                reset,
                pc,
                flags,
                halted,
                pc_source,
                condition_mask,
                pc_target,
                halt_enable,
                halt_signal,
                output,
            )
        });

        pc.set_u16(&mut circuit, 0x1000);
        flags.set_u8(&mut circuit, 0b010);
        pc_source.set_u8(&mut circuit, PcSrc::Execute as u8);
        condition_mask.set_u8(&mut circuit, 0b010);
        pc_target.set_u16(&mut circuit, 0x2345);
        circuit.execute_gates();
        assert_eq!(output.pc.get_u16(&circuit), 0x2345);

        halt_enable.set(&mut circuit, 1);
        halt_signal.set_u16(&mut circuit, 0x55aa);
        circuit.execute_gates();
        assert_eq!(output.pc.get_u16(&circuit), 0x1000);
        assert!(output.halted.is_one(&circuit));
        assert_eq!(output.halt_signal.get_u16(&circuit), 0x55aa);

        halted.set(&mut circuit, 1);
        reset.set(&mut circuit, 1);
        circuit.execute_gates();
        assert_eq!(output.pc.get_u16(&circuit), 0);
        assert_eq!(output.flags.get_u8(&circuit), 0);
        assert!(!output.halted.is_one(&circuit));
    }
}
