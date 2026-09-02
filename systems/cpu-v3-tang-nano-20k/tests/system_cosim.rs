//! System-level emulator-vs-RTL (Icarus) co-simulation for the CpuV3 Tang Nano
//! 20K system: core, instruction fetch queue, two-way I-cache, D-cache, memory
//! arbiter, and the SDRAM word port.
//!
//! Each program runs through the cycle-accurate Rust system model
//! (`system_emu::run_system_trace`) and through Icarus running the composed
//! RTL (`system_cosim_tb.v`), then the per-cycle core-port traces and the
//! post-flush SDRAM check region are compared exactly.

mod system_emu;

use cpu_v3::{
    alu, branch, halt, immediate_unsigned, load, load_immediate16, nop, store, AluOp,
    CpuV3Core, CpuV3DataCache, CpuV3InstructionFetchQueue, CpuV3TwoWayCache, FpuOp, ImmediateOp,
    TestCondition,
};
use cpu_v3::{fpu, fpu_unary, FpuUnaryOp};
use cpu_v3_tang_nano_20k::CpuV3MemoryArbiter;
use digital_design_hardware::{HardwareIdentity, VerilogProject};
use std::collections::BTreeMap;
use std::path::PathBuf;
use system_emu::{compile_cpu_v3_source, run_system_trace, SystemCosimOut};

struct CosimProgram {
    name: &'static str,
    words: Vec<u16>,
    /// Emu-side run bound; the RTL budget derives from the emu trace length.
    max_cycles: usize,
    check_base: usize,
    check_len: usize,
    expected_halt: Option<u16>,
}

/// Dependent immediate chain (GPR forwarding), taken and not-taken branches,
/// an async store that must observe the forwarded `r0`, and a halt signal
/// check — running through the full cache/arbiter/SDRAM path.
fn program_alu_forward_branch() -> Vec<u16> {
    let mut p = Vec::new();
    p.extend(load_immediate16(0, 0)); // r0 = 0
    p.extend(load_immediate16(1, 0x4000)); // r1 = data base
    for _ in 0..5 {
        p.push(immediate_unsigned(ImmediateOp::Add, 0, 1)); // dependent r0 += 1
    }
    p.push(immediate_unsigned(ImmediateOp::CompareUnsigned, 0, 5));
    p.push(branch(TestCondition::Equal, 1)); // taken, skips the nop
    p.push(nop());
    p.push(immediate_unsigned(ImmediateOp::CompareUnsigned, 0, 4));
    p.push(branch(TestCondition::Equal, 1)); // not taken, nop executes
    p.push(nop());
    p.push(alu(AluOp::Add, 2, 0, 1)); // r2 = r0 + r1
    p.push(store(0, 1, 4)); // mem[0x4004] = forwarded r0 = 5
    p.extend(load_immediate16(0, 0x2a));
    p.push(halt());
    p
}

/// Loads and stores, dependent and back-to-back, including D-cache miss
/// write-allocate and dirty eviction: 0x4000/0x4400/0x4800 share cache set 0
/// (set = address[9:4], tag = address[21:10]), so the third store evicts a
/// dirty line and the later loads evict more. Final memory is checked exactly.
fn program_load_store_evict() -> Vec<u16> {
    let mut p = Vec::new();
    p.extend(load_immediate16(1, 0x4000));
    p.extend(load_immediate16(2, 0x4400));
    p.extend(load_immediate16(3, 0x4800));
    p.extend(load_immediate16(0, 0x0011));
    p.push(store(0, 1, 0)); // miss, write-allocate, dirty
    p.extend(load_immediate16(0, 0x0022));
    p.push(store(0, 2, 0)); // miss, second way
    p.extend(load_immediate16(0, 0x0033));
    p.push(store(0, 3, 0)); // miss, dirty eviction of the first line
    p.push(load(4, 1, 0)); // miss: evict + refill, r4 = 0x11
    p.push(load(5, 2, 0)); // miss: evict + refill, r5 = 0x22
    p.push(load(6, 3, 0)); // r6 = 0x33
    p.push(store(4, 1, 4)); // dependent stores of the loaded values
    p.push(store(5, 2, 4));
    p.push(store(6, 3, 4));
    p.extend(load_immediate16(0, 0x2b));
    p.push(halt());
    p
}

