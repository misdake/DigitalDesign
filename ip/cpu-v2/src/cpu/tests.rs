use super::*;
use crate::{Instruction, SimEnv};
use digital_design_circuit::{
    build_circuit, input, input_w, Circuit, CircuitWires, Wire, WiresU16, WiresU8,
};

fn build_cpu<D: CpuV2Design>(
    instructions: &[Instruction],
) -> (Circuit, CpuV2State, CpuV2Output, Wire) {
    let instruction_image: InstructionImage = instructions
        .iter()
        .map(Instruction::encode)
        .collect::<Vec<_>>()
        .into();

    let (circuit, (state, output, reset)) = build_circuit(|| {
        let state = CpuV2State::create();
        let reset = input();
        let output = D::build(&CpuV2BuildInput {
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

fn assert_design_matches_sim<D: CpuV2Design>(program: &[Instruction], max_cycles: usize) {
    let (mut circuit, state, output, _) = build_cpu::<D>(program);
    let mut reference = SimEnv::new(program);

    for _ in 0..max_cycles {
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

    panic!("program did not halt within {max_cycles} cycles");
}

fn basic_program() -> [Instruction; 10] {
    use Instruction::*;

    [
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
    ]
}

#[test]
fn emu_matches_sim_for_basic_program() {
    const MAX_CYCLES: usize = 16;
    assert_design_matches_sim::<CpuV2EmuInstance>(&basic_program(), MAX_CYCLES);
}

#[test]
fn gates_match_sim_for_basic_program() {
    const MAX_CYCLES: usize = 16;
    assert_design_matches_sim::<CpuV2Instance>(&basic_program(), MAX_CYCLES);
}

#[test]
fn reset_clears_cpu_state() {
    use Instruction::*;

    let program = [load_lo(0, 7, 0), halt(0)];
    let (mut circuit, state, output, reset) = build_cpu::<CpuV2EmuInstance>(&program);

    circuit.simulate();
    assert_eq!(state.regs[0].out.get_u16(&circuit), 7);
    assert_eq!(state.pc.out.get_u16(&circuit), 1);

    reset.set(&mut circuit, 1);
    circuit.simulate();
    assert_eq!(state.pc.out.get_u16(&circuit), 0);
    assert_eq!(state.flags.out.get_u8(&circuit), 0);
    assert!(!output.halted.is_one(&circuit));
    for reg in state.regs {
        assert_eq!(reg.out.get_u16(&circuit), 0);
    }
}
