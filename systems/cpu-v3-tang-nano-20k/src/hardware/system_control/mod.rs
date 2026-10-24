//! CpuV3 device-0 system control: cache maintenance, CPU hold, LEDs, and UART TX.
//!
//! The device connects to the CpuV3 device port as device index 0 and answers
//! the standard device register-bank interface (`device_index` /
//! `device_channel` / enables / write data / read data):
//!
//! - channel 0 (write): pulse `icache_invalidate` for one clock;
//! - channel 1 (write): start blocking D-cache clean-plus-invalidate;
//! - channel 2 (write): drive `leds[5:0]` from the low six write-data bits
//!   (logical value; a board wrapper applies active-low inversion);
//! - channel 3 (write): enqueue `write_data[7:0]` to the 8N1 UART
//!   transmitter; (read): bit 0 is the transmitter busy flag.
//! - channel 4 (write): start blocking D-cache clean;
//! - channel 5 (read): return the final maintenance status.
//!
//! The CPU orders channel-1/channel-4 writes after its old stores/FST tails.
//! This device then retains the cache command until maintenance_ready accepts
//! it, and holds CPU progress until that accepted command's done/error.
//! - channel 6 (write): stage a generic watch target as `{channel[3:0], device[2:0]}`;
//! - channel 7 (write): arm a CPU-local sleep while the target equals the written value.
//! - channel 10 (read): free-running milliseconds modulo 65536. CPU hold,
//!   cache maintenance, and watch sleep never gate this wall clock.
//!
//! Writes while the UART is busy are dropped; software must poll the busy
//! flag before enqueueing the next byte. All accesses with a device index
//! other than 0 are ignored and read back as zero.

use crate::{Hardware, HardwareIdentity, Module, ModuleIo};
use askama::Template;
use digital_design_circuit::{CircuitWires, Wire, Wires};

pub use crate::boot::CACHE_MAINTENANCE_STATUS as SYSTEM_CONTROL_CHANNEL_CACHE_MAINTENANCE_STATUS;
pub use crate::boot::D_CLEAN_ALL as SYSTEM_CONTROL_CHANNEL_D_CLEAN_ALL;
pub use crate::boot::D_INVALIDATE_ALL as SYSTEM_CONTROL_CHANNEL_D_INVALIDATE_ALL;
pub use crate::boot::ICACHE_INVALIDATE_ALL_DELAYED as SYSTEM_CONTROL_CHANNEL_ICACHE_INVALIDATE_ALL_DELAYED;
pub use crate::boot::SYSCTL_LED as SYSTEM_CONTROL_CHANNEL_LEDS;
pub use crate::boot::SYSCTL_MILLISECONDS as SYSTEM_CONTROL_CHANNEL_MILLISECONDS;
pub use crate::boot::SYSCTL_UART as SYSTEM_CONTROL_CHANNEL_UART;
pub use crate::boot::SYSCTL_WATCH_EXPECTED as SYSTEM_CONTROL_CHANNEL_WATCH_EXPECTED;
pub use crate::boot::SYSCTL_WATCH_TARGET as SYSTEM_CONTROL_CHANNEL_WATCH_TARGET;
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
    /// Acceptance of the current retained request, not maintenance-busy.
    pub dcache_maintenance_ready: Wire,
    pub dcache_maintenance_done: Wire,
    pub dcache_maintenance_error: Wire,
    /// Readback from the device/channel selected by the watch probe outputs.
    pub watch_read_data: Wires<16>,
}