/// Async store overlapped with independent ALU work, a dependent load that
/// observes the stored value, and back-to-back stores that must wait on the
/// busy store buffer.
fn program_async_store_overlap() -> Vec<u16> {
    let mut p = Vec::new();
    p.extend(load_immediate16(1, 0x5000));
    p.extend(load_immediate16(0, 0x0077));
    p.push(store(0, 1, 0)); // async store, drains in the background
    for _ in 0..8 {
        p.push(alu(AluOp::Xor, 2, 2, 1)); // independent ALU work
    }
    p.push(load(3, 1, 0)); // dependent load observes the stored 0x77
    p.push(store(3, 1, 2)); // store the loaded value
    p.push(store(0, 1, 4)); // store while the store buffer is busy
    p.extend(load_immediate16(0, 0x2c));
    p.push(halt());
    p
}

/// Integer -> FPU -> integer transitions, FADD/FMUL, and an FSTORE vector
/// store to memory. fix16 with 8 fraction bits: 1.5 + 2.0 = 3.5, 3.5 * 2.0 =
/// 7.0 = 0x0700, exactly representable.
fn program_fpu_roundtrip() -> Vec<u16> {
    let mut p = Vec::new();
    p.extend(load_immediate16(1, 0x6000)); // data base (4-aligned for Export4)
    p.extend(load_immediate16(0, 0x0180)); // 1.5
    p.extend(load_immediate16(2, 0x0200)); // 2.0
    p.push(fpu(FpuOp::Load, 0, 0)); // F0.x = r0 (int -> FPU)
    p.push(fpu(FpuOp::Load, 1, 2)); // F1.x = r2
    p.push(fpu(FpuOp::Move, 3, 0)); // F3 = F0
    p.push(fpu(FpuOp::Add, 3, 1)); // F3.x = 1.5 + 2.0 = 3.5
    p.push(fpu(FpuOp::Mul, 3, 1)); // F3.x = 3.5 * 2.0 = 7.0
    p.push(fpu_unary(4, FpuUnaryOp::Neg)); // F4 = -F4 = 0, barrier variety
    p.push(fpu(FpuOp::Store, 5, 3)); // r5 = F3.x (FPU -> int)
    p.push(fpu(FpuOp::Export4, 3, 1)); // mem[0x6000..0x6004] = F3
    p.push(store(5, 1, 4)); // mem[0x6004] = r5 = 0x0700
    p.extend(load_immediate16(0, 0x2d));
    p.push(halt());
    p
}

/// I-cache pressure: an rcc-compiled loop whose body spans multiple 16-word
/// lines, with the loop back-branch crossing line boundaries.
const ICACHE_LOOP_SOURCE: &str = r#"
fn main() {
    let mut i: u16 = 0;
    let mut a: u16 = 1;
    let mut b: u16 = 7;
    let mut c: u16 = 3;
    let mut d: u16 = 5;
    let mut e: u16 = 9;
    let mut f: u16 = 11;
    while i < 6 {
        a = a + b; b = b ^ a; c = c + d; d = d ^ c; e = e + f; f = f ^ e;
        a = a + c; b = b ^ d; c = c + e; d = d ^ f; e = e + a; f = f ^ b;
        i = i + 1;
    }
    if a == 0 { halt(7); } else { halt(a & 255); }
}
"#;

