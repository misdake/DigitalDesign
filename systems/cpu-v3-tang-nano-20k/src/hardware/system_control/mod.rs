//! CpuV3 device-0 system control: cache invalidate pulses, LEDs, and UART TX.
//!
//! The device connects to the CpuV3 device port as device index 0 and answers
//! the standard device register-bank interface (`device_index` /
//! `device_channel` / enables / write data / read data):
//!
//! - channel 0 (write): pulse `icache_invalidate` for one clock;
//! - channel 1 (write): pulse `dcache_invalidate` for one clock;
//! - channel 2 (write): drive `leds[5:0]` from the low six write-data bits
//!   (logical value; a board wrapper applies active-low inversion);
//! - channel 3 (write): enqueue `write_data[7:0]` to the 8N1 UART
//!   transmitter; (read): bit 0 is the transmitter busy flag.
//!
//! Writes while the UART is busy are dropped; software must poll the busy
//! flag before enqueueing the next byte. All accesses with a device index
//! other than 0 are ignored and read back as zero.

use crate::{Hardware, HardwareIdentity, Module, ModuleIo};
use askama::Template;
use digital_design_circuit::{CircuitWires, Wire, Wires};

pub use crate::boot::SYSCTL_INVALIDATE_DCACHE as SYSTEM_CONTROL_CHANNEL_DCACHE_INVALIDATE;
pub use crate::boot::SYSCTL_INVALIDATE_ICACHE as SYSTEM_CONTROL_CHANNEL_ICACHE_INVALIDATE;
pub use crate::boot::SYSCTL_LED as SYSTEM_CONTROL_CHANNEL_LEDS;
pub use crate::boot::SYSCTL_UART as SYSTEM_CONTROL_CHANNEL_UART;
/// Device index of the system control device on the CpuV3 device port.
pub use crate::boot::SYSTEM_CONTROL_DEVICE;

#[derive(Clone, ModuleIo)]
pub struct SystemControlDeviceInput {
    pub reset: Wire,
    pub device_index: Wires<3>,
    pub device_channel: Wires<4>,
    pub device_read_enable: Wire,
    pub device_write_enable: Wire,
    pub device_write_data: Wires<16>,
}

#[derive(Clone, ModuleIo)]
pub struct SystemControlDeviceOutput {
    pub device_read_data: Wires<16>,
    pub icache_invalidate: Wire,
    pub dcache_invalidate: Wire,
    /// Logical LED value; board wrappers handle active-low inversion.
    pub leds: Wires<6>,
    /// 8N1 serial output, idle high.
    pub uart_tx: Wire,
}

/// System control device with a single-byte 8N1 UART transmitter.
///
/// `CLOCKS_PER_BIT` is the UART bit period in main clocks (234 for a 27 MHz
/// design at 115200 baud, 469 for 54 MHz). One frame is ten bits (start,
/// eight data bits LSB first, stop) and therefore occupies exactly
/// `10 * CLOCKS_PER_BIT` clocks after the accepting write clock.
#[derive(Hardware)]
#[hardware(namespace = "components/system_control")]
pub struct SystemControlDevice<const CLOCKS_PER_BIT: u16>;

pub struct SystemControlDeviceState {
    icache_invalidate: bool,
    dcache_invalidate: bool,
    leds: u8,
    uart_busy: bool,
    uart_frame: u16,
    uart_bit: u8,
    uart_divider: u16,
}

impl Default for SystemControlDeviceState {
    fn default() -> Self {
        Self {
            icache_invalidate: false,
            dcache_invalidate: false,
            leds: 0,
            uart_busy: false,
            uart_frame: 0x3ff,
            uart_bit: 0,
            uart_divider: 0,
        }
    }
}

fn validate<const CLOCKS_PER_BIT: u16>() {
    assert!(
        CLOCKS_PER_BIT > 0,
        "UART bit period must contain at least one main-clock cycle"
    );
}

#[derive(Template)]
#[template(path = "hardware/system_control/system_control.v", escape = "none")]
struct SystemControlTemplate<'a> {
    module_name: &'a str,
    clocks_per_bit_minus_one: u16,
}

#[derive(Template)]
#[template(path = "hardware/system_control/system_control_tb.v", escape = "none")]
struct SystemControlTestbenchTemplate<'a> {
    module_name: &'a str,
    clocks_per_bit: u16,
    clocks_per_bit_minus_one: u16,
}

