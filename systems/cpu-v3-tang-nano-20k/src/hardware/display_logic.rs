//! Small, gate-exportable helpers used by the multi-clock display RTL.

use digital_design_circuit::{input_const, CircuitWires, Wire, Wires};
use digital_design_hardware::{Hardware, Module, ModuleIo};

#[derive(Clone, ModuleIo)]
pub struct DisplayGrantInput {
    pub refresh_due: Wire,
    pub display_valid: Wire,
    pub display_urgent: Wire,
    pub cpu_valid: Wire,
    pub prefer_display: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct DisplayGrantOutput {
    pub refresh_grant: Wire,
    pub display_grant: Wire,
    pub cpu_grant: Wire,
    /// Preference to capture after the selected transaction is accepted.
    pub next_prefer_display: Wire,
}

#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/display")]
pub struct DisplayGrant;

fn grant(
    refresh_due: bool,
    display_valid: bool,
    display_urgent: bool,
    cpu_valid: bool,
    prefer_display: bool,
) -> (bool, bool, bool, bool) {
    let display = !refresh_due && display_valid && (display_urgent || !cpu_valid || prefer_display);
    let cpu = !refresh_due && cpu_valid && !display;
    let next = if display && cpu_valid && !display_urgent {
        false
    } else if cpu && display_valid && !display_urgent {
        true
    } else {
        prefer_display
    };
    (refresh_due, display, cpu, next)
}

impl Module for DisplayGrant {
    type Input = DisplayGrantInput;
    type Output = DisplayGrantOutput;
    type EmuState = ();

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

    fn execute_emu(
        _state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let (refresh, display, cpu, next) = grant(
            input.refresh_due,
            input.display_valid,
            input.display_urgent,
            input.cpu_valid,
            input.prefer_display,
        );
        output.drive(
            circuit,
            &DisplayGrantOutputValue {
                refresh_grant: refresh,
                display_grant: display,
                cpu_grant: cpu,
                next_prefer_display: next,
            },
        );
    }

    fn nand(input: &Self::Input) -> Self::Output {
        let not_refresh = !input.refresh_due;
        let display_grant = not_refresh
            & input.display_valid
            & (input.display_urgent | !input.cpu_valid | input.prefer_display);
        let cpu_grant = not_refresh & input.cpu_valid & !display_grant;
        let normal = !input.display_urgent;
        let display_turn = display_grant & input.cpu_valid & normal;
        let cpu_turn = cpu_grant & input.display_valid & normal;
        let next_prefer_display = (display_turn & input_const(0))
            | (cpu_turn & input_const(1))
            | (!display_turn & !cpu_turn & input.prefer_display);
        DisplayGrantOutput {
            refresh_grant: input.refresh_due,
            display_grant,
            cpu_grant,
            next_prefer_display,
        }
    }
}

#[derive(Clone, ModuleIo)]
pub struct Rgb565Input {
    pub pixel: Wires<16>,
    pub visible: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct Rgb888Output {
    pub red: Wires<8>,
    pub green: Wires<8>,
    pub blue: Wires<8>,
}

#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/display")]
pub struct Rgb565ToRgb888;

pub const fn rgb565_to_rgb888(pixel: u16, visible: bool) -> (u8, u8, u8) {
    if !visible {
        return (0, 0, 0);
    }
    let red = ((pixel >> 11) & 0x1f) as u8;
    let green = ((pixel >> 5) & 0x3f) as u8;
    let blue = (pixel & 0x1f) as u8;
    (
        (red << 3) | (red >> 2),
        (green << 2) | (green >> 4),
        (blue << 3) | (blue >> 2),
    )
}

impl Module for Rgb565ToRgb888 {
    type Input = Rgb565Input;
    type Output = Rgb888Output;
    type EmuState = ();

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

    fn execute_emu(
        _state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let (red, green, blue) = rgb565_to_rgb888(input.pixel as u16, input.visible);
        output.drive(
            circuit,
            &Rgb888OutputValue {
                red: u64::from(red),
                green: u64::from(green),
                blue: u64::from(blue),
            },
        );
    }

    fn nand(input: &Self::Input) -> Self::Output {
        let bit = |index: usize| input.pixel.wires[index] & input.visible;
        Rgb888Output {
            red: Wires {
                wires: [
                    bit(13),
                    bit(14),
                    bit(15),
                    bit(11),
                    bit(12),
                    bit(13),
                    bit(14),
                    bit(15),
                ],
            },
            green: Wires {
                wires: [
                    bit(9),
                    bit(10),
                    bit(5),
                    bit(6),
                    bit(7),
                    bit(8),
                    bit(9),
                    bit(10),
                ],
            },
            blue: Wires {
                wires: [
                    bit(2),
                    bit(3),
                    bit(4),
                    bit(0),
                    bit(1),
                    bit(2),
                    bit(3),
                    bit(4),
                ],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ModuleTest, TestStep};

    fn grant_step(
        input: [bool; 5],
        output: [bool; 4],
    ) -> TestStep<DisplayGrantInputValue, DisplayGrantOutputValue> {
        TestStep::new(
            DisplayGrantInputValue {
                refresh_due: input[0],
                display_valid: input[1],
                display_urgent: input[2],
                cpu_valid: input[3],
                prefer_display: input[4],
            },
            DisplayGrantOutputValue {
                refresh_grant: output[0],
                display_grant: output[1],
                cpu_grant: output[2],
                next_prefer_display: output[3],
            },
        )
    }

    #[test]
    fn refresh_urgent_and_normal_round_robin_match_in_emu_and_nand() {
        ModuleTest::<DisplayGrant>::new([
            grant_step([true, true, true, true, false], [true, false, false, false]),
            grant_step(
                [false, true, true, true, false],
                [false, true, false, false],
            ),
            grant_step(
                [false, true, false, true, true],
                [false, true, false, false],
            ),
            grant_step(
                [false, true, false, true, false],
                [false, false, true, true],
            ),
            grant_step(
                [false, true, false, false, false],
                [false, true, false, false],
            ),
        ])
        .run_emu_and_nand();
    }

    #[test]
    fn rgb_expansion_and_black_mux_match_in_emu_and_nand() {
        ModuleTest::<Rgb565ToRgb888>::new([
            TestStep::new(
                Rgb565InputValue {
                    pixel: 0xffff,
                    visible: true,
                },
                Rgb888OutputValue {
                    red: 255,
                    green: 255,
                    blue: 255,
                },
            ),
            TestStep::new(
                Rgb565InputValue {
                    pixel: 0xf800,
                    visible: true,
                },
                Rgb888OutputValue {
                    red: 255,
                    green: 0,
                    blue: 0,
                },
            ),
            TestStep::new(
                Rgb565InputValue {
                    pixel: 0x07e0,
                    visible: false,
                },
                Rgb888OutputValue {
                    red: 0,
                    green: 0,
                    blue: 0,
                },
            ),
        ])
        .run_emu_and_nand();
    }
}
