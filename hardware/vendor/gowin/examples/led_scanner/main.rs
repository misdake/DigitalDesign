use digital_design_circuit::{input_const, mux2, mux2_w, reg, reg_w, CircuitWires, Wire, Wires};
use digital_design_hardware_gowin::{
    run_gowin_project_cli, ClockDivider, ClockDividerInput, ClockDividerOutput, ClockDividerState,
    GowinCliError, GowinModuleProject, Hardware, Module, ModuleIo, TangNano20K, TangNano20KInputs,
    TangNano20KOutputs, TangNano20KOutputsValue,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(led_scanner_gowin_project(), "target/led_scanner_gowin")
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LedScannerState<const DIVISOR: u64> {
    reset_sync: [bool; 2],
    pause_sync: [bool; 2],
    divider: ClockDividerState<DIVISOR, 23>,
    position: u8,
    reverse: bool,
}

/// Six-LED bouncing scanner with synchronized reset/pause buttons.
#[derive(Hardware)]
#[hardware(namespace = "examples/led_scanner")]
pub struct LedScanner<const DIVISOR: u64>;

pub type LedScannerTangNano20K = LedScanner<6_750_000>;

pub fn led_scanner_gowin_project() -> GowinModuleProject<TangNano20K, LedScannerTangNano20K> {
    TangNano20K::user_io_project::<LedScannerTangNano20K>("led_scanner")
}

impl<const DIVISOR: u64> Module for LedScanner<DIVISOR> {
    type Input = TangNano20KInputs;
    type Output = TangNano20KOutputs;
    type EmuState = LedScannerState<DIVISOR>;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        LedScannerState::default()
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
                leds: 1 << state.position,
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        advance_scanner(state);

        state.divider.advance();
        state.reset_sync = [input.buttons.wires[0].is_one(circuit), state.reset_sync[0]];
        state.pause_sync = [input.buttons.wires[1].is_one(circuit), state.pause_sync[0]];
    }

    fn nand(input: &Self::Input) -> Self::Output {
        build_scanner::<DIVISOR>(input, ClockDivider::<DIVISOR, 23>::nand)
    }

    fn build_verilog(input: &Self::Input) -> Self::Output {
        build_scanner::<DIVISOR>(input, ClockDivider::<DIVISOR, 23>::verilog)
    }
}

fn build_scanner<const DIVISOR: u64>(
    input: &TangNano20KInputs,
    clock_divider: fn(&ClockDividerInput) -> ClockDividerOutput,
) -> TangNano20KOutputs {
    let reset_sync = reg_w::<2>();
    reset_sync.set_in(Wires {
        wires: [input.buttons.wires[0], reset_sync.out.wires[0]],
    });
    let pause_sync = reg_w::<2>();
    pause_sync.set_in(Wires {
        wires: [input.buttons.wires[1], pause_sync.out.wires[0]],
    });

    let tick = clock_divider(&ClockDividerInput {}).tick;

    let position = reg_w::<6>();
    let reverse = reg();
    let forward_shift = Wires {
        wires: [
            input_const(0),
            position.out.wires[0],
            position.out.wires[1],
            position.out.wires[2],
            position.out.wires[3],
            position.out.wires[4],
        ],
    };
    let reverse_shift = Wires {
        wires: [
            position.out.wires[1],
            position.out.wires[2],
            position.out.wires[3],
            position.out.wires[4],
            position.out.wires[5],
            input_const(0),
        ],
    };
    let forward_step = mux2_w(
        forward_shift,
        constant_wires::<6>(1 << 4),
        position.out.wires[5],
    );
    let reverse_step = mux2_w(
        reverse_shift,
        constant_wires::<6>(1 << 1),
        position.out.wires[0],
    );
    let selected_step = mux2_w(forward_step, reverse_step, reverse.out());
    let step = tick & !pause_sync.out.wires[1];
    let held_or_step = mux2_w(position.out, selected_step, step);
    let invalid = wires_all_zero(position.out);
    let initialize = reset_sync.out.wires[1] | invalid;
    position.set_in(mux2_w(held_or_step, constant_wires::<6>(1), initialize));

    let reach_high = step & !reverse.out() & position.out.wires[5];
    let reach_low = step & reverse.out() & position.out.wires[0];
    let next_reverse = (reverse.out() | reach_high) & !reach_low;
    reverse.set_in(mux2(next_reverse, input_const(0), initialize));

    TangNano20KOutputs { leds: position.out }
}

