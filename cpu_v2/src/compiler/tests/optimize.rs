//! optimizer + codegen integration tests: branch relaxation, optimization
//! equivalence, mov coalescing, and the disassembly listing demo.

use super::common::*;

use crate::{BinOp, CmpRhs, Compiler, Cond, FuncBuilder, FunctionTableConfig, IrFunc, Opts};

fn repeated_call_program(call_count: usize) -> Vec<IrFunc> {
    let (mut inc, params) = FuncBuilder::new("inc", 1, 1);
    let value = inc.get(params[0]);
    let one = inc.load_imm(1);
    let value = inc.bin(BinOp::Add, value, one);
    inc.ret(&[value]);

    let (mut main, _) = FuncBuilder::new("main", 0, 0);
    let mut value = main.load_imm(0);
    for _ in 0..call_count {
        value = main.call("inc", &[value], 1)[0];
    }
    main.halt(value);
    vec![inc.finish(), main.finish()]
}

fn compile_function_table_program(
    functions: &[IrFunc],
    config: FunctionTableConfig,
) -> (Vec<crate::Instruction>, String) {
    let mut compiler = Compiler::new();
    compiler.opts.function_table = config;
    for function in functions {
        compiler.add_func(function.clone());
    }
    compiler.finish("main")
}

#[test]
fn test_auto_function_table_reduces_repeated_calls_to_call_abs() {
    let functions = repeated_call_program(4);
    let (without_table, _) =
        compile_function_table_program(&functions, FunctionTableConfig::Disabled);
    let (with_table, listing) =
        compile_function_table_program(&functions, FunctionTableConfig::Auto);

    assert_eq!(
        with_table
            .iter()
            .filter(|instruction| matches!(instruction, crate::Instruction::call_abs(..)))
            .count(),
        4
    );
    assert!(with_table.len() < without_table.len());
    assert!(listing.contains("[00] inc"), "{listing}");
    let (state, signal) = crate::simulate(&with_table, 1_000);
    assert_eq!(signal, Some(4));
    assert_ne!(state.mem[crate::FUNCTION_TABLE_BASE as usize], 0);
}

#[test]
fn test_auto_function_table_keeps_one_off_call_out_of_table() {
    let functions = repeated_call_program(1);
    let (instructions, listing) =
        compile_function_table_program(&functions, FunctionTableConfig::Auto);
    assert!(!instructions
        .iter()
        .any(|instruction| matches!(instruction, crate::Instruction::call_abs(..))));
    assert!(!listing.contains("function table"), "{listing}");
    assert_eq!(crate::simulate(&instructions, 1_000).1, Some(1));
}

#[test]
fn test_all_function_table_forces_single_call_into_table() {
    let functions = repeated_call_program(1);
    let (instructions, _) = compile_function_table_program(&functions, FunctionTableConfig::All);
    assert!(instructions
        .iter()
        .any(|instruction| matches!(instruction, crate::Instruction::call_abs(..))));
    assert_eq!(crate::simulate(&instructions, 1_000).1, Some(1));
}

#[test]
fn test_explicit_function_table_selects_only_named_targets() {
    let mut functions = Vec::new();
    for name in ["inc_a", "inc_b"] {
        let (mut function, params) = FuncBuilder::new(name, 1, 1);
        let value = function.get(params[0]);
        let one = function.load_imm(1);
        let value = function.bin(BinOp::Add, value, one);
        function.ret(&[value]);
        functions.push(function.finish());
    }

    let (mut main, _) = FuncBuilder::new("main", 0, 0);
    let zero = main.load_imm(0);
    let value = main.call("inc_a", &[zero], 1)[0];
    let value = main.call("inc_b", &[value], 1)[0];
    main.halt(value);
    functions.push(main.finish());

    let (instructions, listing) = compile_function_table_program(
        &functions,
        FunctionTableConfig::Functions(vec!["inc_b".to_owned()]),
    );
    assert_eq!(
        instructions
            .iter()
            .filter(|instruction| matches!(instruction, crate::Instruction::call_abs(..)))
            .count(),
        1
    );
    assert!(listing.contains("[00] inc_b"), "{listing}");
    assert!(!listing.contains("] inc_a"), "{listing}");
    assert_eq!(crate::simulate(&instructions, 1_000).1, Some(2));
}

