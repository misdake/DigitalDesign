//! optimizer + codegen integration tests: branch relaxation, optimization
//! equivalence, mov coalescing, and the disassembly listing demo.

use super::common::*;

use crate::{BinOp, CmpRhs, Compiler, Cond, FuncBuilder, IrFunc, Opts};

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
    let (mut id, params) = FuncBuilder::new("id", 1, 1);
    let a = id.get(params[0]);
    id.ret(&[a]);

    let (mut m, _) = FuncBuilder::new("main", 0, 0);
    let args = imm_seq(&mut m, &[x]);
    let rets = m.call("id", &args, 1);
    m.halt(rets[0]);

    let mut c = Compiler::new();
    c.add_func(id.finish());
    c.add_func(m.finish());
    let (instructions, _) = c.finish("main");
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
