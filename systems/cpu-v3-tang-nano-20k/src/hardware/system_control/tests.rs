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
    for ch in [0, 1, 2, 4, 6, 7, 8, 9, 12, 13, 14, 15] {
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
fn microseconds_emulator_saturates_and_clear_wins_over_tick() {
    ModuleTest::<SystemControlDevice<4, 1000>>::new(vec![
        TestStep::new(
            SystemControlDeviceInputValue {
                reset: true,
                ..IDLE
            },
            output(0, false, false, 0, true),
        ),
        TestStep::new(read(11), output(1, false, false, 0, true)),
        TestStep::new(read(11), output(65535, false, false, 0, true)).after_cycles(65540),
        TestStep::new(write(11, 2), output(0, false, false, 0, true)),
        TestStep::new(read(11), output(65535, false, false, 0, true)),
        TestStep::new(write(11, 1), output(0, false, false, 0, true)),
        TestStep::new(read(11), output(1, false, false, 0, true)),
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