fn advance_scanner<const DIVISOR: u64>(state: &mut LedScannerState<DIVISOR>) {
    if state.reset_sync[1] {
        state.position = 0;
        state.reverse = false;
    } else if state.divider.tick() && !state.pause_sync[1] {
        if state.reverse {
            if state.position == 0 {
                state.position = 1;
                state.reverse = false;
            } else {
                state.position -= 1;
            }
        } else if state.position == 5 {
            state.position = 4;
            state.reverse = true;
        } else {
            state.position += 1;
        }
    }
}

fn constant_wires<const WIDTH: usize>(value: u64) -> Wires<WIDTH> {
    Wires {
        wires: std::array::from_fn(|bit| input_const(((value >> bit) & 1) as u8)),
    }
}

fn wires_all_zero<const WIDTH: usize>(wires: Wires<WIDTH>) -> Wire {
    wires
        .wires
        .iter()
        .fold(input_const(1), |all_zero, &wire| all_zero & !wire)
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware_gowin::{ModuleTest, TestStep};
    use std::path::Path;

    #[test]
    fn scanner_bounces_pauses_and_resets_in_emu_and_nand() {
        let buttons: [u64; 44] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 2, 2, 2, 2, 2, 0,
            0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        struct ReferenceScanner {
            reset_sync: [bool; 2],
            pause_sync: [bool; 2],
            divider_counter: u8,
            tick: bool,
            position: u8,
            reverse: bool,
        }

        impl ReferenceScanner {
            fn cycle(&mut self, buttons: u64) -> u64 {
                if self.reset_sync[1] {
                    self.position = 0;
                    self.reverse = false;
                } else if self.tick && !self.pause_sync[1] {
                    match (self.reverse, self.position) {
                        (false, 5) => {
                            self.position = 4;
                            self.reverse = true;
                        }
                        (true, 0) => {
                            self.position = 1;
                            self.reverse = false;
                        }
                        (false, _) => self.position += 1,
                        (true, _) => self.position -= 1,
                    }
                }
                self.tick = self.divider_counter == 2;
                self.divider_counter = if self.tick {
                    0
                } else {
                    self.divider_counter + 1
                };
                self.reset_sync = [buttons & 1 != 0, self.reset_sync[0]];
                self.pause_sync = [buttons & 2 != 0, self.pause_sync[0]];
                1 << self.position
            }
        }

        let mut reference = ReferenceScanner {
            reset_sync: [false; 2],
            pause_sync: [false; 2],
            divider_counter: 0,
            tick: false,
            position: 0,
            reverse: false,
        };
        let steps = buttons.into_iter().map(|buttons| {
            let leds = reference.cycle(buttons);
            TestStep::new(
                digital_design_hardware::TangNano20KInputsValue { buttons },
                TangNano20KOutputsValue { leds },
            )
        });
        ModuleTest::<LedScanner<3>>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn gowin_export_keeps_the_clock_divider_boundary() {
        let project = led_scanner_gowin_project().generate().unwrap();
        assert!(project.files.contains_key(Path::new(
            "src/generated/components/timing/clock_divider/divisor6750000_width23.v"
        )));
        let scanner = &project.files
            [Path::new("src/generated/examples/led_scanner/led_scanner/divisor6750000.v")];
        assert!(scanner.contains("ClockDivider_DIVISOR6750000_WIDTH23 u_clock_divider_0"));
    }
}
