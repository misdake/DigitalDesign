//! Bounded check of the actual RCC wire encoder, independent of board timing.
#[path = "../build_support/rcc_modules.rs"]
#[allow(dead_code)]
mod resolver;

use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
use cpu_v3_tang_nano_20k::boot::SystemControlDevice;
use std::path::Path;

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0xffffu16;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[test]
fn native_rcc_packets_match_wire_crc_and_saturation_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let local = root.join("examples/gpu_bench/rcc");
    let source = std::fs::read_to_string(local.join("main.rs")).unwrap();
    let constants = "use crate::dsl_rt::*;pub const BENCH_CASES:u16=1;pub const BENCH_TOKEN:u16=47794;pub const BENCH_WARMUP:u16=3;pub const BENCH_SAMPLES:u16=31;pub const BENCH_PREVIEW_SECTOR:u16=34304;pub static BENCH_TABLE:Buf<u16,4>=Buf::new([0;4]);";
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = cpu_v3_tang_nano_20k::boot::SINGLE_BOOT_STACK;
    let parsed = rcc::frontend::compile_program_named(
        "GPU benchmark protocol",
        &source,
        &options,
        &mut |name| resolver::load_module(&local, &root.join("rcc"), constants, name, &mut |_| {}),
    )
    .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "bench_protocol_main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    cpu.attach_device(0, Box::<SystemControlDevice>::default());
    let mut halted = false;
    for _ in 0..200_000 {
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(signal, 0x600d);
            halted = true;
            break;
        }
    }
    assert!(halted, "protocol encoder exceeded 200000 CPU steps");
    let bytes = &cpu.device::<SystemControlDevice>(0).unwrap().uart;
    assert_eq!(bytes.len(), 40);
    for (frame, expected) in bytes.as_chunks::<20>().0.iter().zip([
        (1u8, 7u16, 3u16, 12345u16, 12u16, 0u16),
        (5u8, 9u16, 30u16, 65535u16, 67u16, 5u16),
    ]) {
        assert_eq!(&frame[..5], b"GBEN\x01");
        assert_eq!(frame[5], expected.0);
        let fields: Vec<u16> = frame[6..18]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        assert_eq!(
            fields,
            [expected.1, expected.2, expected.3, expected.4, expected.5, 47794]
        );
        assert_eq!(
            u16::from_le_bytes([frame[18], frame[19]]),
            crc16(&frame[..18])
        );
    }
}