#[test]
fn test_function_table_initializes_entries_across_eight_word_chunks() {
    let mut functions = Vec::new();
    let (mut main, _) = FuncBuilder::new("main", 0, 0);
    let mut value = main.load_imm(0);
    for name in [
        "inc_0", "inc_1", "inc_2", "inc_3", "inc_4", "inc_5", "inc_6", "inc_7", "inc_8",
    ] {
        let (mut function, params) = FuncBuilder::new(name, 1, 1);
        let argument = function.get(params[0]);
        let one = function.load_imm(1);
        let result = function.bin(BinOp::Add, argument, one);
        function.ret(&[result]);
        functions.push(function.finish());
        value = main.call(name, &[value], 1)[0];
    }
    main.halt(value);
    functions.push(main.finish());

    let (instructions, listing) =
        compile_function_table_program(&functions, FunctionTableConfig::All);
    assert_eq!(
        instructions
            .iter()
            .filter(|instruction| matches!(instruction, crate::Instruction::call_abs(..)))
            .count(),
        9
    );
    assert!(listing.contains("[08] inc_8"), "{listing}");
    assert!(listing.contains("global initialization {"), "{listing}");
    assert!(listing.contains("function-table"), "{listing}");
    assert!(listing.contains("mem[r14 + 0x08] = r15"), "{listing}");
    assert!(!instructions
        .iter()
        .any(|instruction| matches!(instruction, crate::Instruction::addi(..))));
    let (state, signal) = crate::simulate(&instructions, 2_000);
    assert_eq!(signal, Some(9));
    assert!(state.mem[crate::FUNCTION_TABLE_BASE as usize..][..9]
        .iter()
        .all(|address| *address != 0));
}

#[test]
fn test_function_table_restores_an_explicit_stack_base_before_main_frame() {
    let functions = repeated_call_program(1);
    let mut compiler = Compiler::new();
    compiler.opts.function_table = FunctionTableConfig::All;
    compiler.opts.stack_init = 0x9000;
    for function in functions {
        compiler.add_func(function);
    }
    let (instructions, listing) = compiler.finish("main");
    let (state, signal) = crate::simulate(&instructions, 1_000);
    assert_eq!(signal, Some(1));
    assert!(state.reg[crate::SP_REG as usize] < 0x9000);
    assert!(state.reg[crate::SP_REG as usize] >= 0x8f00);
    assert!(listing.contains("temporary sp = 0xff00 for function table"), "{listing}");
    assert!(listing.contains("sp = 0x9000"), "{listing}");
}

#[test]
fn test_branch_relaxation() {
    // loop body with ~160 instructions: the back edge needs relaxation
    let (mut m, _) = FuncBuilder::new("main", 0, 0);
    let sum = m.new_var();
    let i = m.new_var();
    let zero = m.load_imm(0);
    m.set(sum, zero);
    m.set(i, zero);
    m.while_loop(
        |b| {
            let i = b.get(i);
            b.cmp(i, CmpRhs::Imm(5), Cond::Less)
        },
        |b| {
            for k in 0..80u16 {
                let s = b.get(sum);
                let c = b.load_imm(k);
                let s = b.bin(BinOp::Add, s, c);
                b.set(sum, s);
            }
            let i0 = b.get(i);
            let one = b.load_imm(1);
            let i1 = b.bin(BinOp::Add, i0, one);
            b.set(i, i1);
        },
    );
    let s = m.get(sum);
    m.halt(s);

    let expected: u16 = (0..5u16).map(|_| (0..80u16).sum::<u16>()).sum();
    let (_state, signal) = compile_and_run(vec![m.finish()], "main", 100000);
    assert_eq!(signal, Some(expected));
}

