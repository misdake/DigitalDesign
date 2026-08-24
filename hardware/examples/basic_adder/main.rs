use digital_design_code::{add_naive, input_w_const, mux2_w, reg_w, CircuitWires, Wire, Wires};
use digital_design_hardware::{
    run_gowin_project_cli, ClockDivider, ClockDividerInput, ClockDividerOutput, ClockDividerState,
    GowinCliError, GowinModuleProject, Hardware, Module, ModuleIo, ModuleTest, TangNano20K,
    TangNano20KInputs, TangNano20KOutputs, TangNano20KOutputsValue, TestStep,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(basic_adder_gowin_project(), "target/basic_adder_gowin")
}

#[derive(Clone, ModuleIo)]
pub struct Increment6Input {
    pub value: Wires<6>,
}

#[derive(Clone, ModuleIo)]
pub struct Increment6Output {
    pub incremented: Wires<6>,
}

#[derive(Hardware)]
#[hardware(namespace = "examples/basic_adder")]
pub struct Increment6;

impl Module for Increment6 {
    type Input = Increment6Input;
    type Output = Increment6Output;
    type EmuState = ();

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

    fn execute_emu(
        _state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let value = input
            .value
            .wires
            .iter()
            .enumerate()
            .fold(0u8, |value, (bit, wire)| value | (wire.get(circuit) << bit));
        output.drive(
            circuit,
            &Increment6OutputValue {
                incremented: u64::from(value.wrapping_add(1) & 0x3f),
            },
        );
    }

