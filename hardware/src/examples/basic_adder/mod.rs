mod board;
mod increment;

pub use board::*;
pub use increment::*;

use crate::resources::components::{Clock27M, UserButtons, UserLeds};
use crate::{GowinProject, Module, ModuleIo, TangNano20K};
use digital_design_code::{input_w_const, mux2_w, reg_w, CircuitWires, Wire, Wires};

#[derive(Clone, ModuleIo)]
pub struct BasicAdderInput {
    pub reset: Wire,
    pub enable: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct BasicAdderOutput {
    pub count: Wires<6>,
}

pub struct BasicAdder;

pub type BasicAdderTarget = TangNano20K;
pub type BasicAdderTangNano20K = BasicAdderBoard<6_750_000>;

pub fn basic_adder_gowin_project() -> GowinProject<BasicAdderTarget> {
    let mut project = GowinProject::new("basic_adder", "tang_nano_20k_top");
    let clock = project
        .take_named("main-clock", Clock27M)
        .expect("Tang Nano 20K has the example clock");
    let buttons = project
        .take_named("buttons", UserButtons::<2>)
        .expect("Tang Nano 20K has two user buttons");
    let leds = project
        .take_named("leds", UserLeds::<6>)
        .expect("Tang Nano 20K has six user LEDs");
    let binding = TangNano20K::bind_user_io(clock, buttons, leds, "buttons", "leds");
    project.with_board_binding(binding)
}

impl Module for BasicAdder {
    type Input = BasicAdderInput;
    type Output = BasicAdderOutput;
    type EmuState = u8;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        0
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &BasicAdderOutputValue {
                count: u64::from(*state),
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        if input.reset.is_one(circuit) {
            *state = 0;
        } else if input.enable.is_one(circuit) {
            *state = state.wrapping_add(1) & 0x3f;
        }
    }

    fn nand(input: &Self::Input) -> Self::Output {
        build_counter(input, Increment6::nand)
    }

    fn build_verilog(input: &Self::Input) -> Self::Output {
        build_counter(input, Increment6::verilog)
    }
}

fn build_counter(
    input: &BasicAdderInput,
    increment: fn(&Increment6Input) -> Increment6Output,
) -> BasicAdderOutput {
    let state = reg_w::<6>();
    let incremented = increment(&Increment6Input { value: state.out }).incremented;
    let enabled = mux2_w(state.out, incremented, input.enable);
    let next = mux2_w(enabled, input_w_const(0), input.reset);
    state.set_in(next);
    BasicAdderOutput { count: state.out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModuleTest, TestStep, VerilogProject};
    use std::path::Path;

    #[test]
    fn emu_and_nand_follow_the_same_cycles() {
        let mut expected = 0u64;
        let mut steps = Vec::new();
        for cycle in 0..70 {
            let reset = cycle == 0 || cycle == 35;
            let enable = cycle % 4 != 0;
            if reset {
                expected = 0;
            } else if enable {
                expected = (expected + 1) & 0x3f;
            }
            steps.push(TestStep::new(
                BasicAdderInputValue { reset, enable },
                BasicAdderOutputValue { count: expected },
            ));
        }
        ModuleTest::<BasicAdder>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn project_keeps_the_increment_module_boundary() {
        let project = VerilogProject::generate::<BasicAdder>().unwrap();
        assert_eq!(project.top_module, "BasicAdder");
        assert!(project
            .files
            .contains_key(Path::new("examples/basic_adder.v")));
        assert!(project
            .files
            .contains_key(Path::new("examples/basic_adder/increment.v")));
        let top = &project.files[Path::new("examples/basic_adder.v")];
        assert!(top.contains("Increment6 u_increment6_0"));
        assert!(top.contains("input wire clk"));
    }
}
