//! Cycle-accurate full-system emulator for CpuV3 performance benchmarks.
//!
//! The system model (core, fetch queue, I-cache, D-cache, memory arbiter, and
//! the cycle-faithful SDRAM word-port model) lives in the shared `system_emu`
//! module; this file keeps the benchmark programs and probes.

mod system_emu;

use system_emu::*;
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
        #[ignore = "calibration helper: prints each suite program's halt signal (see benchmarks/README.md)"]
        fn calibrate_suite_halts() {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/suite");
            let mut paths = read_dir(&root)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension().and_then(|v| v.to_str()) == Some("rs"))
                .collect::<Vec<_>>();
            paths.sort();
            for path in paths {
                let source = read_to_string(&path).unwrap();
                let words = compile(&source);
                let mut machine = cpu_v3::Machine::default();
                machine.load_program(0, &words).unwrap();
                let outcome = machine.run(200_000_000).unwrap();
                let name = path.file_stem().unwrap().to_str().unwrap();
                println!("CALIBRATE {name} -> {outcome:?}");
            }
        }

        #[test]
        #[ignore = "explicit release-mode folder-driven benchmark suite"]
        fn run_benchmark_directory() {
            // The frozen suite is the default; CPU_V3_BENCH_DIR overrides it.
            let input_root = env::var_os("CPU_V3_BENCH_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/suite")
                });
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
                let tier = metadata_value(&source, "bench-tier")
                    .unwrap_or_else(|| panic!("{} lacks bench-tier metadata", path.display()));
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
                let name = format!("{tier}/{name}");
                let words = match path.extension().and_then(|value| value.to_str()) {
                    Some("rs") => compile(&source),
                    Some("hex") => parse_hex(&source, &path),
                    _ => unreachable!(),
                };
                run_words_case(&name, &words, maximum_cycles, true, expected_halt);
            }
        }
    }
}
