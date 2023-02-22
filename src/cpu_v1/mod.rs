pub mod isa;
pub use isa::*;
mod pc;

use crate::cpu_v1::pc::{CpuPc, CpuPcEmu, CpuPcInput, CpuPcOutput};
use crate::{external, input, input_w, reg, reg_w, External, Reg, Regs, Wires};
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
            inst: [Wires::create_uninitialized(); 256],
            pc: reg_w(),
            mem: [reg_w(); 16],
            reg: [reg_w(); 4],
            flag_p: reg(),
            flag_z: reg(),
            flag_n: reg(),
        }
    }
}

trait CpuV1 {
    type Pc: CpuComponent<Input = CpuPcInput, Output = CpuPcOutput>;
    fn build(state: &mut CpuV1State) {
        let pc = &mut state.pc;
        let pc_in = CpuPcInput {
            prev_pc: pc.out,
            jmp_offset_enable: input(),
            jmp_offset: input_w::<4>(),
            jmp_long_enable: input(),
            jmp_long: input_w::<4>(),
            no_jmp_enable: input(),
        };
        let next_pc = Self::Pc::build(&pc_in);

        pc.set_in(next_pc.next_pc);
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
fn cpu_v1_build() {
    let mut state = CpuV1State::create();
    CpuV1Instance::build(&mut state);
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