#[test]
fn test_optimized_vs_unoptimized() {
    // the same program must compute the same result with all passes off
    fn build() -> IrFunc {
        let (mut m, _) = FuncBuilder::new("main", 0, 0);
        let sum = m.new_var();
        let i = m.new_var();
        let zero = m.load_imm(0);
        m.set(sum, zero);
        m.set(i, zero);
        m.while_loop(
            |b| {
                let i = b.get(i);
                b.cmp(i, CmpRhs::Imm(10), Cond::Less)
            },
            |b| {
                let s = b.get(sum);
                let i0 = b.get(i);
                let two = b.load_imm(2);
                let t = b.bin(BinOp::Add, i0, two);
                let s = b.bin(BinOp::Add, s, t);
                b.set(sum, s);
                let one = b.load_imm(1);
                let i1 = b.bin(BinOp::Add, i0, one);
                b.set(i, i1);
            },
        );
        let s = m.get(sum);
        m.halt(s);
        m.finish()
    }

    let (_s1, opt_signal) = compile_and_run(vec![build()], "main", 10000);

    let mut c = Compiler::new();
    c.opts.opt = Opts {
        const_prop: false,
        cse: false,
        dce: false,
        coalesce: false,
    };
    c.add_func(build());
    let (instructions, _) = c.finish("main");
    let (_s2, raw_signal) = crate::simulate(&instructions, 10000);

    let expected: u16 = (0..10u16).map(|i| i + 2).sum();
    assert_eq!(opt_signal, Some(expected));
    assert_eq!(raw_signal, Some(expected));
}

#[test]
fn test_mov_coalescing() {
    // call forwarding should not produce visible mov chains between the
    // arg setup and the call; check via instruction count sanity
    let x = 7u16;
    let build = || {
        let (mut id, params) = FuncBuilder::new("id", 1, 1);
        let a = id.get(params[0]);
        id.ret(&[a]);

        let (mut m, _) = FuncBuilder::new("main", 0, 0);
        let args = imm_seq(&mut m, &[x]);
        let rets = m.call("id", &args, 1);
        m.halt(rets[0]);
        vec![id.finish(), m.finish()]
    };
    let compile = |coalesce| {
        let mut compiler = Compiler::new();
        compiler.opts.opt.coalesce = coalesce;
        compiler.opts.function_table = FunctionTableConfig::Disabled;
        for function in build() {
            compiler.add_func(function);
        }
        compiler.finish("main").0
    };
    let instructions = compile(true);
    let without_coalescing = compile(false);
    let movs = |instructions: &[crate::Instruction]| {
        instructions
            .iter()
            .filter(|instruction| matches!(instruction, crate::Instruction::mov(..)))
            .count()
    };
    assert!(movs(&instructions) < movs(&without_coalescing));
    let (_state, signal) = crate::simulate(&instructions, 1000);
    assert_eq!(signal, Some(x));
}

#[test]
fn test_listing_demo() {
    // prints a disassembly listing; view with:
    //   cmd //c run_tests.bat test -p cpu_v2 --test optimize test_listing_demo -- --nocapture
    let (mut add, params) = FuncBuilder::new("add", 2, 1);
    add.set_names(&["a", "b"], &["r"]);
    let s = {
        let a = add.get(params[0]);
        let b = add.get(params[1]);
        add.bin(BinOp::Add, a, b)
    };
    add.ret(&[s]);

    let n = 10u16;
    let (mut m, _) = FuncBuilder::new("main", 0, 0);
    let sum = m.new_var();
    let i = m.new_var();
    let zero = m.load_imm(0);
    m.set(sum, zero);
    m.set(i, zero);
    m.while_loop(
        |b| {
            let i = b.get(i);
            b.cmp(i, CmpRhs::Imm(n), Cond::Less)
        },
        |b| {
            let s = b.get(sum);
            let i0 = b.get(i);
            let s = b.bin(BinOp::Add, s, i0);
            b.set(sum, s);
            let one = b.load_imm(1);
            let i1 = b.bin(BinOp::Add, i0, one);
            b.set(i, i1);
        },
    );
    let s = m.get(sum);
    let three = m.load_imm(3);
    let rets = m.call("add", &[s, three], 1);
    m.halt(rets[0]);

    let mut c = Compiler::new();
    c.add_func(add.finish());
    c.add_func(m.finish());
    let (instructions, listing) = c.finish("main");
    println!("{listing}");
    let (_state, signal) = crate::simulate(&instructions, 1000);
    assert_eq!(signal, Some((0..n).sum::<u16>() + 3));
}
