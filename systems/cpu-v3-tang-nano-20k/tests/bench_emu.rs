//! Cycle-accurate full-system emulator for CpuV3 performance benchmarks.
//!
//! The system model (core, fetch queue, I-cache, D-cache, memory arbiter, and
//! the cycle-faithful SDRAM word-port model) lives in the shared `system_emu`
//! module; this file keeps the benchmark programs and probes.

mod system_emu;

use system_emu::*;

use cpu_v3::{
    alu, fpu, fpu_unary, halt, jump_relative, load_immediate16, nop, AluOp, FpuOp, FpuUnaryOp,
};
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs::{read_dir, read_to_string};
    use std::path::PathBuf;

    const CONTROL_FLOW_SOURCE: &str = r#"
fn leaf(x: u16) -> u16 {
    x + 1
}

fn main() {
    let mut i: u16 = 0;
    let mut sum: u16 = 0;
    while i < 256 {
        sum = leaf(sum);
        i = i + 1;
    }
    if sum == 256 {
        halt(1);
    } else {
        halt(0);
    }
}
"#;

    const DATA_SOURCE: &str = r#"
use crate::dsl_rt::*;

const N: u16 = 128;
static DATA: [u16; 128] = [0; 128];

fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    while i < N {
        d[i] = i ^ 0x5a5a;
        i = i + 1;
    }
    let mut sum: u16 = 0;
    i = 0;
    while i < N {
        sum = sum + d[i];
        i = i + 1;
    }
    if sum != 0 {
        halt(1);
    } else {
        halt(0);
    }
}
"#;

    fn compile(source: &str) -> Vec<u16> {
        compile_cpu_v3_source(source)
    }

    fn trace_directory(name: &str) -> PathBuf {
        env::var_os("CPU_V3_BENCH_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join("target/cpu-v3-bench")
            })
            .join(name)
    }

    #[test]
    fn control_flow_probe_records_calls_returns_and_taken_loop_edges() {
        let words = compile(CONTROL_FLOW_SOURCE);
        let trace_directory = trace_directory("control-flow");
        let result = run_benchmark_profiled(&words, 1_000_000, Some(&trace_directory));
        assert_eq!(result.halt_signal, 1);
        assert!(result.redirect_count >= 3 * 256);
        assert!(result.redirect_wait_histogram[2] >= 3 * 256);
    }

    #[test]
    fn data_probe_counts_overlapped_scalar_requests_and_latency() {
        let words = compile(DATA_SOURCE);
        let trace_directory = trace_directory("data");
        let result = run_benchmark_profiled(&words, 1_000_000, Some(&trace_directory));
        assert_eq!(result.halt_signal, 1);
        assert!(result.dcache_line_requests >= 8);
        let scalar_requests = result.opcode_retired[8] + result.opcode_retired[9];
        assert_eq!(result.data_requests, scalar_requests);
        assert!(result.store_latency_cycles > 0);
    }

    #[test]
    fn smoke_halt_runs_to_completion() {
        let words = compile("fn main() { halt(7); }");
        let result = run_benchmark(&words, 100_000);
        println!(
            "smoke halt={} cycles={} retired={}",
            result.halt_signal, result.cycles, result.retired_words
        );
        assert_eq!(result.halt_signal, 7);
    }

    #[allow(dead_code)]
    mod benchmark_suite {
        use super::*;

        const QUICKSORT_SOURCE: &str = include_str!("../benchmarks/algorithms/quicksort.rs");

        const INT_SHORT_ALU_SOURCE: &str = r#"
fn main() {
    let mut x: u16 = 0x1357;
    let mut y: u16 = 0x2468;
    let mut i: u16 = 0;
    while i < 24 {
        x = (x + y) ^ (x << 1);
        y = (y + 3) ^ (x >> 2);
        i = i + 1;
    }
    halt(1);
}
"#;

        const INT_SHORT_BRANCH_SOURCE: &str = r#"
fn main() {
    let mut x: u16 = 0;
    let mut i: u16 = 0;
    while i < 48 {
        if (i & 3) == 0 { x = x + 7; }
        else if (i & 1) == 0 { x = x ^ i; }
        else { x = x - 1; }
        i = i + 1;
    }
    halt(1);
}
"#;

        const INT_SHORT_MEMORY_SOURCE: &str = r#"
use crate::dsl_rt::*;
static DATA: [u16; 48] = [0; 48];
fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    let mut sum: u16 = 0;
    while i < 48 { d[i] = ((i << 3) + i) ^ 0x55aa; i = i + 1; }
    i = 0;
    while i < 48 { sum = sum + d[i]; i = i + 1; }
    if sum != 0 { halt(1); } else { halt(0); }
}
"#;

        const INT_SHORT_MIXED_SOURCE: &str = r#"
