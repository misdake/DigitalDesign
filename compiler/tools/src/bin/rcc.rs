//! rcc: the rcc compiler artifact.
//!
//! compiles an rcc source file (Rust subset, see frontend/spec.md) into a
//! binary image and a disassembly listing.

use cpu_v2::{Compiler, CompilerOptions, FunctionTableConfig, Instruction};
use cpu_v3::rcc_backend::{self as cpu_v3_backend, CompilerOptions as CpuV3Options};
use rcc::frontend::compile_program_named;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "\
rcc: the rcc compiler artifact

usage: rcc <input.rs> [options]
  --target <target>    cpu-v2 (default) or cpu-v3
  -o <file>           binary output (default: <input>.bin)
  --lst <file>        disassembly listing (default: <input>.lst)
  --no-opt            disable all optimization passes
  --function-table <mode>
                      call_abs table: auto (default), none, all, or name,...
  --stack-init <n>    initial stack pointer for main (0 = simulator default)
  --code-base <n>     CpuV3 linked code offset inside its segment
  --data-base <n>     static data section base address
  --heap-begin <n>    heap region start
  --heap-size <n>     heap region size in words
  --vec-cap <n>       Vec initial capacity

numbers are decimal or 0x-prefixed hex.
`mod name;` files are resolved next to the input file (<name>.rs /
<name>.dsl.rs / <name>/mod.rs); the rcc_std library is embedded.";

fn main() -> ExitCode {
    let raw_args = std::env::args().skip(1).collect::<Vec<_>>();
    let target = raw_args
        .windows(2)
        .find_map(|args| (args[0] == "--target").then(|| Target::parse(&args[1])))
        .transpose()
        .unwrap_or_else(|message| die(&message))
        .unwrap_or(Target::CpuV2);
    let mut opts = CompilerOptions::default();
    let mut v3_opts = CpuV3Options::default();
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut lst: Option<PathBuf> = None;

    let mut args = raw_args.into_iter();
    while let Some(a) = args.next() {
        let mut num = |name: &str| -> u16 {
            let v = args
                .next()
                .unwrap_or_else(|| die(&format!("{name} needs a value")));
            parse_u16(&v).unwrap_or_else(|| die(&format!("invalid number for {name}: {v}")))
        };
        match a.as_str() {
            "-o" => {
                out = Some(PathBuf::from(
                    args.next().unwrap_or_else(|| die("-o needs a value")),
                ))
            }
            "--lst" => {
                lst = Some(PathBuf::from(
                    args.next().unwrap_or_else(|| die("--lst needs a value")),
                ))
            }
            "--target" => {
                args.next().unwrap_or_else(|| die("--target needs a value"));
            }
            "--no-opt" => {
                opts.opt = rcc::Opts::disabled();
                v3_opts.opt = rcc::Opts::disabled();
            }
            "--function-table" => {
                if target == Target::CpuV3 {
                    die("--function-table is only supported by cpu-v2");
                }
                let value = args
                    .next()
                    .unwrap_or_else(|| die("--function-table needs a value"));
                opts.function_table = parse_function_table(&value);
            }
            "--stack-init" => {
                let value = num("--stack-init");
                opts.stack_init = value;
                v3_opts.stack_init = value;
            }
            "--code-base" => {
                if target != Target::CpuV3 {
                    die("--code-base requires --target cpu-v3");
                }
                v3_opts.code_base = num("--code-base");
            }
            "--data-base" => {
                let value = num("--data-base");
                opts.data_base = value;
                v3_opts.data_base = value;
            }
            "--heap-begin" => {
                let value = num("--heap-begin");
                opts.heap_begin = value;
                v3_opts.heap_begin = value;
            }
            "--heap-size" => {
                let value = num("--heap-size");
                opts.heap_size = value;
                v3_opts.heap_size = value;
            }
            "--vec-cap" => {
                let value = num("--vec-cap");
                opts.vec_init_cap = value;
                v3_opts.vec_init_cap = value;
            }
            "-h" | "--help" => {
                println!("{HELP}");
                return ExitCode::SUCCESS;
            }
            _ if a.starts_with('-') => die(&format!("unknown option {a}")),
            _ => {
                if input.is_some() {
                    die("multiple input files");
                }
                input = Some(PathBuf::from(a));
            }
        }
    }

    let Some(input) = input else {
        die("no input file (see --help)");
    };
    let out = out.unwrap_or_else(|| {
        input.with_extension(if target == Target::CpuV3 {
            "v3bin"
        } else {
            "bin"
        })
    });
    let lst = lst.unwrap_or_else(|| input.with_extension("lst"));

    let src = std::fs::read_to_string(&input)
        .unwrap_or_else(|e| die(&format!("cannot read {}: {e}", input.display())));
    let dir = input.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut loader = |name: &str| -> Result<String, String> {
        for cand in [
            dir.join(format!("{name}.rs")),
            dir.join(format!("{name}.dsl.rs")),
            dir.join(name).join("mod.rs"),
        ] {
            if let Ok(text) = std::fs::read_to_string(&cand) {
                return Ok(text);
            }
        }
        Err(format!("module file not found next to {}", input.display()))
    };

    if target == Target::CpuV3 {
        let program =
            compile_program_named(&input.display().to_string(), &src, &v3_opts, &mut loader)
                .unwrap_or_else(|error| die(&error.to_string()));
        let n_funcs = program.funcs.len();
        let program = cpu_v3_backend::compile(program, &v3_opts, "main");
        std::fs::write(&out, encode_cpu_v3_words(&program.words))
            .unwrap_or_else(|e| die(&format!("cannot write {}: {e}", out.display())));
        std::fs::write(&lst, &program.listing)
            .unwrap_or_else(|e| die(&format!("cannot write {}: {e}", lst.display())));
        let dbg_path = out.with_extension("dbg");
        std::fs::write(&dbg_path, program.debug.render())
            .unwrap_or_else(|e| die(&format!("cannot write {}: {e}", dbg_path.display())));
        println!(
            "compiled {} functions, {} CpuV3 words -> {} (+ {})",
            n_funcs,
            program.words.len(),
            out.display(),
            lst.display()
        );
    } else {
        let program = compile_program_named(&input.display().to_string(), &src, &opts, &mut loader)
            .unwrap_or_else(|error| die(&error.to_string()));
        let n_funcs = program.funcs.len();
        let mut c = Compiler::new();
        c.opts = opts;
        c.set_debug(program.debug);
        for function in program.funcs {
            c.add_func(function);
        }
        let (instructions, listing, debug) = c.finish_with_debug("main");
        std::fs::write(&out, encode_binary(&instructions))
            .unwrap_or_else(|e| die(&format!("cannot write {}: {e}", out.display())));
        std::fs::write(&lst, &listing)
            .unwrap_or_else(|e| die(&format!("cannot write {}: {e}", lst.display())));
        let dbg_path = out.with_extension("dbg");
        std::fs::write(&dbg_path, debug.render())
            .unwrap_or_else(|e| die(&format!("cannot write {}: {e}", dbg_path.display())));
        println!(
            "compiled {} functions, {} instructions -> {} (+ {})",
            n_funcs,
            instructions.len(),
            out.display(),
            lst.display()
        );
    }
    ExitCode::SUCCESS
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Target {
    CpuV2,
    CpuV3,
}

impl Target {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "cpu-v2" => Ok(Self::CpuV2),
            "cpu-v3" => Ok(Self::CpuV3),
            _ => Err(format!(
                "unknown target `{value}`; expected cpu-v2 or cpu-v3"
            )),
        }
    }
}

