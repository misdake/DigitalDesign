use super::{BasicAdder, BasicAdderInput};
use digital_design_code::{reg_w, CircuitWires, Wires};
use digital_design_hardware::{
    ClockDivider, ClockDividerInput, ClockDividerOutput, ClockDividerState, Hardware, Module,
    ModuleIo, TangNano20KInputs, TangNano20KOutputs, TangNano20KOutputsValue,
};

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
    basic_adder: fn(&BasicAdderInput) -> super::BasicAdderOutput,
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

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ModuleTest, TangNano20KInputsValue, TestStep};

    #[test]
    fn synchronized_divided_board_logic_matches_emu_and_nand() {
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
}