fn mix(x0: u16, n: u16) -> u16 {
    let mut x: u16 = x0;
    let mut i: u16 = 0;
    while i < n {
        x = ((x << 3) ^ (x >> 2)) + i + 0x1234;
        if (x & 7) == 3 { x = x ^ 0xa5a5; }
        i = i + 1;
    }
    x
}
fn main() { let x = mix(7, 24); if x != 0 { halt(1); } else { halt(0); } }
"#;

        const INT_MEDIUM_ALU_SOURCE: &str = r#"
fn main() {
    let mut x: u16 = 1;
    let mut y: u16 = 0x9e37;
    let mut i: u16 = 0;
    while i < 1536 {
        x = (x + y) ^ (x << 5) ^ (x >> 3);
        y = y + x + i;
        i = i + 1;
    }
    halt(1);
}
"#;

        const INT_MEDIUM_MEMORY_SOURCE: &str = r#"
use crate::dsl_rt::*;
static DATA: [u16; 1024] = [0; 1024];
fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    let mut sum: u16 = 0;
    while i < 1024 { d[i] = (i << 3) ^ (i >> 2) ^ 0x6d2b; i = i + 1; }
    i = 0;
    while i < 1024 { sum = sum + d[i]; i = i + 1; }
    if sum != 0 { halt(1); } else { halt(0); }
}
"#;

        const STREAMING_MIX_SOURCE: &str = r#"
use crate::dsl_rt::*;
const N: u16 = 4096;
static INPUT_A: [u16; 4096] = [0; 4096];
static INPUT_B: [u16; 4096] = [0; 4096];
static OUTPUT: [u16; 4096] = [0; 4096];
fn main() {
    let mut a = INPUT_A.as_array();
    let mut b = INPUT_B.as_array();
    let mut out = OUTPUT.as_array();
    let mut i: u16 = 0;
    while i < N { a[i] = i ^ 0x5a5a; b[i] = (i << 1) + 3; i = i + 1; }
    i = 0;
    while i < N {
        out[i] = (a[i] + b[i]) ^ (a[i] >> 3) ^ (b[i] << 2);
        i = i + 1;
    }
    i = 0;
    while i < N {
        if out[i] != ((a[i] + b[i]) ^ (a[i] >> 3) ^ (b[i] << 2)) { halt(0); }
        i = i + 1;
    }
    halt(1);
}
"#;

        const STREAMING_BALANCED_SOURCE: &str = r#"
