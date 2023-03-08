pub mod isa;
pub use isa::*;
mod inst_rom;
use inst_rom::*;
mod pc;
use pc::*;
mod branch;
use branch::*;
mod decoder;
use decoder::*;

use crate::{
    clear_all, external, input, input_w, reg, reg_w, simulate, External, Reg, Regs, Rom256x8, Wires,
};
use std::any::Any;
use std::marker::PhantomData;

struct CpuV1State {
    inst: [Wires<8>; 256],
    pc: Regs<8>,
    mem: [Regs<4>; 16],
    reg: [Regs<4>; 4],
    flag_p: Reg,
    flag_z: Reg,
    flag_n: Reg,
}
impl CpuV1State {
    fn create(inst: [u8; 256]) -> Self {
        Self {
            inst: inst.map(|v| Wires::<8>::parse_u8(v)),
            pc: reg_w(),
            mem: [reg_w(); 16],
            reg: [reg_w(); 4],
            flag_p: reg(),
            flag_z: reg(),
            flag_n: reg(),
        }
    }
}

struct CpuV1StateInternal {
    inst_rom_in: CpuInstInput,
    inst_rom_out: CpuInstOutput,

    next_pc_in: CpuPcInput,
    next_pc_out: CpuPcOutput,
}

trait CpuV1 {
    type Pc: CpuComponent<Input = CpuPcInput, Output = CpuPcOutput>;
    type InstRom: CpuComponent<Input = CpuInstInput, Output = CpuInstOutput>;

    fn build(state: &mut CpuV1State) -> CpuV1StateInternal {
        let pc = &mut state.pc;
        let curr_pc = pc.out;

        let inst_rom_in = CpuInstInput {
            inst: state.inst,
            pc: curr_pc,
        };

        let inst_rom_out = Self::InstRom::build(&inst_rom_in);

        let next_pc_in = CpuPcInput {
            curr_pc,
            jmp_offset_enable: input(),
            jmp_offset: input_w::<4>(),
            jmp_long_enable: input(),
            jmp_long: input_w::<4>(),
            no_jmp_enable: input(),
        };
        let next_pc_out: CpuPcOutput = Self::Pc::build(&next_pc_in);

        pc.set_in(next_pc_out.next_pc);

        CpuV1StateInternal {
            inst_rom_in,
            inst_rom_out,
            next_pc_in,
            next_pc_out,
        }
    }
}

struct CpuV1Instance;
struct CpuV1EmuInstance;
impl CpuV1 for CpuV1Instance {
    type Pc = CpuPc;
    type InstRom = CpuInstRom;
}
impl CpuV1 for CpuV1EmuInstance {
    type Pc = CpuComponentEmuContext<CpuPc, CpuPcEmu>;
    type InstRom = CpuComponentEmuContext<CpuInstRom, CpuInstRomEmu>;
}

#[test]
fn test() {
    cpu_v1_build();
}

pub fn cpu_v1_build() {
    clear_all();

    let mut inst_rom = [0u8; 256];
    inst_rom[0] = inst_mov(0, 1).binary;
    inst_rom[1] = inst_add(0, 1).binary;

    let mut state1 = CpuV1State::create(inst_rom.clone());
    let mut state2 = CpuV1State::create(inst_rom.clone());
    let internal1 = CpuV1Instance::build(&mut state1);
    let internal2 = CpuV1EmuInstance::build(&mut state2);
    internal1.next_pc_in.curr_pc.set_u8(0);
    internal2.next_pc_in.curr_pc.set_u8(0);
    internal1.next_pc_in.no_jmp_enable.set(1);
    internal2.next_pc_in.no_jmp_enable.set(1);

    simulate();
    let inst1 = internal1.inst_rom_out.inst.get_u8();
    let inst2 = internal2.inst_rom_out.inst.get_u8();
    assert_eq!(inst1, inst2);
    let binary = InstDesc::parse(inst1).unwrap();
    println!("inst: {}", binary.desc.name());

    let next_pc1 = internal1.next_pc_out.next_pc.get_u8();
    let next_pc2 = internal2.next_pc_out.next_pc.get_u8();
    assert_eq!(next_pc1, next_pc2);

    simulate();
    let inst1 = internal1.inst_rom_out.inst.get_u8();
    let inst2 = internal2.inst_rom_out.inst.get_u8();
    assert_eq!(inst1, inst2);
    let binary = InstDesc::parse(inst1).unwrap();
    println!("inst: {}", binary.desc.name());

    let next_pc1 = internal1.next_pc_out.next_pc.get_u8();
    let next_pc2 = internal2.next_pc_out.next_pc.get_u8();
    assert_eq!(next_pc1, next_pc2);
}

pub trait CpuComponent: Any {
    type Input: Clone;
    type Output: Clone;
    fn build(input: &Self::Input) -> Self::Output;
}

pub trait CpuComponentEmu<T: CpuComponent>: Sized + Any {
    fn init_output() -> T::Output;
    fn execute(input: &T::Input, output: &T::Output);
    fn build(input: &T::Input) -> T::Output {
        let output = Self::init_output();
        let ctx: CpuComponentEmuContext<T, Self> = CpuComponentEmuContext {
            _phantom: Default::default(),
            input: input.clone(),
            output: output.clone(),
        };
        external(ctx);
        output
    }
}
struct CpuComponentEmuContext<T: CpuComponent, E: CpuComponentEmu<T>> {
    _phantom: PhantomData<E>,
    input: T::Input,
    output: T::Output,
}
impl<T: CpuComponent, E: CpuComponentEmu<T>> External for CpuComponentEmuContext<T, E> {
    fn execute(&mut self) {
        E::execute(&self.input, &mut self.output);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl<T: CpuComponent, E: CpuComponentEmu<T>> CpuComponent for CpuComponentEmuContext<T, E> {
    type Input = T::Input;
    type Output = T::Output;
    fn build(input: &Self::Input) -> Self::Output {
        E::build(input)
    }
}
