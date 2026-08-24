//! Streaming boot copy engine between a byte-oriented Flash reader and physical memory.

mod mmio;
pub use mmio::*;

use crate::{HardwareIdentity, Module, ModuleIo, VerilogIdentity};
use digital_design_code::{CircuitWires, Wire, Wires};

pub const BOOT_DMA_ERROR_NONE: u8 = 0;
pub const BOOT_DMA_ERROR_FILE_LARGER_THAN_MEMORY: u8 = 1;
pub const BOOT_DMA_ERROR_FLASH_RANGE: u8 = 2;
pub const BOOT_DMA_ERROR_MEMORY_RANGE: u8 = 3;
pub const BOOT_DMA_ERROR_FLASH_IO: u8 = 4;
pub const BOOT_DMA_ERROR_MEMORY_IO: u8 = 5;

/// One DMA command plus the two downstream ready/valid response channels.
#[derive(Clone, ModuleIo)]
pub struct BootDmaEngineInput {
    pub reset: Wire,
    pub start: Wire,
    pub flash_offset: Wires<24>,
    pub destination: Wires<22>,
    pub file_size_bytes: Wires<32>,
    pub memory_size_bytes: Wires<32>,
    pub flash_ready: Wire,
    pub flash_data_valid: Wire,
    pub flash_data: Wires<8>,
    pub flash_done: Wire,
    pub flash_error: Wire,
    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct BootDmaEngineOutput {
    pub busy: Wire,
    pub done: Wire,
    pub error: Wire,
    pub error_code: Wires<8>,
    pub completed_words: Wires<32>,
    pub flash_start: Wire,
    pub flash_address: Wires<24>,
    pub flash_length: Wires<24>,
    pub flash_data_ready: Wire,
    pub memory_request_valid: Wire,
    pub memory_write: Wire,
    pub memory_address: Wires<22>,
    pub memory_write_data: Wires<16>,
    pub memory_response_ready: Wire,
}

pub struct BootDmaEngine;

impl HardwareIdentity for BootDmaEngine {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("BootDmaEngine").namespace(["components", "boot"])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Status {
    Idle,
    Busy,
    Done,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    WaitFlash,
    Stream,
    RequestMemory,
    WaitMemory,
}

pub struct BootDmaEngineState {
    status: Status,
    phase: Phase,
    flash_offset: u32,
    destination: u32,
    file_size_bytes: u32,
    memory_words: u32,
    byte_index: u32,
    word_index: u32,
    low_byte: u8,
    write_data: u16,
    error_code: u8,
}

impl Default for BootDmaEngineState {
    fn default() -> Self {
        Self {
            status: Status::Idle,
            phase: Phase::Stream,
            flash_offset: 0,
            destination: 0,
            file_size_bytes: 0,
            memory_words: 0,
            byte_index: 0,
            word_index: 0,
            low_byte: 0,
            write_data: 0,
            error_code: BOOT_DMA_ERROR_NONE,
        }
    }
}

impl BootDmaEngineState {
    fn fail(&mut self, error_code: u8) {
        self.status = Status::Error;
        self.error_code = error_code;
    }

