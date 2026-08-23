//! Semantic smoke tests for the small programs used in compiler comparisons.

mod common;

#[test]
fn test_representative_benchmarks() {
    let opts = cpu_v2::CompilerOptions::default();
    let source = include_str!("../src/dsl_progs/benchmark_suite_dsl.rs");
    assert_eq!(
        common::compile_program_and_run(source, &opts, 20_000).1,
        Some(52_286)
    );
}