fn die(msg: &str) -> ! {
    eprintln!("rcc: error: {msg}");
    std::process::exit(1)
}

fn parse_u16(s: &str) -> Option<u16> {
    if let Some(h) = s.strip_prefix("0x") {
        u16::from_str_radix(h, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn parse_function_table(value: &str) -> FunctionTableConfig {
    match value {
        "auto" => FunctionTableConfig::Auto,
        "none" => FunctionTableConfig::Disabled,
        "all" => FunctionTableConfig::All,
        _ => {
            let names: Vec<String> = value
                .split(',')
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect();
            if names.is_empty() {
                die("--function-table needs auto, none, all, or a comma-separated function list");
            }
            FunctionTableConfig::Functions(names)
        }
    }
}

/// binary image: magic "RCC1", word count (u32 LE), then words (u16 LE)
fn encode_binary(instructions: &[Instruction]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + instructions.len() * 2);
    out.extend_from_slice(b"RCC1");
    out.extend_from_slice(&(instructions.len() as u32).to_le_bytes());
    for w in instructions {
        out.extend_from_slice(&w.encode().to_le_bytes());
    }
    out
}

fn encode_cpu_v3_words(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_v3_output_is_headerless_little_endian_words_for_the_boot_packer() {
        assert_eq!(
            encode_cpu_v3_words(&[0x1234, 0xabcd]),
            [0x34, 0x12, 0xcd, 0xab]
        );
    }
}
