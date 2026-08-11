use super::{BasicAdder, BasicAdderInput};
use crate::{Module, ModuleIo};
use digital_design_code::{
    add_naive, input_const, input_w_const, mux2_w, reg, reg_w, CircuitWires, Wire, Wires,
};

#[derive(Clone, ModuleIo)]
pub struct BasicAdderBoardInput {
    /// Bit 0 resets; bit 1 enables the slow counter.
    pub buttons: Wires<2>,
}

#[derive(Clone, ModuleIo)]
pub struct BasicAdderBoardOutput {
    /// Logical LED-on values. Board binding handles electrical polarity.
    pub leds: Wires<6>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BasicAdderBoardState {
    reset_sync: [bool; 2],
    enable_sync: [bool; 2],
    divider: u32,
    tick: bool,
    count: u8,
}

/// Testable board-facing logic. `DIVISOR=6_750_000` produces a 4 Hz enable
/// pulse from the Tang Nano 20K's 27 MHz oscillator.
pub struct BasicAdderBoard<const DIVISOR: u32>;

impl<const DIVISOR: u32> Module for BasicAdderBoard<DIVISOR> {
    type Input = BasicAdderBoardInput;
    type Output = BasicAdderBoardOutput;
    type EmuState = BasicAdderBoardState;

    const USES_MAIN_CLOCK: bool = true;

    fn verilog_name() -> String {
        format!("BasicAdderBoard{DIVISOR}")
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        assert_divisor::<DIVISOR>();
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
            &BasicAdderBoardOutputValue {
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
        } else if state.enable_sync[1] && state.tick {
            state.count = state.count.wrapping_add(1) & 0x3f;
        }

        let terminal = state.divider == DIVISOR - 1;
        state.divider = if terminal { 0 } else { state.divider + 1 };
        state.tick = terminal;

        state.reset_sync = [input.buttons.wires[0].is_one(circuit), state.reset_sync[0]];
        state.enable_sync = [input.buttons.wires[1].is_one(circuit), state.enable_sync[0]];
    }

    fn nand(input: &Self::Input) -> Self::Output {
        build_board::<DIVISOR>(input, BasicAdder::nand)
    }

    fn build_verilog(input: &Self::Input) -> Self::Output {
        build_board::<DIVISOR>(input, BasicAdder::verilog)
    }
}

fn assert_divisor<const DIVISOR: u32>() {
    assert!(DIVISOR > 0, "board clock divisor must be non-zero");
    assert!(
        DIVISOR <= 1 << 23,
        "board clock divisor {DIVISOR} exceeds the 23-bit counter"
    );
}

fn build_board<const DIVISOR: u32>(
    input: &BasicAdderBoardInput,
    basic_adder: fn(&BasicAdderInput) -> super::BasicAdderOutput,
) -> BasicAdderBoardOutput {
    assert_divisor::<DIVISOR>();

    let reset_sync = reg_w::<2>();
    reset_sync.set_in(Wires {
        wires: [input.buttons.wires[0], reset_sync.out.wires[0]],
    });
    let enable_sync = reg_w::<2>();
    enable_sync.set_in(Wires {
        wires: [input.buttons.wires[1], enable_sync.out.wires[0]],
    });

    let divider = reg_w::<23>();
    let incremented = add_naive(divider.out, constant_wires::<23>(1)).sum;
    let terminal = wires_equal_constant(divider.out, u64::from(DIVISOR - 1));
    divider.set_in(mux2_w(incremented, input_w_const(0), terminal));

    let tick = reg();
    tick.set_in(terminal);
    let count = basic_adder(&BasicAdderInput {
        reset: reset_sync.out.wires[1],
        enable: enable_sync.out.wires[1] & tick.out(),
    })
    .count;
    BasicAdderBoardOutput { leds: count }
}

fn constant_wires<const WIDTH: usize>(value: u64) -> Wires<WIDTH> {
    Wires {
        wires: std::array::from_fn(|bit| input_const(((value >> bit) & 1) as u8)),
    }
}

fn wires_equal_constant<const WIDTH: usize>(wires: Wires<WIDTH>, value: u64) -> Wire {
    wires
        .wires
        .iter()
        .enumerate()
        .fold(input_const(1), |equal, (bit, &wire)| {
            equal & wire.eq_const(((value >> bit) & 1) as u8)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModuleTest, TestStep};

    #[test]
    fn synchronized_divided_board_logic_matches_emu_and_nand() {
        let mut model = BasicAdderBoardState::default();
        let buttons: [u64; 14] = [0, 2, 2, 2, 2, 2, 3, 0, 0, 0, 0, 0, 0, 0];
        let steps = buttons.into_iter().map(|buttons| {
            if model.reset_sync[1] {
                model.count = 0;
            } else if model.enable_sync[1] && model.tick {
                model.count = model.count.wrapping_add(1) & 0x3f;
            }
            let terminal = model.divider == 2;
            model.divider = if terminal { 0 } else { model.divider + 1 };
            model.tick = terminal;
            model.reset_sync = [buttons & 1 != 0, model.reset_sync[0]];
            model.enable_sync = [buttons & 2 != 0, model.enable_sync[0]];
            TestStep::new(
                BasicAdderBoardInputValue { buttons },
                BasicAdderBoardOutputValue {
                    leds: u64::from(model.count),
                },
            )
        });
        ModuleTest::<BasicAdderBoard<3>>::new(steps).run_emu_and_nand();
    }
}
