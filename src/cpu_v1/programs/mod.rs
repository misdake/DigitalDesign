use crate::cpu_v1::isa::InstBinary;
use crate::cpu_v1::{cpu_v1_build_with_ref, CpuV1State};
use crate::{clock_tick, execute_gates};

mod example;
mod test_alu;
mod test_jmp;
mod test_mem;
mod test_perf;

fn print_regs(_: u32, state: &CpuV1State) {
    print!(
        "regs: {} {} {} {}",
        state.reg[0].out.get_u8(),
        state.reg[1].out.get_u8(),
        state.reg[2].out.get_u8(),
        state.reg[3].out.get_u8()
    );
    println!();
}

fn test_cpu(
    inst: &[InstBinary],
    max_cycle: u32,
    mut f: impl FnMut(u32, &CpuV1State),
) -> CpuV1State {
    let mut inst_rom = [0u8; 256];
    inst.iter()
        .enumerate()
        .for_each(|(i, inst)| inst_rom[i] = inst.binary);

    let (state, state_ref, internal, internal_ref) = cpu_v1_build_with_ref(inst_rom);

    for i in 0..max_cycle {
        assert_eq!(state.pc.out.get_u8(), state_ref.pc.out.get_u8());

        let pc = state.pc.out.get_u8();
        if pc as usize >= inst.len() {
            break;
        }
        let inst_desc = inst[pc as usize];
        println!(
            "pc {:08b}: inst {} {:08b}",
            pc,
            inst_desc.desc.name(),
            inst_desc.binary
        );

        execute_gates();

        println!("internal: {internal:?}");
        println!("internal_ref: {internal_ref:?}");

        clock_tick();

        for j in 0..4 {
            assert_eq!(state.reg[j].out.get_u8(), state_ref.reg[j].out.get_u8());
        }
        for j in 0..256 {
            assert_eq!(state.mem[j].out.get_u8(), state_ref.mem[j].out.get_u8());
        }
        assert_eq!(state.mem_bank.out.get_u8(), state_ref.mem_bank.out.get_u8());

        f(i, &state);
    }

    state
}
