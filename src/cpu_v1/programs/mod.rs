use crate::cpu_v1::emu::{EmuEnv, EmuState};
use crate::cpu_v1::isa::Instruction;
use crate::cpu_v1::{cpu_v1_build_mix, CpuV1State};
use crate::{clock_tick, execute_gates};

mod example;
mod game_sokoban;
mod test_alu;
mod test_jmp;
mod test_mem;
mod test_perf;

fn print_regs(cycle: u32, state: &CpuV1State) {
    print!(
        "cycle {}, regs: {} {} {} {}",
        cycle,
        state.reg[0].out.get_u8(),
        state.reg[1].out.get_u8(),
        state.reg[2].out.get_u8(),
        state.reg[3].out.get_u8()
    );
    println!();
}

fn test_cpu_with_emu(inst: &[Instruction], max_cycle: u32, mut f: impl FnMut(u32, &CpuV1State)) {
    let mut inst_rom = [Instruction::default(); 256];
    inst.iter()
        .enumerate()
        .for_each(|(i, inst)| inst_rom[i] = *inst);

    let (state, _) = cpu_v1_build_mix(inst_rom);
    let mut emu = EmuEnv::new(inst_rom);

    for i in 0..max_cycle {
        let pc = state.pc.out.get_u8();
        if pc as usize >= inst.len() {
            break;
        }
        let inst_desc = inst[pc as usize];
        println!("pc {:08b}: inst {}", pc, inst_desc.to_string());

        execute_gates();
        clock_tick();

        emu.clock();

        let test_state = state.export_emu_state();
        let emu_state = emu.get_state();

        if test_state != *emu_state {
            panic!(
                "State not match! diff (test) (emu):\n{}",
                EmuState::diff(&test_state, emu_state)
            );
        }

        f(i, &state);
    }
}
