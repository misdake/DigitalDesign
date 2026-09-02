//! Compiles the frozen CPU V3 benchmark suite (rcc source) into hex word
//! images for historical emulator reruns.
//!
//! Old Stage commits predate the current compiler (no `*`, no FPU types), so
//! their harness consumes prebuilt word images instead of source. Each image
//! carries the program's `bench-*` metadata as `#` comments; `bench-max-cycles`
//! is multiplied by `--max-cycles-scale` because the budgets are tuned on the
//! current hardware and older stages are legitimately slower.

use std::path::PathBuf;
use std::process::ExitCode;

fn metadata(source: &str, key: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let line = line.trim().strip_prefix("//")?.trim();
        let (candidate, value) = line.split_once(':')?;
        (candidate.trim() == key).then(|| value.trim().to_string())
    })
}

fn compile(source: &str) -> Vec<u16> {
    let options = cpu_v3::rcc_backend::CompilerOptions::default();
    let program = rcc::frontend::compile_program_named("bench", source, &options, &mut |_| {
        Err("benchmark programs use no modules".to_string())
    })
    .unwrap_or_else(|error| panic!("compile failed: {error}"));
    cpu_v3::rcc_backend::compile(program, &options, "main").words
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut suite = PathBuf::from("systems/cpu-v3-tang-nano-20k/benchmarks/suite");
    let mut out = PathBuf::from("target/bench-history/images");
    let mut scale = 4usize;
    let mut tier: Option<String> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--suite" => {
                suite = PathBuf::from(iter.next().ok_or("--suite needs a value")?);
            }
            "--out" => {
                out = PathBuf::from(iter.next().ok_or("--out needs a value")?);
            }
            "--max-cycles-scale" => {
                scale = iter
                    .next()
                    .ok_or("--max-cycles-scale needs a value")?
                    .parse()
                    .map_err(|_| "invalid --max-cycles-scale".to_string())?;
            }
            "--tier" => {
                tier = Some(iter.next().ok_or("--tier needs a value")?.clone());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }

    let mut paths: Vec<PathBuf> = std::fs::read_dir(&suite)
        .map_err(|e| format!("cannot read {}: {e}", suite.display()))?
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|v| v.to_str()) == Some("rs"))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err(format!("{} contains no .rs programs", suite.display()));
    }

    std::fs::create_dir_all(&out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let mut written = 0usize;
    for path in paths {
        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        let program_tier = metadata(&source, "bench-tier")
            .ok_or_else(|| format!("{} lacks bench-tier", path.display()))?;
        if tier.as_deref().is_some_and(|t| t != program_tier) {
            continue;
        }
        let max_cycles = metadata(&source, "bench-max-cycles")
            .ok_or_else(|| format!("{} lacks bench-max-cycles", path.display()))?
            .parse::<usize>()
            .map_err(|_| format!("{} has invalid bench-max-cycles", path.display()))?;
        let expected_halt =
            metadata(&source, "bench-expected-halt").unwrap_or_else(|| "1".to_string());

        let words = compile(&source);
        let mut image = String::new();
        image.push_str(&format!("# bench-tier: {program_tier}\n"));
        image.push_str(&format!("# bench-max-cycles: {}\n", max_cycles * scale));
        image.push_str(&format!("# bench-max-cycles-scale: {scale}\n"));
        image.push_str(&format!("# bench-expected-halt: {expected_halt}\n"));
        for word in &words {
            image.push_str(&format!("{word:04x}\n"));
        }
        let target = out.join(format!("{name}.hex"));
        std::fs::write(&target, image)
            .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
        written += 1;
        println!("{}: {} words", path.display(), words.len());
    }
    println!("wrote {written} image(s) to {}", out.display());
    Ok(())
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cpu-v3-bench-images: {error}");
            ExitCode::FAILURE
        }
    }
}

