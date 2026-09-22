//! Small, gate-exportable helpers used by the multi-clock display RTL.

mod hdmi;
mod line_buffer;
mod sdram;

pub use hdmi::*;
pub use line_buffer::*;
pub use sdram::*;

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{Hardware, Module, ModuleIo};

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