#[derive(Clone, ModuleIo)]
pub struct SystemControlDeviceOutput {
    pub device_read_data: Wires<16>,
    pub icache_invalidate: Wire,
    pub dcache_invalidate: Wire,
    pub dcache_clean: Wire,
    /// CPU-local hold. This must never be used to gate DMA, display, SDRAM,
    /// or other machine-owned hardware while D-cache maintenance runs.
    pub cpu_hold: Wire,
    pub watch_device_index: Wires<3>,
    pub watch_device_channel: Wires<4>,
    pub watch_read_enable: Wire,
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
/// `CLOCKS_PER_MS` is independently set from the physical main clock: 54000
/// for 54 MHz, 27000 for 27 MHz. UART baud-rate rounding cannot determine it.
#[derive(Hardware)]
#[hardware(namespace = "components/system_control")]
pub struct SystemControlDevice<const CLOCKS_PER_BIT: u16, const CLOCKS_PER_MS: u16 = 54000>;

pub struct SystemControlDeviceState {
    icache_invalidate: bool,
    maintenance_hold: bool,
    watch_active: bool,
    watch_device_index: u8,
    watch_device_channel: u8,
    watch_expected: u16,
    cache_maintenance_status: u16,
    pending_dcache_maintenance: u8,
    leds: u8,
    uart_busy: bool,
    uart_frame: u16,
    uart_bit: u8,
    uart_divider: u16,
    millisecond_divider: u16,
    milliseconds: u16,
}

impl Default for SystemControlDeviceState {
    fn default() -> Self {
        Self {
            icache_invalidate: false,
            maintenance_hold: false,
            watch_active: false,
            watch_device_index: 0,
            watch_device_channel: 0,
            watch_expected: 0,
            cache_maintenance_status: crate::boot::CACHE_MAINTENANCE_STATUS_SUCCESS,
            pending_dcache_maintenance: 0,
            leds: 0,
            uart_busy: false,
            uart_frame: 0x3ff,
            uart_bit: 0,
            uart_divider: 0,
            millisecond_divider: 0,
            milliseconds: 0,
        }
    }
}

fn validate<const CLOCKS_PER_BIT: u16, const CLOCKS_PER_MS: u16>() {
    assert!(
        CLOCKS_PER_BIT > 0,
        "UART bit period must contain at least one main-clock cycle"
    );
    assert!(CLOCKS_PER_MS > 0, "millisecond period must be positive");
}

#[derive(Template)]
#[template(path = "hardware/system_control/system_control.v", escape = "none")]
struct SystemControlTemplate<'a> {
    module_name: &'a str,
    clocks_per_bit_minus_one: u16,
    uart_divider_msb: u32,
    clocks_per_ms_minus_one: u16,
}

#[derive(Template)]
#[template(path = "hardware/system_control/system_control_tb.v", escape = "none")]
struct SystemControlTestbenchTemplate<'a> {
    module_name: &'a str,
    clocks_per_bit: u16,
    clocks_per_bit_minus_one: u16,
}

