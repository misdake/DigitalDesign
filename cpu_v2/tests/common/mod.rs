#![allow(dead_code)]
//! shared helpers for the rcc integration tests

use cpu_v2::frontend::parse_source;
use cpu_v2::{Compiler, SimState};

/// compile an rcc source string and run `main` on the simulator (cycle-capped,
/// so a runaway program cannot hang the suite)
pub fn compile_and_run(src: &str, main: &'static str, max_cycles: usize) -> (SimState, Option<u16>) {
    let funcs = parse_source(src).expect("parse failed");
    let mut c = Compiler::new();
    for f in funcs {
        c.add_func(f);
    }
    let (instructions, _) = c.finish(main);
    cpu_v2::simulate(&instructions, max_cycles)
}

/// just the halt signal of `compile_and_run`
pub fn run(src: &str) -> Option<u16> {
    compile_and_run(src, "main", 10_000).1
}

/// expect `src` to fail compilation with an error containing `needle`
pub fn expect_error(src: &str, needle: &str) {
    match parse_source(src) {
        Ok(_) => panic!("expected error containing `{needle}`, got Ok"),
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains(needle),
                "error `{msg}` should contain `{needle}`"
            );
        }
    }
}
