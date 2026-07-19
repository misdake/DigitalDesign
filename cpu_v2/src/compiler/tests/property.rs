//! property tests: random small programs are compiled and simulated, then
//! compared against an equivalent Rust reference evaluation.


use crate::{BoolExpr, CmpRhs, Compiler, Cond, FuncBuilder, Opts, VarId, simulate};
use crate::{BinOp, ShiftOp};

/// deterministic xorshift64 PRNG
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
}

const NVARS: usize = 8;

#[derive(Clone, Debug)]
enum Op {
    SetImm { dst: usize, v: u16 },
    Bin { dst: usize, op: BinOp, a: usize, b: usize },
    BinImm { dst: usize, op: BinOp, a: usize, imm: u16 },
    /// if vars[lhs] < imm { body }
    IfLess { lhs: usize, imm: u16, body: Box<Op> },
    /// for i in 0..times { body } (i is a fresh counter)
    Repeat { times: u16, body: Box<Op> },
    /// dst = combine(a, b) via the generated helper function
    CallCombine { dst: usize, a: usize, b: usize },
}

fn gen_op(rng: &mut Rng, depth: usize) -> Op {
    let vars: Vec<usize> = (0..NVARS).collect();
    match rng.below(if depth == 0 { 5 } else { 7 }) {
        0 => Op::SetImm {
            dst: *rng.pick(&vars),
            v: rng.next() as u16,
        },
        1 | 2 => Op::Bin {
            dst: *rng.pick(&vars),
            op: *rng.pick(&[BinOp::Add, BinOp::Sub, BinOp::And, BinOp::Or, BinOp::Xor]),
            a: *rng.pick(&vars),
            b: *rng.pick(&vars),
        },
        3 => Op::BinImm {
            dst: *rng.pick(&vars),
            op: *rng.pick(&[BinOp::Add, BinOp::Sub, BinOp::And, BinOp::Or, BinOp::Xor]),
            a: *rng.pick(&vars),
            imm: (rng.next() as u16) & 0xff,
        },
        4 => Op::CallCombine {
            dst: *rng.pick(&vars),
            a: *rng.pick(&vars),
            b: *rng.pick(&vars),
        },
        5 => Op::IfLess {
            lhs: *rng.pick(&vars),
            imm: (rng.next() as u16) & 0xff,
            body: Box::new(gen_op(rng, depth - 1)),
        },
        _ => Op::Repeat {
            times: (rng.below(5) + 1) as u16,
            body: Box::new(gen_op(rng, depth - 1)),
        },
    }
}

// ----- reference evaluation -----

fn eval_bin(op: BinOp, a: u16, b: u16) -> u16 {
    match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::And => a & b,
        BinOp::Or => a | b,
        BinOp::Xor => a ^ b,
    }
}

fn eval(ops: &[Op], combine: BinOp, vars: &mut [u16; NVARS]) {
    for op in ops {
        eval_op(op, combine, vars);
    }
}
fn eval_op(op: &Op, combine: BinOp, vars: &mut [u16; NVARS]) {
    match op {
        Op::SetImm { dst, v } => vars[*dst] = *v,
        Op::Bin { dst, op, a, b } => vars[*dst] = eval_bin(*op, vars[*a], vars[*b]),
        Op::BinImm { dst, op, a, imm } => vars[*dst] = eval_bin(*op, vars[*a], *imm),
        Op::IfLess { lhs, imm, body } => {
            if vars[*lhs] < *imm {
                eval_op(body, combine, vars);
            }
        }
        Op::Repeat { times, body } => {
            for _ in 0..*times {
                eval_op(body, combine, vars);
            }
        }
        Op::CallCombine { dst, a, b } => vars[*dst] = eval_bin(combine, vars[*a], vars[*b]),
    }
}

// ----- emission -----

