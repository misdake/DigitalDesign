use crate::emu::{EmuEnv, EmuState};
use crate::isa::Instruction;
use crate::{build_cpu_v1_sim, CpuV1State};
use digital_design_circuit::{CircuitWires, WiresU8};

mod example;
mod game_sokoban;
mod test_alu;
mod test_jmp;
mod test_mem;
mod test_perf;

fn print_regs(cycle: u32, state: &CpuV1State, circuit: &CircuitWires) {
    print!(
        "cycle {}, regs: {} {} {} {}",
        cycle,
        state.reg[0].out.get_u8(circuit),
        state.reg[1].out.get_u8(circuit),
        state.reg[2].out.get_u8(circuit),
        state.reg[3].out.get_u8(circuit)
    );
    println!();
}

fn test_cpu_with_emu(
    inst: &[Instruction],
    max_cycle: u32,
    mut f: impl FnMut(u32, &CpuV1State, &CircuitWires),
) {
    let mut inst_rom = [Instruction::default(); 256];
    inst.iter()
        .enumerate()
        .for_each(|(i, inst)| inst_rom[i] = *inst);

    let (mut circuit, state, _) = build_cpu_v1_sim(inst_rom);
    let mut emu = EmuEnv::new(inst_rom);

    for i in 0..max_cycle {
        let pc = state.pc.out.get_u8(&circuit);
        if pc as usize >= inst.len() {
            break;
        }
        let inst_desc = inst[pc as usize];
        println!("pc {:08b}: inst {}", pc, inst_desc.to_string());

        circuit.simulate();

        emu.clock();

        let test_state = state.export_emu_state(&mut circuit);
        let emu_state = emu.get_state();

        if test_state != *emu_state {
            panic!(
                "State not match! diff (test) (emu):\n{}",
                EmuState::diff(&test_state, emu_state)
            );
        }

        f(i, &state, &circuit);
    }
}