use crate::dsl_rt::*;
const N: u16 = 4096;
const B_OFFSET: u16 = 4112;
const OUT_OFFSET: u16 = 8224;
static DATA: [u16; 12320] = [0; 12320];
fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    while i < N {
        d[i] = i ^ 0x5a5a;
        d[B_OFFSET + i] = (i << 1) + 3;
        i = i + 1;
    }
    i = 0;
    while i < N {
        d[OUT_OFFSET + i] =
            (d[i] + d[B_OFFSET + i]) ^ (d[i] >> 3) ^ (d[B_OFFSET + i] << 2);
        i = i + 1;
    }
    i = 0;
    while i < N {
        let expected =
            (d[i] + d[B_OFFSET + i]) ^ (d[i] >> 3) ^ (d[B_OFFSET + i] << 2);
        if d[OUT_OFFSET + i] != expected { halt(0); }
        i = i + 1;
    }
    halt(1);
}
"#;

        fn generated_int_icache_jump() -> Vec<u16> {
            let mut words = Vec::new();
            words.extend(load_immediate16(1, 0x1357));
            words.extend(load_immediate16(2, 0x2468));
            for _ in 0..144 {
                for lane in 0..10 {
                    words.push(alu(AluOp::Add, 1, 1, 2));
                    words.push(alu(AluOp::Xor, 2, 2, 1));
                    words.push(alu(AluOp::ShiftLeft, 1, 1, 2 + (lane & 1)));
                }
                words.push(jump_relative(1));
                words.push(nop());
            }
            words.extend(load_immediate16(0, 1));
            words.push(halt());
            words
        }

        fn generated_fpu_short(op: FpuOp, left: u16, right: u16) -> Vec<u16> {
            let mut words = Vec::new();
            words.extend(load_immediate16(1, left));
            words.extend(load_immediate16(2, right));
            words.push(fpu(FpuOp::Load, 0, 1));
            words.push(fpu(FpuOp::Load, 1, 2));
            words.push(fpu(op, 0, 1));
            words.push(fpu(FpuOp::Store, 0, 0));
            words.push(halt());
            words
        }

        fn generated_fpu_unary() -> Vec<u16> {
            let mut words = Vec::new();
            words.extend(load_immediate16(1, 0xff00));
            words.push(fpu(FpuOp::Load, 0, 1));
            words.push(fpu_unary(0, FpuUnaryOp::Abs));
            words.push(fpu(FpuOp::Store, 0, 0));
            words.push(halt());
            words
        }

        fn generated_fpu_long() -> Vec<u16> {
            let mut words = Vec::new();
            words.extend(load_immediate16(1, 256));
            words.extend(load_immediate16(2, 1));
            words.push(fpu(FpuOp::Load, 0, 1));
            words.push(fpu(FpuOp::Load, 1, 2));
            for index in 0..3072 {
                words.push(fpu(
                    if index & 1 == 0 {
                        FpuOp::Add
                    } else {
                        FpuOp::Sub
                    },
                    0,
                    1,
                ));
            }
            words.push(fpu(FpuOp::Store, 0, 0));
            words.push(halt());
            words
        }

        fn run_case(name: &str, source: &str, maximum_cycles: usize, prefetch_enabled: bool) {
            let words = compile(source);
            run_words_case(name, &words, maximum_cycles, prefetch_enabled, 1);
        }

        fn run_words_case(
            name: &str,
            words: &[u16],
            maximum_cycles: usize,
            prefetch_enabled: bool,
            expected_halt: u16,
        ) {
            let trace_name = if prefetch_enabled {
                name.to_string()
            } else {
                format!("stage2-{name}")
            };
            let trace_directory = trace_directory(&trace_name);
            let result = run_benchmark_profiled_with_prefetch(
                words,
                maximum_cycles,
                Some(&trace_directory),
                prefetch_enabled,
            );
            assert_eq!(
                result.halt_signal, expected_halt,
                "{name} self-check failed"
            );
            let loads = result.opcode_retired[8];
            let stores = result.opcode_retired[9];
            let retired_words = result.retired_words.max(1) as f64;
            let fetch_wait_percent = 100.0 * result.fetch_wait_cycles as f64 / result.cycles as f64;
            let data_path_percent = 100.0
                * (result.data_request_cycles + result.data_response_cycles) as f64
                / result.cycles as f64;
            let avg_load = if loads == 0 {
                0.0
            } else {
                result.load_latency_cycles as f64 / f64::from(loads)
            };
            let avg_store = if stores == 0 {
                0.0
            } else {
                result.store_latency_cycles as f64 / f64::from(stores)
            };
            println!(
            "BENCH name={name} program_words={} cycles={} retired_words={} cpi={:.6} cpw={:.6} fetch_wait_pct={fetch_wait_percent:.3} data_path_pct={data_path_percent:.3} data_req_cycles={} data_resp_cycles={} data_requests={} loads={loads} stores={stores} avg_load_latency={avg_load:.3} avg_store_latency={avg_store:.3} dcache_word_requests={} dcache_line_requests={} icache_line_requests={} redirects={} redirect_wait={} prefetch_issued={} prefetch_useful={} prefetch_useless={} prefetch_dropped={} refreshes={}",
            result.program_words,
            result.cycles,
            result.retired_words,
            result.cycles as f64 / retired_words,
            result.cycles as f64 / retired_words,
            result.data_request_cycles,
            result.data_response_cycles,
            result.data_requests,
            result.dcache_word_requests,
            result.dcache_line_requests,
            result.icache_line_requests,
            result.redirect_count,
            result.redirect_wait_cycles,
            result.prefetch_issued,
            result.prefetch_useful,
            result.prefetch_useless,
            result.prefetch_dropped,
            result.refreshes,
        );
        }

        fn metadata_value(source: &str, key: &str) -> Option<String> {
            source.lines().find_map(|line| {
                let line = line
                    .trim()
                    .strip_prefix("//")
                    .or_else(|| line.trim().strip_prefix('#'))?
                    .trim();
                let (candidate, value) = line.split_once(':')?;
                (candidate.trim() == key).then(|| value.trim().to_string())
            })
        }

        fn parse_hex(source: &str, path: &Path) -> Vec<u16> {
            source
                .lines()
                .filter_map(|line| {
                    let word = line.split('#').next().unwrap_or_default().trim();
                    (!word.is_empty()).then_some(word)
                })
                .map(|word| {
                    u16::from_str_radix(word.trim_start_matches("0x"), 16).unwrap_or_else(|error| {
                        panic!("invalid word {word:?} in {}: {error}", path.display())
                    })
                })
                .collect()
        }

        #[test]
        #[ignore = "explicit release-mode folder-driven benchmark suite"]
        fn run_benchmark_directory() {
            let input_root = env::var_os("CPU_V3_BENCH_DIR")
                .map(PathBuf::from)
                .expect("set CPU_V3_BENCH_DIR to a benchmark program directory");
            let mut paths = read_dir(&input_root)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", input_root.display()))
                .map(|entry| entry.expect("cannot read benchmark directory entry").path())
                .filter(|path| {
                    matches!(
                        path.extension().and_then(|value| value.to_str()),
                        Some("rs" | "hex")
                    )
                })
                .collect::<Vec<_>>();
            paths.sort();
            assert!(
                !paths.is_empty(),
                "{} contains no .rs or .hex benchmarks",
                input_root.display()
            );

            for path in paths {
                let source = read_to_string(&path)
                    .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
                let maximum_cycles = metadata_value(&source, "bench-max-cycles")
                    .unwrap_or_else(|| panic!("{} lacks bench-max-cycles metadata", path.display()))
                    .parse::<usize>()
                    .unwrap_or_else(|error| {
                        panic!("invalid bench-max-cycles in {}: {error}", path.display())
                    });
                let expected_halt = metadata_value(&source, "bench-expected-halt")
                    .unwrap_or_else(|| "1".to_string())
                    .parse::<u16>()
                    .unwrap_or_else(|error| {
                        panic!("invalid bench-expected-halt in {}: {error}", path.display())
                    });
                let name = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .expect("benchmark filename must be UTF-8");
                let words = match path.extension().and_then(|value| value.to_str()) {
                    Some("rs") => compile(&source),
                    Some("hex") => parse_hex(&source, &path),
                    _ => unreachable!(),
                };
                run_words_case(name, &words, maximum_cycles, true, expected_halt);
            }
        }

        fn run_suite(prefetch_enabled: bool) {
            run_case(
                "int-short-alu",
                INT_SHORT_ALU_SOURCE,
                10_000,
                prefetch_enabled,
            );
            run_case(
                "int-short-branch",
                INT_SHORT_BRANCH_SOURCE,
                10_000,
                prefetch_enabled,
            );
            run_case(
                "int-short-memory",
                INT_SHORT_MEMORY_SOURCE,
                10_000,
                prefetch_enabled,
            );
            run_case(
                "int-short-mixed",
                INT_SHORT_MIXED_SOURCE,
                10_000,
                prefetch_enabled,
            );
            run_case(
                "int-medium-alu",
                INT_MEDIUM_ALU_SOURCE,
                400_000,
                prefetch_enabled,
            );
            run_case(
                "int-medium-memory",
                INT_MEDIUM_MEMORY_SOURCE,
                600_000,
                prefetch_enabled,
            );
            run_case(
                "quicksort-4096",
                QUICKSORT_SOURCE,
                30_000_000,
                prefetch_enabled,
            );
            run_words_case(
                "int-icache-jump",
                &generated_int_icache_jump(),
                1_000_000,
                prefetch_enabled,
                1,
            );
            run_words_case(
                "fpu-short-add",
                &generated_fpu_short(FpuOp::Add, 256, 512),
                10_000,
                prefetch_enabled,
                768,
            );
            run_words_case(
                "fpu-short-mul",
                &generated_fpu_short(FpuOp::Mul, 384, 512),
                10_000,
                prefetch_enabled,
                768,
            );
            run_words_case(
                "fpu-short-unary",
                &generated_fpu_unary(),
                10_000,
                prefetch_enabled,
                256,
            );
            run_words_case(
                "fpu-long-mixed",
                &generated_fpu_long(),
                1_000_000,
                prefetch_enabled,
                256,
            );
            run_case(
                "streaming-mix",
                STREAMING_MIX_SOURCE,
                8_000_000,
                prefetch_enabled,
            );
            run_case(
                "streaming-balanced",
                STREAMING_BALANCED_SOURCE,
                8_000_000,
                prefetch_enabled,
            );
        }
    }
}