impl<const CLOCKS_PER_BIT: u16> Module for SystemControlDevice<CLOCKS_PER_BIT> {
    type Input = SystemControlDeviceInput;
    type Output = SystemControlDeviceOutput;
    type EmuState = SystemControlDeviceState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        validate::<CLOCKS_PER_BIT>();
        SystemControlDeviceState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let selected =
            input.device_read_enable && input.device_index == u64::from(SYSTEM_CONTROL_DEVICE);
        let read_data =
            if selected && input.device_channel == u64::from(SYSTEM_CONTROL_CHANNEL_UART) {
                u16::from(state.uart_busy)
            } else {
                0
            };
        output.drive(
            circuit,
            &SystemControlDeviceOutputValue {
                device_read_data: u64::from(read_data),
                icache_invalidate: state.icache_invalidate,
                dcache_invalidate: state.dcache_invalidate,
                leds: u64::from(state.leds),
                uart_tx: !state.uart_busy || ((state.uart_frame >> state.uart_bit) & 1) == 1,
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
            *state = SystemControlDeviceState::default();
            return;
        }
        state.icache_invalidate = false;
        state.dcache_invalidate = false;

        let was_busy = state.uart_busy;
        if was_busy {
            if state.uart_divider == CLOCKS_PER_BIT - 1 {
                state.uart_divider = 0;
                if state.uart_bit == 9 {
                    state.uart_busy = false;
                } else {
                    state.uart_bit += 1;
                }
            } else {
                state.uart_divider += 1;
            }
        }

        if !input.device_write_enable || input.device_index != u64::from(SYSTEM_CONTROL_DEVICE) {
            return;
        }
        let value = input.device_write_data as u16;
        match input.device_channel as u8 {
            SYSTEM_CONTROL_CHANNEL_ICACHE_INVALIDATE => state.icache_invalidate = true,
            SYSTEM_CONTROL_CHANNEL_DCACHE_INVALIDATE => state.dcache_invalidate = true,
            SYSTEM_CONTROL_CHANNEL_LEDS => state.leds = (value & 0x3f) as u8,
            // A write while busy is dropped; software polls the busy flag.
            SYSTEM_CONTROL_CHANNEL_UART if !was_busy => {
                state.uart_frame = 0x200 | (u16::from(value as u8) << 1);
                state.uart_bit = 0;
                state.uart_divider = 0;
                state.uart_busy = true;
            }
            _ => {}
        }
    }