fn emit_op(b: &mut FuncBuilder, op: &Op, vars: &[VarId]) {
    match op {
        Op::SetImm { dst, v } => {
            let x = b.load_imm(*v);
            b.set(vars[*dst], x);
        }
        Op::Bin { dst, op, a, b: bb } => {
            let (x, y) = (b.get(vars[*a]), b.get(vars[*bb]));
            let r = b.bin(*op, x, y);
            b.set(vars[*dst], r);
        }
        Op::BinImm { dst, op, a, imm } => {
            let (x, y) = (b.get(vars[*a]), b.load_imm(*imm));
            let r = b.bin(*op, x, y);
            b.set(vars[*dst], r);
        }
        Op::IfLess { lhs, imm, body } => {
            let x = b.get(vars[*lhs]);
            let cmp = b.cmp(x, CmpRhs::Imm(*imm), Cond::Less);
            b.if_bool(BoolExpr::Cmp(cmp), |b| emit_op(b, body, vars));
        }
        Op::Repeat { times, body } => {
            let counter = b.new_var();
            let zero = b.load_imm(0);
            b.set(counter, zero);
            b.while_bool(
                |b| {
                    let c = b.get(counter);
                    BoolExpr::Cmp(b.cmp(c, CmpRhs::Imm(*times), Cond::Less))
                },
                |b| {
                    emit_op(b, body, vars);
                    let c = b.get(counter);
                    let one = b.load_imm(1);
                    let c = b.bin(BinOp::Add, c, one);
                    b.set(counter, c);
                },
            );
        }
        Op::CallCombine { dst, a, b: bb } => {
            let (x, y) = (b.get(vars[*a]), b.get(vars[*bb]));
            let [r] = b.call("combine", &[x, y], 1).try_into().unwrap();
            b.set(vars[*dst], r);
        }
    }
}

fn run_case(seed: u64, n_ops: usize, opts_on: bool) {
    let mut rng = Rng(seed);
    let combine = *rng.pick(&[BinOp::Add, BinOp::Sub, BinOp::And, BinOp::Or, BinOp::Xor]);
    let ops: Vec<Op> = (0..n_ops).map(|_| gen_op(&mut rng, 1)).collect();

    // reference
    let mut ref_vars = [0u16; NVARS];
    for (i, v) in ref_vars.iter_mut().enumerate() {
        *v = i as u16 * 7 + 3;
    }
    eval(&ops, combine, &mut ref_vars);
    let expected = ref_vars[0]
        ^ ref_vars[1].wrapping_shl(1)
        ^ ref_vars[2].wrapping_shr(1)
        ^ ref_vars[3];

    // combine(a, b) helper
    let (mut cf, params) = FuncBuilder::new("combine", 2, 1);
    let (x, y) = (cf.get(params[0]), cf.get(params[1]));
    let r = cf.bin(combine, x, y);
    cf.ret(&[r]);

    // main
    let (mut b, _) = FuncBuilder::new("main", 0, 0);
    let vars: Vec<VarId> = (0..NVARS).map(|_| b.new_var()).collect();
    for (i, &var) in vars.iter().enumerate() {
        let x = b.load_imm(i as u16 * 7 + 3);
        b.set(var, x);
    }
    for op in &ops {
        emit_op(&mut b, op, &vars);
    }
    // signal = v0 ^ (v1 << 1) ^ (v2 >> 1) ^ v3
    let s = {
        let v0 = b.get(vars[0]);
        let v1 = b.get(vars[1]);
        let v1 = b.shift(ShiftOp::Lsl, v1, 1);
        let v2 = b.get(vars[2]);
        let v2 = b.shift(ShiftOp::Lsr, v2, 1);
        let v3 = b.get(vars[3]);
        let s = b.bin(BinOp::Xor, v0, v1);
        let s = b.bin(BinOp::Xor, s, v2);
        b.bin(BinOp::Xor, s, v3)
    };
    b.halt(s);

    let mut c = Compiler::new();
    if !opts_on {
        c.opts = Opts {
            const_prop: false,
            cse: false,
            dce: false,
            coalesce: false,
        };
    }
    c.add_func(cf.finish());
    c.add_func(b.finish());
    let (instructions, _) = c.finish("main");
    let (_state, signal) = simulate(&instructions, 100_000);
    assert_eq!(signal, Some(expected), "seed {seed} ops {ops:?}");
}

#[test]
fn test_random_programs() {
    for seed in 1..40u64 {
        run_case(seed, 20, true);
        run_case(seed, 20, false);
    }
}
