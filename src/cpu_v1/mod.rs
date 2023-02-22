pub mod isa;
pub use isa::*;
mod pc;

use crate::cpu_v1::pc::{CpuPc, CpuPcEmu, CpuPcInput, CpuPcOutput};
use crate::{
    clear_all, external, input, input_w, reg, reg_w, simulate, External, Reg, Regs, Wires,
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
    fn create() -> Self {
        Self {
            inst: [Wires::uninitialized(); 256],
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
    pc_in: CpuPcInput,
    pc_out: CpuPcOutput,
}

trait CpuV1 {
    type Pc: CpuComponent<Input = CpuPcInput, Output = CpuPcOutput>;
    fn build(state: &mut CpuV1State) -> CpuV1StateInternal {
        let pc = &mut state.pc;
        let curr_pc = pc.out;

        let pc_in = CpuPcInput {
            curr_pc,
            jmp_offset_enable: input(),
            jmp_offset: input_w::<4>(),
            jmp_long_enable: input(),
            jmp_long: input_w::<4>(),
            no_jmp_enable: input(),
        };
        let pc_out: CpuPcOutput = Self::Pc::build(&pc_in);

        pc.set_in(pc_out.next_pc);

        CpuV1StateInternal { pc_in, pc_out }
    }
}

struct CpuV1Instance;
struct CpuV1EmuInstance;
impl CpuV1 for CpuV1Instance {
    type Pc = CpuPc;
}
impl CpuV1 for CpuV1EmuInstance {
    type Pc = CpuComponentEmuContext<CpuPc, CpuPcEmu>;
}
#[test]
fn cpu_v1_build() {
    clear_all();
    let mut state1 = CpuV1State::create();
    let mut state2 = CpuV1State::create();
    let internal1 = CpuV1Instance::build(&mut state1);
    let internal2 = CpuV1EmuInstance::build(&mut state2);
    internal1.pc_in.curr_pc.set_u8(123);
    internal2.pc_in.curr_pc.set_u8(123);
    internal1.pc_in.no_jmp_enable.set(1);
    internal2.pc_in.no_jmp_enable.set(1);
    simulate();

    let next_pc1 = internal1.pc_out.next_pc.get_u8();
    let next_pc2 = internal2.pc_out.next_pc.get_u8();
    assert_eq!(next_pc1, next_pc2);
    //TODO simulate
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
