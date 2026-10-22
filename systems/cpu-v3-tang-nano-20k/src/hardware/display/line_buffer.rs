//! Four RGB565 lines and packed color/TMDS tables in two dual-clock BSRAMs.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{BsramBlocks, Hardware, Module, ModuleIo, TargetResourceRequest};

#[path = "precode_table.rs"]
mod precode;
#[cfg(test)]
use precode::tmds_precode;
pub(super) use precode::TMDS_PRECODE_BITS;

#[cfg(test)]
fn table_word(index: usize) -> u32 {
    // G has six encoded bits; linear R/B use the same six-bit index.
    precode::table_word(index, &crate::display::LINEAR6_TO_SRGB8)
}

#[derive(Clone, ModuleIo)]
pub struct DisplayLineBufferInput {
    pub write_clock: Wire,
    pub write_enable: Wire,
    pub write_address: Wires<9>,
    pub write_data: Wires<64>,
    pub read_clock: Wire,
    pub read_address_a: Wires<9>,
    pub read_address_b: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct DisplayLineBufferOutput {
    pub read_data_a: Wires<32>,
    pub read_data_b: Wires<32>,
}

#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/display", target_leaf)]
pub struct DisplayLineBuffer;

impl Module for DisplayLineBuffer {
    type Input = DisplayLineBufferInput;
    type Output = DisplayLineBufferOutput;
    type EmuState = ();

    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(BsramBlocks::new(2))]
    }

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("dual-clock line buffer is Verilog-only")
    }

    fn verilog_source() -> Option<String> {
        let init = precode::initialization(&crate::display::LINEAR6_TO_SRGB8);
        Some(include_str!("display_line_buffer.v").replace("__SRGB_INIT__", &init))
    }

    fn verilog_testbench() -> Option<String> {
        Some(
            include_str!("display_line_buffer_tb.v")
                .replace("__TMDS_PRECODE_BITS__", &TMDS_PRECODE_BITS.to_string()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_tables_cover_every_rgb565_value_in_both_formats() {
        let decode = |code: u32| -> u8 {
            let qm = code & 255;
            let xnor = ((code >> 8) & 1) ^ 1;
            let mut byte = qm & 1;
            for bit in 1..8 {
                byte |= (((qm >> bit) ^ (qm >> (bit - 1)) ^ xnor) & 1) << bit;
            }
            byte as u8
        };
        for pixel in 0u32..=65535 {
            for linear in [false, true] {
                for (raw, width) in [(pixel >> 11, 5), ((pixel >> 5) & 63, 6), (pixel & 31, 5)] {
                    let index = if width == 5 {
                        (raw << 1) | (raw >> 4)
                    } else {
                        raw
                    };
                    let expected = if linear {
                        crate::display::LINEAR6_TO_SRGB8[index as usize]
                    } else if width == 5 {
                        ((raw << 3) | (raw >> 2)) as u8
                    } else {
                        ((raw << 2) | (raw >> 4)) as u8
                    };
                    let mask = (1u32 << TMDS_PRECODE_BITS) - 1;
                    let packed = if linear {
                        table_word(index as usize) & mask
                    } else if width == 6 {
                        table_word(raw as usize) >> TMDS_PRECODE_BITS
                    } else {
                        tmds_precode(((raw << 3) | (raw >> 2)) as u8)
                    };
                    assert_eq!(decode(packed), expected);
                    assert_eq!(packed, tmds_precode(expected));
                    if TMDS_PRECODE_BITS == 13 {
                        assert_eq!(packed >> 9, (packed & 255).count_ones());
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "explicit external simulation of the dual-clock inferred RAM"]
    fn dual_clock_ram_runs_in_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<DisplayLineBuffer>().unwrap();
    }
}
