use super::abi::REG_TMP;
use super::*;
use crate as cpu_v3;
use rcc::frontend::parse_source_with;

/// Every emitted hardware VECTOR instruction obeys the design's section
/// 3.2 partial-overlap rule: a destination range either shares a source
/// base exactly or is disjoint from it, and a `VMULS` scalar is never
/// inside the destination range.
fn assert_no_partial_overlap(program: &CpuV3Program) {
    let mut index = 0;
    while index + 1 < program.words.len() {
        if let cpu_v3::Instruction::FpuVector {
            fa,
            fb,
            fd,
            len,
            subop,
            ..
        } = cpu_v3::decode_fpu_pair(program.words[index], program.words[index + 1])
        {
            let lanes = usize::from(len.lanes());
            let (fa, fb, fd) = (usize::from(fa), usize::from(fb), usize::from(fd));
            let overlap = |a: usize, b: usize, blen: usize| a != b && a < b + blen && b < a + lanes;
            assert!(
                !overlap(fd, fa, lanes),
                "destination f{fd}.. partially overlaps Fa f{fa}.. in:\n{}",
                program.listing
            );
            if subop == cpu_v3::FpuVectorSubop::VMulS {
                assert!(
                    !(fb >= fd && fb < fd + lanes),
                    "VMULS scalar f{fb} is inside destination f{fd}.. in:\n{}",
                    program.listing
                );
            } else if !subop.ignores_second_source() && !subop.uses_stride_mode() {
                assert!(
                    !overlap(fd, fb, lanes),
                    "destination f{fd}.. partially overlaps Fb f{fb}.. in:\n{}",
                    program.listing
                );
            }
        }
        index += 1;
    }
}

fn compile(source: &str, options: CompilerOptions) -> CpuV3Program {
    let program = parse_source_with(source, options.data_base).unwrap();
    super::compile(program, &options, "main")
}

fn assert_source_in_both_modes(source: &str, expected: u16) {
    for opt in [rcc::Opts::default(), rcc::Opts::disabled()] {
        let options = CompilerOptions {
            opt,
            ..CompilerOptions::default()
        };
        let program =
            rcc::frontend::compile_program_named("<regression>", source, &options, &mut |name| {
                Err(format!("unknown module `{name}`"))
            })
            .unwrap();
        let compiled = super::compile(program, &options, "main");
        assert_eq!(execute_capped(compiled, 100_000).0, expected);
    }
}

/// Compile with the rcc standard library appended. `compile` above is
/// parse-only, so operators that lower to a library call (such as `/`) and
/// the library modules themselves need this entry point.
fn compile_with_std(source: &str) -> CpuV3Program {
    let options = CompilerOptions::default();
    compile_with_std_options(source, &options)
}

fn compile_with_std_options(source: &str, options: &CompilerOptions) -> CpuV3Program {
    let program = rcc::frontend::compile_program_named("<test>", source, options, &mut |name| {
        Err(format!("unknown module `{name}`"))
    })
    .unwrap();
    super::compile(program, options, "main")
}

fn execute(program: CpuV3Program) -> (u16, cpu_v3::CpuV3Sim) {
    execute_capped(program, 10_000)
}

fn execute_capped(program: CpuV3Program, max_cycles: usize) -> (u16, cpu_v3::CpuV3Sim) {
    let mut machine = cpu_v3::CpuV3Sim::default();
    machine
        .load_program(program.code_base, &program.words)
        .unwrap();
    if program.code_base != 0 {
        let mut bootstrap = cpu_v3::load_immediate16(REG_TMP, program.code_base).to_vec();
        bootstrap.push(cpu_v3::jump_register(REG_TMP));
        machine.load_program(0, &bootstrap).unwrap();
    }
    let signal = match machine.run(max_cycles).unwrap() {
        cpu_v3::RunOutcome::Halted { signal, .. } => signal,
        outcome => panic!("CpuV3 program did not halt: {outcome:?}"),
    };
    (signal, machine)
}

fn run(source: &str) -> u16 {
    run_with_options(source, CompilerOptions::default()).0
}

/// like `run`, but first asserts the section 3.2 partial-overlap rule over
/// every emitted VECTOR instruction (C2 lowering)
fn run_vector(source: &str) -> u16 {
    let program = compile(source, CompilerOptions::default());
    assert_no_partial_overlap(&program);
    execute(program).0
}

fn run_with_options(source: &str, options: CompilerOptions) -> (u16, cpu_v3::CpuV3Sim) {
    execute(compile(source, options))
}

fn run_with_std(source: &str) -> u16 {
    execute(compile_with_std(source)).0
}

/// like `run_with_std`, for a program that needs more than the default cap
fn run_with_std_capped(source: &str, max_cycles: usize) -> u16 {
    execute_capped(compile_with_std(source), max_cycles).0
}

fn run_with_std_options(
    source: &str,
    options: CompilerOptions,
    max_cycles: usize,
) -> (u16, cpu_v3::CpuV3Sim) {
    execute_capped(compile_with_std_options(source, &options), max_cycles)
}

fn disasm(program: &CpuV3Program) -> Vec<cpu_v3::DisasmLine> {
    cpu_v3::disassemble_words(&program.words, program.code_base)
}

fn mnemonics(program: &CpuV3Program) -> Vec<String> {
    disasm(program).into_iter().map(|line| line.text).collect()
}

fn has_prefix(program: &CpuV3Program, prefix: &str) -> bool {
    mnemonics(program).iter().any(|m| m.starts_with(prefix))
}

mod allocation;

mod control;

mod debug;

mod fpu;

mod integer;

mod machine;

mod validation;

mod values;
