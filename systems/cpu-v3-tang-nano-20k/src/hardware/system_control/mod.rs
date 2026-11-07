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
//! - channel 11 (read): microsecond interval, saturating at 65535; a write
//!   with bit 0 set clears its count and prescaler. Reads have no side effects.
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
pub use crate::boot::SYSCTL_MICROSECONDS as SYSTEM_CONTROL_CHANNEL_MICROSECONDS;
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
    microsecond_divider: u16,
    microseconds: u16,
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
            microsecond_divider: 0,
            microseconds: 0,
        }
    }
}

impl SystemControlDeviceState {
    pub(crate) fn read_channel(&self, channel: u8) -> u16 {
        match channel {
            SYSTEM_CONTROL_CHANNEL_UART => u16::from(self.uart_busy),
            SYSTEM_CONTROL_CHANNEL_CACHE_MAINTENANCE_STATUS => self.cache_maintenance_status,
            SYSTEM_CONTROL_CHANNEL_MILLISECONDS => self.milliseconds,
            SYSTEM_CONTROL_CHANNEL_MICROSECONDS => self.microseconds,
            _ => 0,
        }
    }
    pub(crate) fn active_watch(&self) -> Option<(u8, u8, u16)> {
        self.watch_active.then_some((
            self.watch_device_index,
            self.watch_device_channel,
            self.watch_expected,
        ))
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
    clocks_per_us_minus_one: u16,
    microsecond_divider_msb: u32,
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
            state.read_channel(input.device_channel as u8)
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
        let microsecond_period = (CLOCKS_PER_MS / 1000).max(1);
        if state.microsecond_divider == microsecond_period - 1 {
            state.microsecond_divider = 0;
            state.microseconds = state.microseconds.saturating_add(1);
        } else {
            state.microsecond_divider += 1;
        }
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
            SYSTEM_CONTROL_CHANNEL_MICROSECONDS if value & 1 != 0 => {
                state.microseconds = 0;
                state.microsecond_divider = 0;
            }
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
                clocks_per_us_minus_one: (CLOCKS_PER_MS / 1000).max(1) - 1,
                microsecond_divider_msb: (u16::BITS
                    - ((CLOCKS_PER_MS / 1000).max(1) - 1).leading_zeros())
                .max(1)
                    - 1,
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
mod tests;
