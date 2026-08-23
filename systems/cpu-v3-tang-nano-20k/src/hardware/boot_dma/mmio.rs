//! G16 device-2 register bank for [`super::BootDmaEngine`].

use crate::{HardwareIdentity, Module, ModuleIo, VerilogIdentity};
use digital_design_circuit::{CircuitWires, Wire, Wires};

const DMA_DEVICE: u8 = 2;

#[derive(Clone, ModuleIo)]
pub struct BootDmaMmioInput {
    pub reset: Wire,
    pub device_index: Wires<4>,
    pub device_channel: Wires<4>,
    pub device_read_enable: Wire,
    pub device_write_enable: Wire,
    pub device_write_data: Wires<16>,
    pub dma_busy: Wire,
    pub dma_done: Wire,
    pub dma_error: Wire,
    pub dma_error_code: Wires<8>,
    pub dma_completed_words: Wires<32>,
}

#[derive(Clone, ModuleIo)]
pub struct BootDmaMmioOutput {
    pub device_read_data: Wires<16>,
    pub dma_start: Wire,
    pub flash_offset: Wires<24>,
    pub destination: Wires<22>,
    pub file_size_bytes: Wires<32>,
    pub memory_size_bytes: Wires<32>,
}

pub struct BootDmaMmio;

impl HardwareIdentity for BootDmaMmio {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("BootDmaMmio").namespace(["components", "boot"])
    }
}

#[derive(Default)]
pub struct BootDmaMmioState {
    start: bool,
    flash_offset: u32,
    destination: u32,
    file_size_bytes: u32,
    memory_size_bytes: u32,
}