/// Straight-line pipeline-overlap program: a long dependent immediate chain
/// that must retire one instruction per cycle once resident in the fetch
/// queue and I-cache.
fn program_pipeline_overlap() -> Vec<u16> {
    let mut p = Vec::new();
    p.extend(load_immediate16(0, 0));
    for _ in 0..60 {
        p.push(immediate_unsigned(ImmediateOp::Add, 0, 1));
    }
    p.push(halt()); // halt signal = r0 = 60
    p
}

/// An rcc-compiled FPU program: vec4 constructors, a mixed-signature call
/// with FPU argument and return registers, the ACC splat multiply, fdot, and
/// an aligned FEXPORT4 through the D-cache.
const FPU_COMPILER_SOURCE: &str = r#"
use crate::dsl_rt::*;
static OUT: [u16; 4] = [0; 4];

fn scaled(v: vec4, factor: fix16, tag: u16) -> vec4 {
    if tag == 1 { v * factor } else { v }
}

fn main() {
    let a = vec4::new(
        fix16::from_int(1),
        fix16::from_int(2),
        fix16::from_int(3),
        fix16::from_int(4),
    );
    let b = scaled(a, fix16::from_bits(0x0080), 1); // halves every lane
    let d = fdot(a, b); // 0.5 + 2 + 4.5 + 8 = 15.0
    vec4::export(b, OUT.as_array().as_ptr());
    if d.to_bits() == 3840 { halt(1); } else { halt(0); }
}
"#;

fn programs() -> Vec<CosimProgram> {
    vec![
        CosimProgram {
            name: "alu_forward_branch",
            words: program_alu_forward_branch(),
            max_cycles: 20_000,
            check_base: 0x4000,
            check_len: 16,
            expected_halt: Some(0x2a),
        },
        CosimProgram {
            name: "load_store_evict",
            words: program_load_store_evict(),
            max_cycles: 20_000,
            check_base: 0x4000,
            check_len: 0x810,
            expected_halt: Some(0x2b),
        },
        CosimProgram {
            name: "async_store_overlap",
            words: program_async_store_overlap(),
            max_cycles: 20_000,
            check_base: 0x5000,
            check_len: 16,
            expected_halt: Some(0x2c),
        },
        CosimProgram {
            name: "fpu_roundtrip",
            words: program_fpu_roundtrip(),
            max_cycles: 20_000,
            check_base: 0x6000,
            check_len: 8,
            expected_halt: Some(0x2d),
        },
        CosimProgram {
            name: "fpu_compiler_rcc",
            words: compile_cpu_v3_source(FPU_COMPILER_SOURCE),
            max_cycles: 20_000,
            check_base: 0x4000,
            check_len: 4,
            expected_halt: Some(1),
        },
        CosimProgram {
            name: "icache_loop",
            words: compile_cpu_v3_source(ICACHE_LOOP_SOURCE),
            max_cycles: 50_000,
            check_base: 0x4000,
            check_len: 0,
            expected_halt: None,
        },
        CosimProgram {
            name: "pipeline_overlap",
            words: program_pipeline_overlap(),
            max_cycles: 20_000,
            check_base: 0x4000,
            check_len: 0,
            expected_halt: Some(60),
        },
    ]
}

/// Collects the composed RTL sources: per-module `VerilogProject::generate`
/// output merged with dedup by file content (the two cache projects share the
/// dual-port data RAM and tag RAM leaves). Generated testbench files (`tb.v`)
/// are skipped.
fn system_verilog_sources() -> Vec<String> {
    let mut sources: Vec<String> = Vec::new();
    let mut append = |files: &BTreeMap<PathBuf, String>| {
        for (path, source) in files {
            if path.file_name().and_then(|name| name.to_str()) == Some("tb.v")
                || source.contains("module tb")
            {
                continue;
            }
            if !sources.contains(source) {
                sources.push(source.clone());
            }
        }
    };
    append(&VerilogProject::generate::<CpuV3Core>().unwrap().files);
    append(&VerilogProject::generate::<CpuV3InstructionFetchQueue>().unwrap().files);
    append(&VerilogProject::generate::<CpuV3TwoWayCache>().unwrap().files);
    append(&VerilogProject::generate::<CpuV3DataCache>().unwrap().files);
    append(&VerilogProject::generate::<CpuV3MemoryArbiter>().unwrap().files);
    sources
}