impl<const CLOCKS_PER_BIT: u16, const CLOCKS_PER_MS: u16> Module
    for SystemControlDevice<CLOCKS_PER_BIT, CLOCKS_PER_MS>
{
    type Input = SystemControlDeviceInput;
    type Output = SystemControlDeviceOutput;
    type EmuState = SystemControlDeviceState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        validate::<CLOCKS_PER_BIT, CLOCKS_PER_MS>();
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
        let read_data = if selected {
            match input.device_channel as u8 {
                SYSTEM_CONTROL_CHANNEL_UART => u16::from(state.uart_busy),
                SYSTEM_CONTROL_CHANNEL_CACHE_MAINTENANCE_STATUS => state.cache_maintenance_status,
                SYSTEM_CONTROL_CHANNEL_MILLISECONDS => state.milliseconds,
                _ => 0,
            }
        } else {
            0
        };
        output.drive(
            circuit,
            &SystemControlDeviceOutputValue {
                device_read_data: u64::from(read_data),
                icache_invalidate: state.icache_invalidate,
                dcache_invalidate: state.pending_dcache_maintenance == 1,
                dcache_clean: state.pending_dcache_maintenance == 2,
                cpu_hold: state.maintenance_hold || state.watch_active,
                watch_device_index: u64::from(state.watch_device_index),
                watch_device_channel: u64::from(state.watch_device_channel),
                watch_read_enable: state.watch_active,
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
        // Physical wall time advances even while the CPU is held or no device
        // read is active. This rate is independent of the UART divider.
        if state.millisecond_divider == CLOCKS_PER_MS - 1 {
            state.millisecond_divider = 0;
            state.milliseconds = state.milliseconds.wrapping_add(1);
        } else {
            state.millisecond_divider += 1;
        }

        let maintenance_was_pending = state.pending_dcache_maintenance != 0;
        let held_before_edge = state.maintenance_hold || state.watch_active;
        if maintenance_was_pending && input.dcache_maintenance_ready {
            state.pending_dcache_maintenance = 0;
        }

        // A line copy can lower busy and raise done on the same clock where a
        // deferred command becomes issuable. That completion belongs to the
        // copy, not to the maintenance operation emitted above.
        if state.maintenance_hold && !maintenance_was_pending && input.dcache_maintenance_done {
            state.maintenance_hold = false;
            state.cache_maintenance_status = if input.dcache_maintenance_error {
                crate::boot::CACHE_MAINTENANCE_STATUS_ERROR
            } else {
                crate::boot::CACHE_MAINTENANCE_STATUS_SUCCESS
            };
        }

        if state.watch_active && input.watch_read_data as u16 != state.watch_expected {
            state.watch_active = false;
        }

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
            SYSTEM_CONTROL_CHANNEL_ICACHE_INVALIDATE_ALL_DELAYED => state.icache_invalidate = true,
            SYSTEM_CONTROL_CHANNEL_D_INVALIDATE_ALL if !held_before_edge => {
                state.maintenance_hold = true;
                state.pending_dcache_maintenance = 1;
            }
            SYSTEM_CONTROL_CHANNEL_D_CLEAN_ALL if !held_before_edge => {
                state.maintenance_hold = true;
                state.pending_dcache_maintenance = 2;
            }
            SYSTEM_CONTROL_CHANNEL_LEDS => state.leds = (value & 0x3f) as u8,
            // A write while busy is dropped; software polls the busy flag.
            SYSTEM_CONTROL_CHANNEL_UART if !was_busy => {
                state.uart_frame = 0x200 | (u16::from(value as u8) << 1);
                state.uart_bit = 0;
                state.uart_divider = 0;
                state.uart_busy = true;
            }
            SYSTEM_CONTROL_CHANNEL_WATCH_TARGET => {
                state.watch_device_index = (value & 7) as u8;
                state.watch_device_channel = ((value >> 3) & 15) as u8;
            }
            SYSTEM_CONTROL_CHANNEL_WATCH_EXPECTED if !held_before_edge => {
                state.watch_expected = value;
                state.watch_active = true;
            }
            _ => {}
        }
    }

    fn generated_verilog_source() -> Option<String> {
        validate::<CLOCKS_PER_BIT, CLOCKS_PER_MS>();
        let module_name = Self::verilog_identity().module_name();
        Some(
            SystemControlTemplate {
                module_name: &module_name,
                clocks_per_bit_minus_one: CLOCKS_PER_BIT - 1,
                uart_divider_msb: (u16::BITS - (CLOCKS_PER_BIT - 1).leading_zeros()).max(1) - 1,
                clocks_per_ms_minus_one: CLOCKS_PER_MS - 1,
            }
            .render()
            .expect("system control Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        validate::<CLOCKS_PER_BIT, CLOCKS_PER_MS>();
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
        dcache_maintenance_ready: true,
        dcache_maintenance_done: false,
        dcache_maintenance_error: false,
        watch_read_data: 0,
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
            dcache_clean: false,
            cpu_hold: false,
            watch_device_index: 0,
            watch_device_channel: 0,
            watch_read_enable: false,
            leds,
            uart_tx,
        }
    }

    fn maintenance_output(
        read_data: u64,
        invalidate: bool,
        clean: bool,
        hold: bool,
        leds: u64,
    ) -> SystemControlDeviceOutputValue {
        SystemControlDeviceOutputValue {
            device_read_data: read_data,
            icache_invalidate: false,
            dcache_invalidate: invalidate,
            dcache_clean: clean,
            cpu_hold: hold,
            watch_device_index: 0,
            watch_device_channel: 0,
            watch_read_enable: false,
            leds,
            uart_tx: true,
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
            // Ready accepts a retained D-cache request on the following edge.
            TestStep::new(write(1, 0), maintenance_output(0, true, false, true, 0)),
            TestStep::new(IDLE, maintenance_output(0, false, false, true, 0)),
            TestStep::new(
                SystemControlDeviceInputValue {
                    dcache_maintenance_done: true,
                    ..IDLE
                },
                output(0, false, false, 0, true),
            ),
            // A command arriving while LCOPY owns the D-cache is retained.
            // Its completion pulse must not be mistaken for this invalidate's
            // completion when the deferred command is finally emitted.
            TestStep::new(
                SystemControlDeviceInputValue {
                    dcache_maintenance_ready: false,
                    ..write(1, 0)
                },
                maintenance_output(0, true, false, true, 0),
            ),
            TestStep::new(
                SystemControlDeviceInputValue {
                    dcache_maintenance_ready: false,
                    ..IDLE
                },
                maintenance_output(0, true, false, true, 0),
            ),
            TestStep::new(
                SystemControlDeviceInputValue {
                    dcache_maintenance_done: true,
                    ..IDLE
                },
                maintenance_output(0, false, false, true, 0),
            ),
            TestStep::new(IDLE, maintenance_output(0, false, false, true, 0)),
            TestStep::new(
                SystemControlDeviceInputValue {
                    dcache_maintenance_done: true,
                    ..IDLE
                },
                output(0, false, false, 0, true),
            ),
            // Channel 4 starts a clean and completion records the final error.
            TestStep::new(write(4, 0), maintenance_output(0, false, true, true, 0)),
            // The hold is consumed only by the CPU. The system-control
            // peripheral itself continues to accept work while it is high.
            TestStep::new(
                write(2, 0x0025),
                maintenance_output(0, false, false, true, 0x25),
            ),
            TestStep::new(
                SystemControlDeviceInputValue {
                    dcache_maintenance_done: true,
                    dcache_maintenance_error: true,
                    ..IDLE
                },
                output(0, false, false, 0x25, true),
            ),
            TestStep::new(
                read(u64::from(SYSTEM_CONTROL_CHANNEL_CACHE_MAINTENANCE_STATUS)),
                output(
                    u64::from(crate::boot::CACHE_MAINTENANCE_STATUS_ERROR),
                    false,
                    false,
                    0x25,
                    true,
                ),
            ),
            // Writes to another device index are ignored.
            TestStep::new(write_index(2, 0, 1), output(0, false, false, 0x25, true)),
            TestStep::new(
                write_index(2, 2, 0x003f),
                output(0, false, false, 0x25, true),
            ),
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
            // Stage GPU executed_count (device 4, channel 1), then sleep
            // while its value is 7. The probe keeps running while the CPU is
            // held and releases it on the first changed sample.
            TestStep::new(
                write(
                    u64::from(SYSTEM_CONTROL_CHANNEL_WATCH_TARGET),
                    u64::from(crate::boot::sysctl_watch_target(4, 1)),
                ),
                SystemControlDeviceOutputValue {
                    watch_device_index: 4,
                    watch_device_channel: 1,
                    ..output(0, false, false, 0, true)
                },
            ),
            TestStep::new(
                write(u64::from(SYSTEM_CONTROL_CHANNEL_WATCH_EXPECTED), 7),
                SystemControlDeviceOutputValue {
                    cpu_hold: true,
                    watch_device_index: 4,
                    watch_device_channel: 1,
                    watch_read_enable: true,
                    ..output(0, false, false, 0, true)
                },
            ),
            TestStep::new(
                SystemControlDeviceInputValue {
                    watch_read_data: 7,
                    ..IDLE
                },
                SystemControlDeviceOutputValue {
                    cpu_hold: true,
                    watch_device_index: 4,
                    watch_device_channel: 1,
                    watch_read_enable: true,
                    ..output(0, false, false, 0, true)
                },
            ),
            TestStep::new(
                SystemControlDeviceInputValue {
                    watch_read_data: 8,
                    ..IDLE
                },
                SystemControlDeviceOutputValue {
                    watch_device_index: 4,
                    watch_device_channel: 1,
                    ..output(0, false, false, 0, true)
                },
            ),
            // If the target changed between the software snapshot and arm,
            // the first probe releases the CPU instead of missing the event.
            TestStep::new(
                SystemControlDeviceInputValue {
                    watch_read_data: 10,
                    ..write(u64::from(SYSTEM_CONTROL_CHANNEL_WATCH_EXPECTED), 9)
                },
                SystemControlDeviceOutputValue {
                    cpu_hold: true,
                    watch_device_index: 4,
                    watch_device_channel: 1,
                    watch_read_enable: true,
                    ..output(0, false, false, 0, true)
                },
            ),
            TestStep::new(
                SystemControlDeviceInputValue {
                    watch_read_data: 10,
                    ..IDLE
                },
                SystemControlDeviceOutputValue {
                    watch_device_index: 4,
                    watch_device_channel: 1,
                    ..output(0, false, false, 0, true)
                },
            ),
        ])
    }

    #[test]
    fn emulator_matches_the_documented_cycle_behavior() {
        system_control_test().run_emu();
    }

    #[test]
    fn pending_request_reset_and_completion_edge_ownership() {
        ModuleTest::<TestDevice>::new(vec![
            TestStep::new(
                SystemControlDeviceInputValue {
                    reset: true,
                    ..IDLE
                },
                output(0, false, false, 0, true),
            ),
            TestStep::new(
                SystemControlDeviceInputValue {
                    dcache_maintenance_ready: false,
                    ..write(1, 0)
                },
                maintenance_output(0, true, false, true, 0),
            ),
            // Another write and stale completion cannot replace an unaccepted request.
            TestStep::new(
                SystemControlDeviceInputValue {
                    dcache_maintenance_ready: false,
                    dcache_maintenance_done: true,
                    ..write(4, 0)
                },
                maintenance_output(0, true, false, true, 0),
            ),
            TestStep::new(
                SystemControlDeviceInputValue {
                    reset: true,
                    ..IDLE
                },
                output(0, false, false, 0, true),
            ),
            TestStep::new(write(4, 0), maintenance_output(0, false, true, true, 0)),
            TestStep::new(IDLE, maintenance_output(0, false, false, true, 0)),
            // Hold was high before this edge: release, but do not accept a new write.
            TestStep::new(
                SystemControlDeviceInputValue {
                    dcache_maintenance_done: true,
                    ..write(1, 0)
                },
                output(0, false, false, 0, true),
            ),
            TestStep::new(IDLE, output(0, false, false, 0, true)),
        ])
        .run_emu();
    }

    #[test]
    fn each_bit_period_exports_one_fabric_module() {
        let project = VerilogProject::generate::<TestDevice>().unwrap();
        assert!(project.resource_claims.is_empty());
        let source = project
            .files
            .values()
            .find(|source| source.contains("reg [15:0] millisecond_divider"))
            .unwrap();
        assert!(source.contains("uart_divider == 16'd3"));
        assert!(source.contains("millisecond_divider == 16'd53999"));
    }

    #[test]
    fn milliseconds_exact_divider_read_only_and_default_channels() {
        let mut steps = vec![
            TestStep::new(
                SystemControlDeviceInputValue {
                    reset: true,
                    ..IDLE
                },
                output(0, false, false, 0, true),
            ),
            TestStep::new(read(10), output(0, false, false, 0, true)).after_cycles(53999),
            TestStep::new(read(10), output(1, false, false, 0, true)),
            TestStep::new(write(10, 65535), output(0, false, false, 0, true)),
            TestStep::new(read(10), output(1, false, false, 0, true)),
        ];
        for ch in [0, 1, 2, 4, 6, 7, 8, 9, 11, 12, 13, 14, 15] {
            steps.push(TestStep::new(read(ch), output(0, false, false, 0, true)));
        }
        steps.push(TestStep::new(
            read_index(1, 10),
            output(0, false, false, 0, true),
        ));
        ModuleTest::<SystemControlDevice<4, 54000>>::new(steps).run_emu();
    }

    #[test]
    fn milliseconds_continue_through_maintenance_watch_and_wrap() {
        ModuleTest::<SystemControlDevice<4, 2>>::new(vec![
            TestStep::new(
                SystemControlDeviceInputValue {
                    reset: true,
                    ..IDLE
                },
                output(0, false, false, 0, true),
            ),
            TestStep::new(write(4, 0), maintenance_output(0, false, true, true, 0)),
            TestStep::new(read(10), maintenance_output(20, false, false, true, 0)).after_cycles(39),
            TestStep::new(
                SystemControlDeviceInputValue {
                    dcache_maintenance_done: true,
                    ..read(10)
                },
                output(20, false, false, 0, true),
            ),
            TestStep::new(
                write(7, 0),
                SystemControlDeviceOutputValue {
                    cpu_hold: true,
                    watch_read_enable: true,
                    ..output(0, false, false, 0, true)
                },
            ),
            TestStep::new(
                read(10),
                SystemControlDeviceOutputValue {
                    cpu_hold: true,
                    watch_read_enable: true,
                    ..output(41, false, false, 0, true)
                },
            )
            .after_cycles(40),
            TestStep::new(
                SystemControlDeviceInputValue {
                    watch_read_data: 1,
                    ..read(10)
                },
                output(41, false, false, 0, true),
            ),
            TestStep::new(
                SystemControlDeviceInputValue {
                    reset: true,
                    ..IDLE
                },
                output(0, false, false, 0, true),
            ),
            TestStep::new(read(10), output(65535, false, false, 0, true)).after_cycles(131071),
            TestStep::new(read(10), output(0, false, false, 0, true)),
            TestStep::new(read(10), output(1, false, false, 0, true)).after_cycles(2),
        ])
        .run_emu();
    }

    #[test]
    #[ignore = "explicit external simulator validation of the system control device"]
    fn verify_verilog_with_iverilog() {
        crate::verify_verilog_with_iverilog::<TestDevice>().unwrap();
    }

    #[test]
    #[ignore = "bounded 8N1 waveform oracle over all bytes and u16 period boundaries"]
    fn uart_waveform_and_terminal_writes() {
        fn check<const PERIOD: u16>() {
            use std::{path::PathBuf, process::Command};
            let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/system-control-uart-contract")
                .join(PERIOD.to_string());
            std::fs::create_dir_all(&directory).unwrap();
            let source = directory.join("device.v");
            let bench = directory.join("tb.v");
            let executable = directory.join("tb.vvp");
            std::fs::write(
                &source,
                SystemControlDevice::<PERIOD>::generated_verilog_source().unwrap(),
            )
            .unwrap();
            std::fs::write(
                &bench,
                include_str!("uart_contract_tb.v")
                    .replace("__CLOCKS_PER_BIT__", &PERIOD.to_string())
                    .replace(
                        "__MODULE__",
                        &SystemControlDevice::<PERIOD>::verilog_identity().module_name(),
                    ),
            )
            .unwrap();
            let compile =
                Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()))
                    .args(["-g2012", "-s", "tb", "-o"])
                    .arg(&executable)
                    .args([source, bench])
                    .output()
                    .unwrap();
            assert!(
                compile.status.success(),
                "{}",
                String::from_utf8_lossy(&compile.stderr)
            );
            let run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()))
                .arg(executable)
                .output()
                .unwrap();
            std::fs::write(directory.join("result.log"), &run.stdout).unwrap();
            assert!(
                run.status.success()
                    && String::from_utf8_lossy(&run.stdout).contains("UART_CONTRACT_PASS"),
                "period {PERIOD}: {} {}",
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&run.stderr)
            );
        }
        check::<1>();
        check::<2>();
        check::<3>();
        check::<469>();
        check::<65535>();
    }
}
