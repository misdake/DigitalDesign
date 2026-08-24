//! Passive boot-phase observation for the fitted CPU V3 system.

use crate::{Hardware, Module, ModuleIo};
use digital_design_circuit::{CircuitWires, Wire, Wires};

pub const BOOT_PHASE_RESET: u8 = 0;
pub const BOOT_PHASE_WAIT_SDRAM: u8 = 1;
pub const BOOT_PHASE_STAGE0: u8 = 2;
pub const BOOT_PHASE_DMA: u8 = 3;
pub const BOOT_PHASE_STAGE1: u8 = 4;
pub const BOOT_PHASE_APPLICATION: u8 = 5;
pub const BOOT_PHASE_ERROR: u8 = 7;

#[derive(Clone, ModuleIo)]
pub struct BootProgressMonitorInput {
    pub reset: Wire,
    pub sdram_ready: Wire,
    pub dma_busy: Wire,
    pub dma_error: Wire,
    pub cpu_fault: Wire,
    pub code_segment: Wires<16>,
    /// Pulses when software writes the system-control LED channel.
    pub software_led_write: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct BootProgressMonitorOutput {
    /// True until software has deliberately taken ownership of the LEDs.
    pub diagnostic_active: Wire,
    pub diagnostic_leds: Wires<6>,
    pub phase: Wires<3>,
    pub error_sticky: Wire,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BootProgressMonitorState {
    software_leds_seen: bool,
    error_sticky: bool,
}

impl BootProgressMonitorState {
    pub fn advance(
        &mut self,
        reset: bool,
        software_led_write: bool,
        dma_error: bool,
        cpu_fault: bool,
    ) {
        if reset {
            *self = Self::default();
            return;
        }
        self.software_leds_seen |= software_led_write;
        self.error_sticky |= dma_error || cpu_fault;
    }

    fn phase(
        &self,
        reset: bool,
        sdram_ready: bool,
        dma_busy: bool,
        dma_error: bool,
        cpu_fault: bool,
        code_segment: u16,
    ) -> u8 {
        if reset {
            BOOT_PHASE_RESET
        } else if self.error_sticky || dma_error || cpu_fault {
            BOOT_PHASE_ERROR
        } else if !sdram_ready {
            BOOT_PHASE_WAIT_SDRAM
        } else if dma_busy {
            BOOT_PHASE_DMA
        } else if code_segment == 0 {
            BOOT_PHASE_STAGE0
        } else if code_segment == 1 {
            BOOT_PHASE_STAGE1
        } else {
            BOOT_PHASE_APPLICATION
        }
    }
}

/// Observes boot progress without controlling the processor, DMA, or memory.
///
/// The monitor drives a diagnostic LED pattern only until software performs
/// its first system-control LED write. It never generates success and cannot
/// make a failed boot appear complete.
#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/diagnostics")]
pub struct BootProgressMonitor;

impl Module for BootProgressMonitor {
    type Input = BootProgressMonitorInput;
    type Output = BootProgressMonitorOutput;
    type EmuState = BootProgressMonitorState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        BootProgressMonitorState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let phase = state.phase(
            input.reset,
            input.sdram_ready,
            input.dma_busy,
            input.dma_error,
            input.cpu_fault,
            input.code_segment as u16,
        );
        output.drive(
            circuit,
            &BootProgressMonitorOutputValue {
                diagnostic_active: !state.software_leds_seen,
                diagnostic_leds: u64::from(phase_leds(phase)),
                phase: u64::from(phase),
                error_sticky: state.error_sticky,
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
        state.advance(
            input.reset,
            input.software_led_write,
            input.dma_error,
            input.cpu_fault,
        );
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("boot_progress.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("boot_progress_tb.v").to_string())
    }
}

fn phase_leds(phase: u8) -> u8 {
    match phase {
        BOOT_PHASE_RESET => 0b00_0001,
        BOOT_PHASE_WAIT_SDRAM => 0b00_0010,
        BOOT_PHASE_STAGE0 => 0b00_0100,
        BOOT_PHASE_DMA => 0b00_1000,
        BOOT_PHASE_STAGE1 => 0b01_0000,
        BOOT_PHASE_APPLICATION => 0b10_0000,
        _ => 0b10_0001,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_tracks_errors_and_irrevocably_hands_leds_to_software() {
        let mut state = BootProgressMonitorState::default();
        assert_eq!(state.phase(false, false, false, false, false, 0), 1);
        assert_eq!(state.phase(false, true, false, false, false, 0), 2);
        assert_eq!(state.phase(false, true, true, false, false, 0), 3);
        assert_eq!(state.phase(false, true, false, false, false, 1), 4);
        assert_eq!(state.phase(false, true, false, false, false, 3), 5);

        state.advance(false, true, false, false);
        assert!(state.software_leds_seen);
        state.advance(false, false, true, false);
        assert!(state.error_sticky);
        assert_eq!(state.phase(false, true, false, false, false, 3), 7);

        state.advance(true, false, false, false);
        assert!(!state.software_leds_seen);
        assert!(!state.error_sticky);
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn verify_verilog_with_iverilog() {
        crate::verify_verilog_with_iverilog::<BootProgressMonitor>().unwrap();
    }
}
