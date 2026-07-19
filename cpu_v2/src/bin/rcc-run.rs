//! rcc-run: run an rcc binary image on the simulator and report the halt signal.
//!
//! usage: rcc-run <input.bin> [max_cycles]

use cpu_v2::{decode_binary, simulate_quiet};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(input) = args.next() else {
        eprintln!("usage: rcc-run <input.bin> [max_cycles]");
        return ExitCode::FAILURE;
    };
    let max_cycles: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000_000);

    let bytes = match std::fs::read(&input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("rcc-run: cannot read {input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(instructions) = decode_binary(&bytes) else {
        eprintln!("rcc-run: {input} is not an rcc binary image");
        return ExitCode::FAILURE;
    };

    let (state, signal) = simulate_quiet(&instructions, max_cycles);
    match signal {
        Some(sig) => {
            println!("halt: signal = {sig} (0x{sig:04x}), {} cycles", state.cycles);
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("rcc-run: did not halt within {max_cycles} cycles");
            ExitCode::FAILURE
        }
    }
}