    fn generated_verilog_source() -> Option<String> {
        validate::<CLOCKS_PER_BIT>();
        let module_name = Self::verilog_identity().module_name();
        Some(
            SystemControlTemplate {
                module_name: &module_name,
                clocks_per_bit_minus_one: CLOCKS_PER_BIT - 1,
            }
            .render()
            .expect("system control Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        validate::<CLOCKS_PER_BIT>();
        let module_name = Self::verilog_identity().module_name();
        Some(
            SystemControlTestbenchTemplate {
                module_name: &module_name,
                clocks_per_bit: CLOCKS_PER_BIT,
                clocks_per_bit_minus_one: CLOCKS_PER_BIT - 1,
            }
            .render()
            .expect("system control Verilog testbench template must render"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModuleTest, TestStep, VerilogProject};
    use std::path::Path;

    /// Small representative bit period; 234/469 are covered by the same
    /// divider logic and by export assertions.
    type TestDevice = SystemControlDevice<4>;

    const IDLE: SystemControlDeviceInputValue = SystemControlDeviceInputValue {
        reset: false,
        device_index: 0,
        device_channel: 0,
        device_read_enable: false,
        device_write_enable: false,
        device_write_data: 0,
    };

    fn write(channel: u64, data: u64) -> SystemControlDeviceInputValue {
        SystemControlDeviceInputValue {
            device_channel: channel,
            device_write_enable: true,
            device_write_data: data,
            ..IDLE
        }
    }

    fn write_index(index: u64, channel: u64, data: u64) -> SystemControlDeviceInputValue {
        SystemControlDeviceInputValue {
            device_index: index,
            ..write(channel, data)
        }
    }

    fn read(channel: u64) -> SystemControlDeviceInputValue {
        SystemControlDeviceInputValue {
            device_channel: channel,
            device_read_enable: true,
            ..IDLE
        }
    }

    fn read_index(index: u64, channel: u64) -> SystemControlDeviceInputValue {
        SystemControlDeviceInputValue {
            device_index: index,
            ..read(channel)
        }
    }

    fn output(
        read_data: u64,
        icache_invalidate: bool,
        dcache_invalidate: bool,
        leds: u64,
        uart_tx: bool,
    ) -> SystemControlDeviceOutputValue {
        SystemControlDeviceOutputValue {
            device_read_data: read_data,
            icache_invalidate,
            dcache_invalidate,
            leds,
            uart_tx,
        }
    }

    /// Vectors shared with `system_control_tb.v`. With `CLOCKS_PER_BIT = 4`
    /// one 8N1 frame is ten four-clock bits, so each `after_cycles(4)` step
    /// after the accepting write samples the next frame bit.
    fn system_control_test() -> ModuleTest<TestDevice> {
        let uart = u64::from(SYSTEM_CONTROL_CHANNEL_UART);
        ModuleTest::new(vec![
            // Reset clears the LEDs and leaves the UART idle-high.
            TestStep::new(
                SystemControlDeviceInputValue {
                    reset: true,
                    ..IDLE
                },
                output(0, false, false, 0, true),
            ),
            // Channel 0 pulses icache_invalidate for exactly one clock.
            TestStep::new(write(0, 0xffff), output(0, true, false, 0, true)),
            TestStep::new(IDLE, output(0, false, false, 0, true)),
            // Channel 1 pulses dcache_invalidate for exactly one clock.
            TestStep::new(write(1, 0), output(0, false, true, 0, true)),
            TestStep::new(IDLE, output(0, false, false, 0, true)),
            // Writes to another device index are ignored.
            TestStep::new(write_index(2, 0, 1), output(0, false, false, 0, true)),
            TestStep::new(write_index(2, 2, 0x003f), output(0, false, false, 0, true)),
            // Channel 2 drives the six LEDs from the low write-data bits.
            TestStep::new(write(2, 0xffea), output(0, false, false, 0x2a, true)),
            // The UART reports not busy before the first byte.
            TestStep::new(read(uart), output(0, false, false, 0x2a, true)),
            // Enqueue 0xa5: the start bit and the busy flag appear together.
            TestStep::new(
                SystemControlDeviceInputValue {
                    device_read_enable: true,
                    ..write(uart, 0x00a5)
                },
                output(1, false, false, 0x2a, false),
            ),
            // Reads through another device index observe nothing.
            TestStep::new(read_index(2, uart), output(0, false, false, 0x2a, true)).after_cycles(4),
            // 0xa5 shifts out LSB first: 1, 0, 1, 0, 0, 1, 0, 1.
            TestStep::new(IDLE, output(0, false, false, 0x2a, false)).after_cycles(4),
            TestStep::new(IDLE, output(0, false, false, 0x2a, true)).after_cycles(4),
            TestStep::new(IDLE, output(0, false, false, 0x2a, false)).after_cycles(4),
            TestStep::new(IDLE, output(0, false, false, 0x2a, false)).after_cycles(4),
            TestStep::new(IDLE, output(0, false, false, 0x2a, true)).after_cycles(4),
            TestStep::new(IDLE, output(0, false, false, 0x2a, false)).after_cycles(4),
            TestStep::new(IDLE, output(0, false, false, 0x2a, true)).after_cycles(4),
            // The stop bit is high while the transmitter is still busy.
            TestStep::new(read(uart), output(1, false, false, 0x2a, true)).after_cycles(4),
            // Exactly 10 * CLOCKS_PER_BIT clocks after the write it is idle.
            TestStep::new(read(uart), output(0, false, false, 0x2a, true)).after_cycles(4),
            // A write while busy is dropped; 0xa5 continues undisturbed.
            TestStep::new(write(uart, 0x00a5), output(0, false, false, 0x2a, false)),
            TestStep::new(write(uart, 0x00ff), output(0, false, false, 0x2a, false)),
            TestStep::new(IDLE, output(0, false, false, 0x2a, true)).after_cycles(3),
            // 0xa5 data bit 1 is low; the dropped 0xff would read high here.
            TestStep::new(IDLE, output(0, false, false, 0x2a, false)).after_cycles(4),
            // The remaining eight bits complete the first frame.
            TestStep::new(read(uart), output(0, false, false, 0x2a, true)).after_cycles(32),
            // Reset aborts a frame in flight and clears the LEDs.
            TestStep::new(write(uart, 0x0055), output(0, false, false, 0x2a, false)),
            TestStep::new(
                SystemControlDeviceInputValue {
                    reset: true,
                    ..IDLE
                },
                output(0, false, false, 0, true),
            ),
            TestStep::new(read(uart), output(0, false, false, 0, true)),
        ])
    }

    #[test]
    fn emulator_matches_the_documented_cycle_behavior() {
        system_control_test().run_emu();
    }

    #[test]
    fn each_bit_period_exports_one_fabric_module() {
        let project = VerilogProject::generate::<TestDevice>().unwrap();
        assert!(project.resource_claims.is_empty());
        assert!(project.files.contains_key(Path::new(
            "components/system_control/system_control_device/clocks_per_bit4.v"
        )));
        let source = project
            .files
            .get(Path::new(
                "components/system_control/system_control_device/clocks_per_bit4.v",
            ))
            .unwrap();
        assert!(source.contains("uart_divider == 16'd3"));
    }

    #[test]
    #[ignore = "explicit external simulator validation of the system control device"]
    fn verify_verilog_with_iverilog() {
        crate::verify_verilog_with_iverilog::<TestDevice>().unwrap();
    }
}
