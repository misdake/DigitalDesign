#![allow(dead_code)]
//! shared helpers for the rcc integration tests

use cpu_v2::frontend::parse_source;
use cpu_v2::{Compiler, SimState};

/// compile an rcc source string and run `main` on the simulator (cycle-capped,
/// so a runaway program cannot hang the suite)
pub fn compile_and_run(
    src: &str,
    main: &'static str,
    max_cycles: usize,
) -> (SimState, Option<u16>) {
    let program = parse_source(src).expect("parse failed");
    let mut c = Compiler::new();
    for f in program.funcs {
        c.add_func(f);
    }
    let (instructions, _) = c.finish(main);
    cpu_v2::simulate(&instructions, max_cycles)
}

/// compile with the std library + compile options, and run
pub fn compile_program_and_run(
    src: &str,
    opts: &cpu_v2::CompilerOptions,
    max_cycles: usize,
) -> (SimState, Option<u16>, String) {
    let program = cpu_v2::frontend::compile_program(src, opts, &mut |name| {
        Err(format!("unknown module `{name}`"))
    })
    .expect("parse failed");
    let mut c = Compiler::new();
    c.opts = opts.clone();
    for f in program.funcs {
        c.add_func(f);
    }
    let (instructions, listing) = c.finish("main");
    let (_state, signal) = cpu_v2::simulate(&instructions, max_cycles);
    (_state, signal, listing)
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