    fn finish(&mut self) {
        self.status = Status::Done;
        self.error_code = BOOT_DMA_ERROR_NONE;
    }
}

impl Module for BootDmaEngine {
    type Input = BootDmaEngineInput;
    type Output = BootDmaEngineOutput;
    type EmuState = BootDmaEngineState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        BootDmaEngineState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        let busy = state.status == Status::Busy;
        output.drive(
            circuit,
            &BootDmaEngineOutputValue {
                busy,
                done: state.status == Status::Done,
                error: state.status == Status::Error,
                error_code: u64::from(state.error_code),
                completed_words: u64::from(state.word_index),
                flash_start: busy && state.phase == Phase::WaitFlash,
                flash_address: u64::from(state.flash_offset),
                flash_length: u64::from(state.file_size_bytes & 0x00ff_ffff),
                flash_data_ready: busy
                    && state.phase == Phase::Stream
                    && state.byte_index < state.file_size_bytes,
                memory_request_valid: busy && state.phase == Phase::RequestMemory,
                memory_write: true,
                memory_address: u64::from(state.destination + state.word_index),
                memory_write_data: u64::from(state.write_data),
                memory_response_ready: busy && state.phase == Phase::WaitMemory,
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
            *state = BootDmaEngineState::default();
            return;
        }

        if state.status != Status::Busy {
            if input.start {
                let file_size = input.file_size_bytes as u32;
                let memory_size = input.memory_size_bytes as u32;
                let memory_words = memory_size.div_ceil(2);
                let flash_end = input.flash_offset + u64::from(file_size);
                let memory_end = input.destination + u64::from(memory_words);
                *state = BootDmaEngineState {
                    status: Status::Busy,
                    phase: if file_size == 0 {
                        Phase::Stream
                    } else {
                        Phase::WaitFlash
                    },
                    flash_offset: input.flash_offset as u32,
                    destination: input.destination as u32,
                    file_size_bytes: file_size,
                    memory_words,
                    ..BootDmaEngineState::default()
                };
                if file_size > memory_size {
                    state.fail(BOOT_DMA_ERROR_FILE_LARGER_THAN_MEMORY);
                } else if flash_end > 1 << 24 {
                    state.fail(BOOT_DMA_ERROR_FLASH_RANGE);
                } else if memory_end > 1 << 22 {
                    state.fail(BOOT_DMA_ERROR_MEMORY_RANGE);
                } else if memory_words == 0 {
                    state.finish();
                }
            }
            return;
        }

        if input.flash_error {
            state.fail(BOOT_DMA_ERROR_FLASH_IO);
            return;
        }
        if input.memory_error {
            state.fail(BOOT_DMA_ERROR_MEMORY_IO);
            return;
        }

        match state.phase {
            Phase::WaitFlash => {
                if input.flash_ready {
                    state.phase = Phase::Stream;
                }
            }
            Phase::Stream => {
                if state.byte_index < state.file_size_bytes {
                    if input.flash_data_valid {
                        let byte = input.flash_data as u8;
                        if state.byte_index & 1 == 0 {
                            state.low_byte = byte;
                            state.byte_index += 1;
                            if state.byte_index == state.file_size_bytes {
                                state.write_data = u16::from(byte);
                                state.phase = Phase::RequestMemory;
                            }
                        } else {
                            state.write_data = u16::from(state.low_byte) | (u16::from(byte) << 8);
                            state.byte_index += 1;
                            state.phase = Phase::RequestMemory;
                        }
                    }
                } else {
                    state.write_data = 0;
                    state.phase = Phase::RequestMemory;
                }
            }
            Phase::RequestMemory => {
                if input.memory_request_ready {
                    state.phase = Phase::WaitMemory;
                }
            }
            Phase::WaitMemory => {
                if input.memory_response_valid {
                    state.word_index += 1;
                    if state.word_index == state.memory_words {
                        state.finish();
                    } else {
                        state.phase = Phase::Stream;
                    }
                }
            }
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("boot_dma.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("boot_dma_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_code::{build_circuit, Circuit};

    fn drive(
        circuit: &mut Circuit,
        input: &BootDmaEngineInput,
        start: bool,
        flash_data_valid: bool,
        flash_data: u8,
        memory_request_ready: bool,
        memory_response_valid: bool,
    ) {
        input.drive(
            circuit,
            &BootDmaEngineInputValue {
                reset: false,
                start,
                flash_offset: 0x100,
                destination: 0x10_0007,
                file_size_bytes: 3,
                memory_size_bytes: 6,
                flash_ready: true,
                flash_data_valid,
                flash_data: u64::from(flash_data),
                flash_done: false,
                flash_error: false,
                memory_request_ready,
                memory_response_valid,
                memory_error: false,
            },
        );
    }

    #[test]
    fn emulator_copies_little_endian_words_and_zero_fills() {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = BootDmaEngineInput::allocate();
            let output = BootDmaEngine::emu(&input);
            (input, output)
        });
        drive(&mut circuit, &input, true, false, 0, false, false);
        circuit.clock_tick();

        let bytes = [0x11, 0x22, 0x33];
        let mut next_byte = 0;
        let mut writes = Vec::new();
        for _ in 0..80 {
            circuit.execute_gates();
            let value = output.sample(&circuit);
            let send_byte = value.flash_data_ready && next_byte < bytes.len();
            let accept_request = value.memory_request_valid;
            let response = value.memory_response_ready;
            if accept_request {
                writes.push((value.memory_address as u32, value.memory_write_data as u16));
            }
            drive(
                &mut circuit,
                &input,
                false,
                send_byte,
                bytes.get(next_byte).copied().unwrap_or(0),
                accept_request,
                response,
            );
            if send_byte {
                next_byte += 1;
            }
            circuit.clock_tick();
            circuit.execute_gates();
            if output.sample(&circuit).done {
                break;
            }
        }
        assert_eq!(
            writes,
            [(0x10_0007, 0x2211), (0x10_0008, 0x0033), (0x10_0009, 0)]
        );
        let result = output.sample(&circuit);
        assert!(result.done && !result.error);
        assert_eq!(result.completed_words, 3);
    }

    #[test]
    fn emulator_rejects_invalid_extents_before_starting_io() {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = BootDmaEngineInput::allocate();
            let output = BootDmaEngine::emu(&input);
            (input, output)
        });
        input.drive(
            &mut circuit,
            &BootDmaEngineInputValue {
                reset: false,
                start: true,
                flash_offset: 0,
                destination: 0,
                file_size_bytes: 3,
                memory_size_bytes: 2,
                flash_ready: true,
                flash_data_valid: false,
                flash_data: 0,
                flash_done: false,
                flash_error: false,
                memory_request_ready: true,
                memory_response_valid: false,
                memory_error: false,
            },
        );
        circuit.clock_tick();
        circuit.execute_gates();
        let result = output.sample(&circuit);
        assert!(result.error && !result.busy);
        assert_eq!(
            result.error_code,
            u64::from(BOOT_DMA_ERROR_FILE_LARGER_THAN_MEMORY)
        );
        assert!(!result.flash_start && !result.memory_request_valid);
    }

    #[test]
    #[ignore = "explicit external simulation of the streaming boot DMA protocol"]
    fn verify_verilog_with_iverilog() {
        crate::verify_verilog_with_iverilog::<BootDmaEngine>().unwrap();
    }
}