fn build_tb(program: &CosimProgram, max_cycles: usize) -> String {
    let mut memory_init = String::new();
    for (index, word) in program.words.iter().copied().enumerate() {
        memory_init.push_str(&format!("    memory[{index}] = 16'h{word:04x};\n"));
    }
    include_str!("system_cosim_tb.v")
        .replace(
            "__CORE__",
            &CpuV3Core::verilog_identity().module_name(),
        )
        .replace(
            "__FETCH__",
            &CpuV3InstructionFetchQueue::verilog_identity().module_name(),
        )
        .replace(
            "__ICACHE__",
            &CpuV3TwoWayCache::verilog_identity().module_name(),
        )
        .replace(
            "__DCACHE__",
            &CpuV3DataCache::verilog_identity().module_name(),
        )
        .replace(
            "__ARBITER__",
            &CpuV3MemoryArbiter::verilog_identity().module_name(),
        )
        .replace("__MEMORY_INIT__", &memory_init)
        .replace("__CHECK_BASE__", &program.check_base.to_string())
        .replace("__CHECK_LEN__", &program.check_len.to_string())
        .replace("__MAX_CYCLES__", &max_cycles.to_string())
        // The trace budget plus a generous allowance for the post-halt flush
        // (up to 128 dirty line writebacks through the SDRAM model).
        .replace("__TIMEOUT_CYCLES__", &(max_cycles * 4 + 20_000).to_string())
}

struct RtlRun {
    cycles: Vec<SystemCosimOut>,
    memory: BTreeMap<usize, u16>,
    halted: bool,
}

fn run_system_rtl(program: &CosimProgram, sources: &[String], max_cycles: usize) -> RtlRun {
    let directory = std::env::temp_dir().join(format!(
        "system-cosim-{}-{}",
        std::process::id(),
        program.name
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let mut module_paths = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let path = directory.join(format!("src_{index}.v"));
        std::fs::write(&path, source).unwrap();
        module_paths.push(path);
    }
    let tb_path = directory.join("tb.v");
    std::fs::write(&tb_path, build_tb(program, max_cycles)).unwrap();

    let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let output_path = directory.join("sim.vvp");
    let mut compile = std::process::Command::new(&iverilog);
    compile
        .current_dir(&directory)
        .args(["-g2005", "-s", "tb", "-o"])
        .arg(&output_path);
    for path in &module_paths {
        compile.arg(path);
    }
    compile.arg(&tb_path);
    let compile_output = compile.output().unwrap();
    assert!(
        compile_output.status.success(),
        "iverilog compile failed:\n{}",
        String::from_utf8_lossy(&compile_output.stderr)
    );
    let simulation = std::process::Command::new(&vvp)
        .current_dir(&directory)
        .arg(&output_path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&simulation.stdout);
    assert!(
        !stdout.lines().any(|line| line.trim() == "TIMEOUT"),
        "RTL simulation timed out:\n{}",
        String::from_utf8_lossy(&simulation.stderr)
    );

    let mut run = RtlRun {
        cycles: Vec::new(),
        memory: BTreeMap::new(),
        halted: false,
    };
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("CORE ") {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            assert_eq!(fields.len(), 18, "unexpected CORE line: {line}");
            let num = |i: usize| fields[i].parse().unwrap_or(0);
            run.cycles.push(SystemCosimOut {
                pc: num(1) as u16,
                code_segment: num(2) as u16,
                data_segment: num(3) as u16,
                retired_words: num(4) as u32,
                halted: num(5) == 1,
                halt_signal: num(6) as u16,
                fault: num(7) == 1,
                fault_code: num(8) as u8,
                fault_pc: num(9) as u16,
                instruction_request_valid: num(10) == 1,
                instruction_address: num(11) as u32,
                instruction_response_ready: num(12) == 1,
                data_request_valid: num(13) == 1,
                data_write: num(14) == 1,
                data_address: num(15) as u32,
                data_write_data: num(16) as u16,
                data_response_ready: num(17) == 1,
            });
        } else if let Some(rest) = line.strip_prefix("MEM ") {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            assert_eq!(fields.len(), 2, "unexpected MEM line: {line}");
            let address: usize = fields[0].parse().unwrap();
            let value = u16::from_str_radix(fields[1], 16).unwrap();
            run.memory.insert(address, value);
        } else if line == "TRACE_END" {
            break;
        }
    }
    run.halted = run.cycles.last().is_some_and(|last| last.halted);
    std::fs::remove_dir_all(&directory).ok();
    run
}