    fn nand(input: &Self::Input) -> Self::Output {
        Self::Output {
            incremented: add_naive(input.value, Wires::<6>::parse_u8(1)).sum,
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("increment.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(increment_test().verilog_testbench())
    }
}

fn increment_test() -> ModuleTest<Increment6> {
    ModuleTest::new([
        TestStep::new(
            Increment6InputValue { value: 0 },
            Increment6OutputValue { incremented: 1 },
        ),
        TestStep::new(
            Increment6InputValue { value: 17 },
            Increment6OutputValue { incremented: 18 },
        ),
        TestStep::new(
            Increment6InputValue { value: 63 },
            Increment6OutputValue { incremented: 0 },
        ),
    ])
}

#[derive(Clone, ModuleIo)]
pub struct BasicAdderInput {
    pub reset: Wire,
    pub enable: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct BasicAdderOutput {
    pub count: Wires<6>,
}

#[derive(Hardware)]
#[hardware(namespace = "examples/basic_adder")]
pub struct BasicAdder;

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

#[derive(Clone, Copy, Debug, Default)]
pub struct BasicAdderBoardState<const DIVISOR: u64> {
    reset_sync: [bool; 2],
    enable_sync: [bool; 2],
    divider: ClockDividerState<DIVISOR, 23>,
    count: u8,
}

/// Testable board-facing logic. `DIVISOR=6_750_000` produces a 4 Hz enable
/// pulse from the Tang Nano 20K's 27 MHz oscillator.
#[derive(Hardware)]
#[hardware(namespace = "examples/basic_adder")]
pub struct BasicAdderBoard<const DIVISOR: u64>;

impl<const DIVISOR: u64> Module for BasicAdderBoard<DIVISOR> {
    type Input = TangNano20KInputs;
    type Output = TangNano20KOutputs;
    type EmuState = BasicAdderBoardState<DIVISOR>;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        BasicAdderBoardState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &TangNano20KOutputsValue {
                leds: u64::from(state.count),
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        if state.reset_sync[1] {
            state.count = 0;
        } else if state.enable_sync[1] && state.divider.tick() {
            state.count = state.count.wrapping_add(1) & 0x3f;
        }

        state.divider.advance();
        state.reset_sync = [input.buttons.wires[0].is_one(circuit), state.reset_sync[0]];
        state.enable_sync = [input.buttons.wires[1].is_one(circuit), state.enable_sync[0]];
    }

    fn nand(input: &Self::Input) -> Self::Output {
        build_board::<DIVISOR>(input, BasicAdder::nand, ClockDivider::<DIVISOR, 23>::nand)
    }

    fn build_verilog(input: &Self::Input) -> Self::Output {
        build_board::<DIVISOR>(
            input,
            BasicAdder::verilog,
            ClockDivider::<DIVISOR, 23>::verilog,
        )
    }
}

fn build_board<const DIVISOR: u64>(
    input: &TangNano20KInputs,
    basic_adder: fn(&BasicAdderInput) -> BasicAdderOutput,
    clock_divider: fn(&ClockDividerInput) -> ClockDividerOutput,
) -> TangNano20KOutputs {
    let reset_sync = reg_w::<2>();
    reset_sync.set_in(Wires {
        wires: [input.buttons.wires[0], reset_sync.out.wires[0]],
    });
    let enable_sync = reg_w::<2>();
    enable_sync.set_in(Wires {
        wires: [input.buttons.wires[1], enable_sync.out.wires[0]],
    });

    let tick = clock_divider(&ClockDividerInput {}).tick;
    let count = basic_adder(&BasicAdderInput {
        reset: reset_sync.out.wires[1],
        enable: enable_sync.out.wires[1] & tick,
    })
    .count;
    TangNano20KOutputs { leds: count }
}

pub type BasicAdderTangNano20K = BasicAdderBoard<6_750_000>;

pub fn basic_adder_gowin_project() -> GowinModuleProject<TangNano20K, BasicAdderTangNano20K> {
    TangNano20K::user_io_project::<BasicAdderTangNano20K>("basic_adder")
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ResourceKind, TangNano20KInputsValue};
    use std::path::Path;

    #[test]
    fn increment_vectors_drive_emu_nand_and_verilog() {
        increment_test().run_emu_and_nand();
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn verify_handwritten_verilog_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<Increment6>().unwrap();
    }

    #[test]
    fn counter_and_board_match_emu_and_nand() {
        let mut expected = 0u64;
        let steps = (0..70).map(|cycle| {
            let reset = cycle == 0 || cycle == 35;
            let enable = cycle % 4 != 0;
            if reset {
                expected = 0;
            } else if enable {
                expected = (expected + 1) & 0x3f;
            }
            TestStep::new(
                BasicAdderInputValue { reset, enable },
                BasicAdderOutputValue { count: expected },
            )
        });
        ModuleTest::<BasicAdder>::new(steps).run_emu_and_nand();

        let mut model = BasicAdderBoardState::<3>::default();
        let buttons: [u64; 14] = [0, 2, 2, 2, 2, 2, 3, 0, 0, 0, 0, 0, 0, 0];
        let steps = buttons.into_iter().map(|buttons| {
            if model.reset_sync[1] {
                model.count = 0;
            } else if model.enable_sync[1] && model.divider.tick() {
                model.count = model.count.wrapping_add(1) & 0x3f;
            }
            model.divider.advance();
            model.reset_sync = [buttons & 1 != 0, model.reset_sync[0]];
            model.enable_sync = [buttons & 2 != 0, model.enable_sync[0]];
            TestStep::new(
                TangNano20KInputsValue { buttons },
                TangNano20KOutputsValue {
                    leds: u64::from(model.count),
                },
            )
        });
        ModuleTest::<BasicAdderBoard<3>>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn tang_nano_project_is_complete() {
        let project = basic_adder_gowin_project().generate().unwrap();
        for path in [
            "build.tcl",
            "basic_adder.gprj",
            "resource-report.txt",
            "src/generated/board_top.v",
            "src/generated/board.cst",
            "src/generated/board.sdc",
            "src/generated/components/timing/clock_divider/divisor6750000_width23.v",
            "src/generated/examples/basic_adder/basic_adder_board/divisor6750000.v",
            "src/generated/examples/basic_adder/increment6.v",
        ] {
            assert!(
                project.files.contains_key(Path::new(path)),
                "missing {path}"
            );
        }

        let wrapper = &project.files[Path::new("src/generated/board_top.v")];
        assert!(wrapper.contains("BasicAdderBoard_DIVISOR6750000 u_logic"));
        assert!(wrapper.contains("assign leds[0] = ~bound_leds[0]"));
        let cst = &project.files[Path::new("src/generated/board.cst")];
        assert!(cst.contains("IO_LOC \"clk\" 4;"));
        assert!(cst.contains("IO_LOC \"buttons[0]\" 88;"));
        assert!(cst.contains("IO_LOC \"leds[5]\" 20;"));
        assert_eq!(project.resources.claimed[&ResourceKind::UserLed], 6);
        assert_eq!(project.resources.claimed[&ResourceKind::UserButton], 2);
    }
}
