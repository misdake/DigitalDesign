use crate::{HardwareIdentity, Module, ModuleIo, VerilogIdentity};
use digital_design_circuit::{CircuitWires, Wire, Wires};

const PHYSICAL_WORDS: usize = 1 << 22;
const LINE_BEATS: usize = 8;

/// One physical 16-bit memory transaction above the fitted Controller HS.
/// A line read instead streams one eight-beat 32-bit burst.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KSdramWordPortInput {
    pub reset: Wire,
    pub request_valid: Wire,
    pub write: Wire,
    pub read_line: Wire,
    pub address: Wires<22>,
    pub write_data: Wires<16>,
    pub response_ready: Wire,
    pub controller_read_data: Wires<32>,
    pub controller_read_valid: Wire,
    pub controller_init_done: Wire,
    pub controller_command_ack: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct TangNano20KSdramWordPortOutput {
    pub request_ready: Wire,
    pub response_valid: Wire,
    pub read_data: Wires<32>,
    pub response_last: Wire,
    pub error: Wire,
    pub controller_command_valid: Wire,
    pub controller_command: Wires<3>,
    pub controller_precharge: Wire,
    pub controller_address: Wires<21>,
    pub controller_write_mask: Wires<4>,
    pub controller_write_data: Wires<32>,
    pub controller_burst_length: Wires<8>,
}

pub struct TangNano20KSdramWordPort;

impl HardwareIdentity for TangNano20KSdramWordPort {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("TangNano20KSdramWordPort").namespace([
            "target",
            "tang_nano_20k",
            "sdram",
        ])
    }
}

pub struct TangNano20KSdramWordPortState {
    memory: Box<[u16]>,
    pending: Option<Pending>,
    delay: u8,
    serving_line: bool,
    beat: u8,
    response_valid: bool,
    response_last: bool,
    read_data: u32,
}

#[derive(Clone, Copy)]
struct Pending {
    write: bool,
    line: bool,
    address: usize,
    write_data: u16,
}

impl TangNano20KSdramWordPortState {
    fn serving(&self) -> bool {
        self.serving_line
    }

    fn beat_data(&self) -> u32 {
        let base = self.pending.map(|pending| pending.address).unwrap_or(0);
        let beat = usize::from(self.beat);
        let low = self.memory[base + 2 * beat];
        let high = self.memory[base + 2 * beat + 1];
        u32::from(low) | u32::from(high) << 16
    }
}