fn compare_program(program: &CosimProgram, sources: &[String]) -> Result<(), String> {
    let emu = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_system_trace(&program.words, program.max_cycles)
    }))
    .map_err(|panic| {
        format!(
            "emu run panicked: {}",
            panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic")
        )
    })?;
    if !emu.halted {
        return Err("emu did not halt".to_string());
    }
    if let Some(expected) = program.expected_halt {
        if emu.halt_signal != expected {
            return Err(format!(
                "emu halt signal {:#06x} != expected {expected:#06x}",
                emu.halt_signal
            ));
        }
    }

    let rtl = run_system_rtl(program, sources, emu.cycles.len() + 2000);
    if !rtl.halted {
        return Err(format!(
            "RTL did not halt (emu trace {} cycles, rtl trace {} cycles)",
            emu.cycles.len(),
            rtl.cycles.len()
        ));
    }
    let common = emu.cycles.len().min(rtl.cycles.len());
    for index in 0..common {
        let expected = &emu.cycles[index];
        let actual = &rtl.cycles[index];
        if !actual.equal_core(expected) {
            return Err(format!(
                "trace mismatch at cycle {index} (emu len {}, rtl len {})\nemu={expected:?}\nrtl={actual:?}",
                emu.cycles.len(),
                rtl.cycles.len()
            ));
        }
    }
    if emu.cycles.len() != rtl.cycles.len() {
        return Err(format!(
            "trace length mismatch: emu={} rtl={}\nemu pcs={:?}\nrtl pcs={:?}",
            emu.cycles.len(),
            rtl.cycles.len(),
            emu.cycles.iter().map(|v| v.pc).collect::<Vec<_>>(),
            rtl.cycles.iter().map(|v| v.pc).collect::<Vec<_>>()
        ));
    }
    for (index, (expected, actual)) in emu.cycles.iter().zip(&rtl.cycles).enumerate() {
        if !actual.equal_core(expected) {
            return Err(format!(
                "trace mismatch at cycle {index}\nemu={expected:?}\nrtl={actual:?}"
            ));
        }
    }
    for offset in 0..program.check_len {
        let address = program.check_base + offset;
        let expected = emu.memory[address];
        let actual = rtl
            .memory
            .get(&address)
            .copied()
            .ok_or_else(|| format!("RTL memory dump missed address {address:#06x}"))?;
        if actual != expected {
            return Err(format!(
                "memory mismatch at {address:#06x}: emu={expected:#06x} rtl={actual:#06x}"
            ));
        }
    }
    Ok(())
}

#[test]
#[ignore = "explicit emulator-vs-Icarus co-simulation of the full CpuV3 system"]
fn system_emu_matches_rtl() {
    let programs = programs();
    let sources = system_verilog_sources();
    let mut failures = Vec::new();
    for program in &programs {
        match compare_program(program, &sources) {
            Ok(()) => println!("PASS {}", program.name),
            Err(message) => {
                println!("FAIL {}: {message}", program.name);
                failures.push(format!("{}: {message}", program.name));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "system co-simulation failures:\n{}",
        failures.join("\n")
    );
}