impl Module for BootDmaMmio {
    type Input = BootDmaMmioInput;
    type Output = BootDmaMmioOutput;
    type EmuState = BootDmaMmioState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        BootDmaMmioState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let selected = input.device_read_enable && input.device_index == u64::from(DMA_DEVICE);
        let read_data = if selected {
            match input.device_channel as u8 {
                0 => 0,
                1 if input.dma_error => 0x8000,
                1 if input.dma_done => 2,
                1 if input.dma_busy => 1,
                1 => 0,
                2 => state.flash_offset as u16,
                3 => (state.flash_offset >> 16) as u16,
                4 => state.destination as u16,
                5 => (state.destination >> 16) as u16,
                6 => state.file_size_bytes as u16,
                7 => (state.file_size_bytes >> 16) as u16,
                8 => state.memory_size_bytes as u16,
                9 => (state.memory_size_bytes >> 16) as u16,
                14 => input.dma_error_code as u16,
                15 => input.dma_completed_words as u16,
                _ => 0,
            }
        } else {
            0
        };
        output.drive(
            circuit,
            &BootDmaMmioOutputValue {
                device_read_data: u64::from(read_data),
                dma_start: state.start,
                flash_offset: u64::from(state.flash_offset & 0x00ff_ffff),
                destination: u64::from(state.destination & 0x003f_ffff),
                file_size_bytes: u64::from(state.file_size_bytes),
                memory_size_bytes: u64::from(state.memory_size_bytes),
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        if input.reset {
            *state = BootDmaMmioState::default();
            return;
        }
        state.start = false;
        if !input.device_write_enable || input.device_index != u64::from(DMA_DEVICE) {
            return;
        }
        let value = input.device_write_data as u16;
        match input.device_channel as u8 {
            0 if value == 1 || value == 2 => state.start = true,
            2 => state.flash_offset = (state.flash_offset & 0xffff_0000) | u32::from(value),
            3 => {
                state.flash_offset =
                    (state.flash_offset & 0x0000_ffff) | (u32::from(value & 0x00ff) << 16)
            }
            4 => state.destination = (state.destination & 0xffff_0000) | u32::from(value),
            5 => {
                state.destination =
                    (state.destination & 0x0000_ffff) | (u32::from(value & 0x003f) << 16)
            }
            6 => state.file_size_bytes = (state.file_size_bytes & 0xffff_0000) | u32::from(value),
            7 => {
                state.file_size_bytes =
                    (state.file_size_bytes & 0x0000_ffff) | (u32::from(value) << 16)
            }
            8 => {
                state.memory_size_bytes = (state.memory_size_bytes & 0xffff_0000) | u32::from(value)
            }
            9 => {
                state.memory_size_bytes =
                    (state.memory_size_bytes & 0x0000_ffff) | (u32::from(value) << 16)
            }
            _ => {}
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("boot_dma_mmio.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("boot_dma_mmio_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VerilogProject;
    use digital_design_circuit::{build_circuit, Circuit};

    fn drive(
        circuit: &mut Circuit,
        input: &BootDmaMmioInput,
        channel: u64,
        read: bool,
        write: bool,
        value: u64,
    ) {
        input.drive(
            circuit,
            &BootDmaMmioInputValue {
                reset: false,
                device_index: 2,
                device_channel: channel,
                device_read_enable: read,
                device_write_enable: write,
                device_write_data: value,
                dma_busy: false,
                dma_done: false,
                dma_error: false,
                dma_error_code: 0,
                dma_completed_words: 0,
            },
        );
    }

    #[test]
    fn emulator_latches_wide_fields_and_pulses_start() {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = BootDmaMmioInput::allocate();
            let output = BootDmaMmio::emu(&input);
            (input, output)
        });
        for (channel, value) in [(2, 0xbcde), (3, 0x007a), (4, 0x4567), (5, 0x0032)] {
            drive(&mut circuit, &input, channel, false, true, value);
            circuit.clock_tick();
        }
        drive(&mut circuit, &input, 0, false, true, 1);
        circuit.clock_tick();
        circuit.execute_gates();
        let value = output.sample(&circuit);
        assert!(value.dma_start);
        assert_eq!(value.flash_offset, 0x7a_bcde);
        assert_eq!(value.destination, 0x32_4567);
        drive(&mut circuit, &input, 0, false, false, 0);
        circuit.clock_tick();
        circuit.execute_gates();
        assert!(!output.sample(&circuit).dma_start);
    }

    #[test]
    fn emulator_exposes_status_and_diagnostics_only_on_device_two_reads() {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = BootDmaMmioInput::allocate();
            let output = BootDmaMmio::emu(&input);
            (input, output)
        });
        input.drive(
            &mut circuit,
            &BootDmaMmioInputValue {
                reset: false,
                device_index: 2,
                device_channel: 14,
                device_read_enable: true,
                device_write_enable: false,
                device_write_data: 0,
                dma_busy: false,
                dma_done: false,
                dma_error: true,
                dma_error_code: 3,
                dma_completed_words: 0x1234_5678,
            },
        );
        circuit.execute_gates();
        assert_eq!(output.sample(&circuit).device_read_data, 3);
        drive(&mut circuit, &input, 1, true, false, 0);
        input.drive(
            &mut circuit,
            &BootDmaMmioInputValue {
                reset: false,
                device_index: 2,
                device_channel: 1,
                device_read_enable: true,
                device_write_enable: false,
                device_write_data: 0,
                dma_busy: false,
                dma_done: false,
                dma_error: true,
                dma_error_code: 3,
                dma_completed_words: 0,
            },
        );
        circuit.execute_gates();
        assert_eq!(output.sample(&circuit).device_read_data, 0x8000);
    }

    #[test]
    fn numeric_channels_match_the_compiler_boot_abi() {
        assert_eq!(DMA_DEVICE, crate::boot::BOOT_DMA_DEVICE);
        assert_eq!(1, crate::boot::DMA_STATUS);
        assert_eq!(14, crate::boot::DMA_ERROR);
        assert_eq!(15, crate::boot::DMA_COMPLETED_WORDS_LOW);
        assert!(VerilogProject::generate::<BootDmaMmio>()
            .unwrap()
            .resource_claims
            .is_empty());
    }
}