impl Module for TangNano20KSdramWordPort {
    type Input = TangNano20KSdramWordPortInput;
    type Output = TangNano20KSdramWordPortOutput;
    type EmuState = TangNano20KSdramWordPortState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        TangNano20KSdramWordPortState {
            memory: vec![0; PHYSICAL_WORDS].into_boxed_slice(),
            pending: None,
            delay: 0,
            serving_line: false,
            beat: 0,
            response_valid: false,
            response_last: false,
            read_data: 0,
        }
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let streaming = state.serving();
        output.drive(
            circuit,
            &TangNano20KSdramWordPortOutputValue {
                request_ready: input.controller_init_done
                    && state.pending.is_none()
                    && !streaming
                    && !state.response_valid,
                response_valid: state.response_valid || streaming,
                read_data: u64::from(if streaming {
                    state.beat_data()
                } else {
                    state.read_data
                }),
                response_last: if streaming {
                    state.beat as usize + 1 == LINE_BEATS
                } else {
                    state.response_last
                },
                error: false,
                controller_command_valid: false,
                controller_command: 0b111,
                controller_precharge: false,
                controller_address: 0,
                controller_write_mask: 0,
                controller_write_data: 0,
                controller_burst_length: 0,
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
            state.pending = None;
            state.serving_line = false;
            state.response_valid = false;
            state.response_last = false;
            state.delay = 0;
            state.beat = 0;
            return;
        }
        if !input.controller_init_done {
            state.pending = None;
            state.serving_line = false;
            state.response_valid = false;
            state.response_last = false;
            state.delay = 0;
            state.beat = 0;
            return;
        }
        // Line beats stream one per cycle and cannot be stalled.
        if state.serving_line {
            if state.beat as usize + 1 == LINE_BEATS {
                state.serving_line = false;
                state.pending = None;
            } else {
                state.beat += 1;
            }
            return;
        }
        if state.response_valid {
            if input.response_ready {
                state.response_valid = false;
            }
            return;
        }
        if let Some(pending) = state.pending {
            if state.delay != 0 {
                state.delay -= 1;
                return;
            }
            if pending.write {
                state.memory[pending.address] = pending.write_data;
                state.read_data = 0;
                state.response_valid = true;
                state.response_last = true;
            } else if pending.line {
                state.serving_line = true;
                state.beat = 0;
            } else {
                state.read_data = u32::from(state.memory[pending.address]);
                state.response_valid = true;
                state.response_last = true;
            }
            if !pending.line {
                state.pending = None;
            }
            return;
        }
        if input.request_valid {
            state.pending = Some(Pending {
                write: input.write,
                line: input.read_line,
                address: input.address as usize,
                write_data: input.write_data as u16,
            });
            state.delay = 2;
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("sdram_word_port.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("sdram_word_port_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_circuit::{build_circuit, Circuit};

    #[allow(clippy::too_many_arguments)]
    fn drive(
        circuit: &mut Circuit,
        input: &TangNano20KSdramWordPortInput,
        request_valid: bool,
        write: bool,
        read_line: bool,
        address: u64,
        write_data: u64,
        response_ready: bool,
    ) {
        input.drive(
            circuit,
            &TangNano20KSdramWordPortInputValue {
                reset: false,
                request_valid,
                write,
                read_line,
                address,
                write_data,
                response_ready,
                controller_read_data: 0,
                controller_read_valid: false,
                controller_init_done: true,
                controller_command_ack: false,
            },
        );
    }

    fn transact(
        circuit: &mut Circuit,
        input: &TangNano20KSdramWordPortInput,
        output: &TangNano20KSdramWordPortOutput,
        write: bool,
        address: u64,
        write_data: u64,
    ) -> u64 {
        drive(circuit, input, true, write, false, address, write_data, false);
        circuit.execute_gates();
        assert!(output.sample(circuit).request_ready);
        circuit.clock_tick();
        drive(circuit, input, false, false, false, 0, 0, false);
        for _ in 0..8 {
            circuit.execute_gates();
            let value = output.sample(circuit);
            if value.response_valid {
                let result = value.read_data;
                assert!(value.response_last, "word response must carry last");
                drive(circuit, input, false, false, false, 0, 0, true);
                circuit.clock_tick();
                return result;
            }
            circuit.clock_tick();
        }
        panic!("SDRAM emulator transaction did not complete")
    }

    fn read_line(
        circuit: &mut Circuit,
        input: &TangNano20KSdramWordPortInput,
        output: &TangNano20KSdramWordPortOutput,
        address: u64,
    ) -> [u32; LINE_BEATS] {
        drive(circuit, input, true, false, true, address, 0, false);
        circuit.execute_gates();
        assert!(output.sample(circuit).request_ready);
        circuit.clock_tick();
        drive(circuit, input, false, false, false, 0, 0, false);
        let mut beats = [0; LINE_BEATS];
        let mut received = 0;
        for _ in 0..32 {
            circuit.execute_gates();
            let value = output.sample(circuit);
            if value.response_valid {
                assert!(received < LINE_BEATS, "line returned too many beats");
                assert_eq!(
                    value.response_last,
                    received + 1 == LINE_BEATS,
                    "last must mark only the final beat"
                );
                beats[received] = value.read_data as u32;
                received += 1;
                if received == LINE_BEATS {
                    circuit.clock_tick();
                    return beats;
                }
            }
            circuit.clock_tick();
        }
        panic!("SDRAM emulator line read did not complete")
    }

    #[test]
    fn emulator_preserves_full_physical_addresses() {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = TangNano20KSdramWordPortInput::allocate();
            let output = TangNano20KSdramWordPort::hardware(&input);
            (input, output)
        });
        drive(&mut circuit, &input, false, false, false, 0, 0, false);
        assert_eq!(transact(&mut circuit, &input, &output, true, 7, 0x1234), 0);
        assert_eq!(
            transact(&mut circuit, &input, &output, true, 0x10_0007, 0xabcd),
            0
        );
        assert_eq!(transact(&mut circuit, &input, &output, false, 7, 0), 0x1234);
        assert_eq!(
            transact(&mut circuit, &input, &output, false, 0x10_0007, 0),
            0xabcd
        );
    }

    #[test]
    #[ignore = "explicit external simulation of the Controller HS transaction adapter"]
    fn verify_verilog_with_iverilog() {
        crate::verify_verilog_with_iverilog::<TangNano20KSdramWordPort>().unwrap();
    }

    #[test]
    fn line_read_streams_eight_ordered_beats() {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = TangNano20KSdramWordPortInput::allocate();
            let output = TangNano20KSdramWordPort::hardware(&input);
            (input, output)
        });
        drive(&mut circuit, &input, false, false, false, 0, 0, false);
        for word in 0x120u64..0x130 {
            transact(&mut circuit, &input, &output, true, word, 0x4000 + word);
        }
        let beats = read_line(&mut circuit, &input, &output, 0x120);
        for (index, beat) in beats.iter().enumerate() {
            let low = 0x4120 + 2 * index as u32;
            let high = low + 1;
            assert_eq!(*beat, high << 16 | low, "beat {index} pairing");
        }
        // A word read afterwards still works and sees the written data.
        assert_eq!(transact(&mut circuit, &input, &output, false, 0x121, 0), 0x4121);
    }
}
