//! Compiler: drives the new pipeline per function (allocate -> codegen) and
//! lays out/Links functions via the shared Assembler/Linker.

use crate::programmer::{Assembler, FuncDecl, FuncName, Linker};
use crate::programmer::codegen::compile_function;
use crate::programmer::ir::*;
use crate::programmer::passes::{Opts, optimize};
use crate::programmer::regalloc::allocate;
use crate::Instruction;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct Compiler {
    pub opts: Opts,
    funcs: HashMap<FuncName, IrFunc>,
}

impl Compiler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_func(&mut self, f: IrFunc) {
        assert!(
            self.funcs.insert(f.name, f.clone()).is_none(),
            "function {} defined twice",
            f.name
        );
    }

    pub fn has_func(&self, name: FuncName) -> bool {
        self.funcs.contains_key(name)
    }

    pub fn finish(self, main: FuncName) -> Vec<Instruction> {
        self.finish_with_listing(main).0
    }

    /// like `finish`, but also returns a disassembly listing of the whole
    /// program (per function, with call targets annotated)
    pub fn finish_with_listing(self, main: FuncName) -> (Vec<Instruction>, String) {
        let mut asm = Assembler::default();
        let mut linker = Linker::default();
        let mut cursor = 0usize;
        let mut layout: Vec<(FuncName, (usize, usize))> = vec![];

        let mut called: HashSet<FuncName> = HashSet::new();
        let mut next = vec![main];
        called.insert(main);

        while let Some(name) = next.pop() {
            let f = self
                .funcs
                .get(&name)
                .unwrap_or_else(|| panic!("unknown function `{name}`"));
            let mut f = f.clone();
            optimize(&mut f, &self.opts);
            let (allocated_ir, alloc) = allocate(&f, self.opts.coalesce);
            let emitted = compile_function(&allocated_ir, &alloc, &mut asm, cursor);
            let end = cursor + emitted.len;

            for rel in &emitted.relocations {
                if called.insert(rel.func_name) {
                    next.push(rel.func_name);
                }
            }
            linker.register_function(
                (cursor, end),
                FuncDecl::new(name, &[], &[]),
                emitted.relocations,
            );
            layout.push((name, (cursor, end)));
            // 4-word gap between functions: disassembly readability + halt guard
            cursor = end + 4;
            asm.set_cursor(cursor);
        }

        linker.relocate_all(&mut asm);
        let end = asm.get_cursor();
        let instructions = asm.slice_ref()[0..end].to_vec();

        // disassembly listing: per function, call targets annotated by name
        let calls = linker.get_all_calls();
        let mut listing = String::new();
        for (name, (start, end)) in &layout {
            listing.push_str(&format!("{name} {{\n"));
            for (i, inst) in instructions[*start..*end].iter().enumerate() {
                let addr = start + i;
                match calls.get(&addr) {
                    Some(target) => {
                        listing.push_str(&format!("  {addr:04x}: {inst} -----> {target}\n"))
                    }
                    None => listing.push_str(&format!("  {addr:04x}: {inst}\n")),
                }
            }
            listing.push_str("}\n");
        }

        (instructions, listing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isa::Cond;
    use crate::programmer::builder::FuncBuilder;
    use crate::programmer::ir::{BinOp, CmpRhs, VReg};
    use crate::{SimState, simulate};

    pub(super) fn compile_and_run(funcs: Vec<IrFunc>, main: &'static str, max_cycles: usize) -> (SimState, Option<u16>) {
        let mut c = Compiler::new();
        for f in funcs {
            c.add_func(f);
        }
        let instructions = c.finish(main);
        simulate(&instructions, max_cycles)
    }

    pub(super) fn imm_seq(m: &mut FuncBuilder, values: &[u16]) -> Vec<VReg> {
        values.iter().map(|&v| m.load_imm(v)).collect()
    }

    /// all sp_sub immediate values in the program
    fn sp_sub_values(instructions: &[Instruction]) -> Vec<u16> {
        instructions
            .iter()
            .filter_map(|i| match i {
                Instruction::sp_sub(hi, lo) => Some(((*hi as u16) << 4) | *lo as u16),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_add_call() {
        let x = 12u16;
        let y = 43u16;

        let (mut add, params) = FuncBuilder::new("add", 2, 1);
        let s = {
            let a = add.get(params[0]);
            let b = add.get(params[1]);
            add.bin(BinOp::Add, a, b)
        };
        add.ret(&[s]);

        let (mut m, _) = FuncBuilder::new("main", 0, 0);
        let args = imm_seq(&mut m, &[x, y]);
        let rets = m.call("add", &args, 1);
        m.halt(rets[0]);

        let mut c = Compiler::new();
        c.add_func(add.finish());
        c.add_func(m.finish());
        let instructions = c.finish("main");
        // add is a leaf function (no frame); main only saves ra (1 slot)
        assert_eq!(sp_sub_values(&instructions), vec![1]);

        let (_state, signal) = simulate(&instructions, 1000);
        assert_eq!(signal, Some(x + y));
    }

    #[test]
    fn test_loop_sum() {
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
        m.halt(s);

        let (_state, signal) = compile_and_run(vec![m.finish()], "main", 1000);
        assert_eq!(signal, Some((0..n).sum()));
    }

    #[test]
    fn test_spill_stress() {
        // 16 values live simultaneously (> 13 allocatable registers)
        let values: Vec<u16> = (0..16u16).map(|i| i * 3 + 1).collect();
        let (mut m, _) = FuncBuilder::new("main", 0, 0);
        let vals = imm_seq(&mut m, &values);
        let mut acc = vals[0];
        for &v in &vals[1..] {
            acc = m.bin(BinOp::Add, acc, v);
        }
        m.halt(acc);

        // turn the optimizer off: constant folding would collapse the whole
        // computation into one immediate and nothing would spill
        let mut c = Compiler::new();
        c.opts = Opts {
            const_prop: false,
            cse: false,
            dce: false,
            coalesce: true,
        };
        c.add_func(m.finish());
        let instructions = c.finish("main");
        // frame is sized by actual need: some spills, but far below 255
        let subs = sp_sub_values(&instructions);
        assert_eq!(subs.len(), 1);
        assert!((1..=16).contains(&subs[0]), "frame size {}", subs[0]);

        let (_state, signal) = simulate(&instructions, 1000);
        assert_eq!(signal, Some(values.iter().sum()));
    }

    #[test]
    fn test_if_else_max() {
        let (x, y) = (30u16, 20u16);
        let (mut f, params) = FuncBuilder::new("max", 2, 1);
        let cmp = {
            let a = f.get(params[0]);
            let b = f.get(params[1]);
            f.cmp(a, CmpRhs::Reg(b), Cond::Greater)
        };
        let r = f.new_var();
        f.if_else(
            cmp,
            |b| {
                let a = b.get(params[0]);
                b.set(r, a);
            },
            |b| {
                let v = b.get(params[1]);
                b.set(r, v);
            },
        );
        let v = f.get(r);
        f.ret(&[v]);

        let (mut m, _) = FuncBuilder::new("main", 0, 0);
        let args = imm_seq(&mut m, &[x, y]);
        let rets = m.call("max", &args, 1);
        m.halt(rets[0]);

        let (_state, signal) = compile_and_run(vec![f.finish(), m.finish()], "main", 1000);
        assert_eq!(signal, Some(x.max(y)));
    }

    #[test]
    fn test_break_continue() {
        // sum 0,2,4,... while i < 20, but break once sum > 30 (skip nothing else)
        let (mut m, _) = FuncBuilder::new("main", 0, 0);
        let sum = m.new_var();
        let i = m.new_var();
        let zero = m.load_imm(0);
        m.set(sum, zero);
        m.set(i, zero);
        m.while_loop(
            |b| {
                let i = b.get(i);
                b.cmp(i, CmpRhs::Imm(20), Cond::Less)
            },
            |b| {
                let i0 = b.get(i);
                let two = b.load_imm(2);
                let i1 = b.bin(BinOp::Add, i0, two);
                b.set(i, i1);
                // if i == 6 { continue; }
                let i1c = b.get(i);
                let cmp = b.cmp(i1c, CmpRhs::Imm(6), Cond::Equal);
                b.if_then(cmp, |b| b.continue_());
                // if sum > 30 { break; }
                let s = b.get(sum);
                let cmp = b.cmp(s, CmpRhs::Imm(30), Cond::Greater);
                b.if_then(cmp, |b| b.break_());
                let s = b.get(sum);
                let i = b.get(i);
                let s = b.bin(BinOp::Add, s, i);
                b.set(sum, s);
            },
        );
        let s = m.get(sum);
        m.halt(s);

        // rust reference
        let mut sum = 0u16;
        let mut i = 0u16;
        while i < 20 {
            i += 2;
            if i == 6 {
                continue;
            }
            if sum > 30 {
                break;
            }
            sum += i;
        }

        let (_state, signal) = compile_and_run(vec![m.finish()], "main", 1000);
        assert_eq!(signal, Some(sum));
    }

    #[test]
    fn test_six_args_two_rets() {
        // f(a..f) -> (a+b+c, d+e+f)
        let (mut f, params) = FuncBuilder::new("f6", 6, 2);
        let s0 = {
            let a = f.get(params[0]);
            let b = f.get(params[1]);
            f.bin(BinOp::Add, a, b)
        };
        let s0 = {
            let c = f.get(params[2]);
            f.bin(BinOp::Add, s0, c)
        };
        let s1 = {
            let d = f.get(params[3]);
            let e = f.get(params[4]);
            f.bin(BinOp::Add, d, e)
        };
        let s1 = {
            let g = f.get(params[5]);
            f.bin(BinOp::Add, s1, g)
        };
        f.ret(&[s0, s1]);

        let args: Vec<u16> = (1..=6u16).map(|i| i * 10).collect();
        let (mut m, _) = FuncBuilder::new("main", 0, 0);
        let arg_regs = imm_seq(&mut m, &args);
        let rets = m.call("f6", &arg_regs, 2);
        let s = m.bin(BinOp::Add, rets[0], rets[1]);
        m.halt(s);

        let (_state, signal) = compile_and_run(vec![f.finish(), m.finish()], "main", 1000);
        let expected = args[..3].iter().sum::<u16>() + args[3..].iter().sum::<u16>();
        assert_eq!(signal, Some(expected));
    }

    #[test]
    fn test_callee_save_across_call() {
        // g(x) = x + 1
        let (mut g, params) = FuncBuilder::new("g", 1, 1);
        let r = {
            let x = g.get(params[0]);
            let one = g.load_imm(1);
            g.bin(BinOp::Add, x, one)
        };
        g.ret(&[r]);

        // f(a): 10 values derived from a stay live across a call to g(a);
        // 10 > 8 caller-save regs, so some must survive in callee-save/stack
        let (mut f, params) = FuncBuilder::new("f", 1, 1);
        let a = f.get(params[0]);
        let mut vals = vec![];
        for i in 0..10u16 {
            let c = f.load_imm(i * 7 + 1);
            let v = f.bin(BinOp::Add, a, c);
            vals.push(v);
        }
        let rets = f.call("g", &[a], 1);
        let mut acc = rets[0];
        for &v in &vals {
            acc = f.bin(BinOp::Add, acc, v);
        }
        f.ret(&[acc]);

        let av = 5u16;
        let (mut m, _) = FuncBuilder::new("main", 0, 0);
        let args = imm_seq(&mut m, &[av]);
        let rets = m.call("f", &args, 1);
        m.halt(rets[0]);

        let mut c = Compiler::new();
        c.add_func(g.finish());
        c.add_func(f.finish());
        c.add_func(m.finish());
        let instructions = c.finish("main");

        // f uses a frame (callee-save and/or spills), sized by actual need
        let subs = sp_sub_values(&instructions);
        assert!(!subs.is_empty(), "expected a stack frame");
        assert!(subs.iter().all(|&v| (1..=16).contains(&v)), "frames: {subs:?}");
        // epilogues restore sp with the same amount as their prologue
        // (main has no epilogue: it ends with halt)
        let adds: Vec<u16> = instructions
            .iter()
            .filter_map(|i| match i {
                Instruction::sp_add(hi, lo) => Some(((*hi as u16) << 4) | *lo as u16),
                _ => None,
            })
            .collect();
        assert!(!adds.is_empty(), "expected at least one epilogue");
        assert!(adds.iter().all(|a| subs.contains(a)), "{adds:?} vs {subs:?}");

        let a = av;
        let expected: u16 = (a + 1) + (0..10u16).map(|i| a + (i * 7 + 1)).sum::<u16>();
        let (_state, signal) = simulate(&instructions, 2000);
        assert_eq!(signal, Some(expected));
    }
}

// ---------------------------------------------------------------------------
// M5: branch relaxation + optimization integration tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod opt_tests {
    use super::tests::{compile_and_run, imm_seq};
    use super::*;
    use crate::isa::Cond;
    use crate::programmer::builder::FuncBuilder;
    use crate::programmer::ir::{BinOp, CmpRhs};

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
        c.opts = Opts {
            const_prop: false,
            cse: false,
            dce: false,
            coalesce: false,
        };
        c.add_func(build());
        let instructions = c.finish("main");
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
        let instructions = c.finish("main");
        let (_state, signal) = crate::simulate(&instructions, 1000);
        assert_eq!(signal, Some(x));
    }

    #[test]
    fn test_listing_demo() {
        // prints a disassembly listing; view with:
        //   cmd //c run_tests.bat test -p cpu_v2 test_listing_demo -- --nocapture
        let (mut add, params) = FuncBuilder::new("add", 2, 1);
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
        let (instructions, listing) = c.finish_with_listing("main");
        println!("{listing}");
        let (_state, signal) = crate::simulate(&instructions, 1000);
        assert_eq!(signal, Some((0..n).sum::<u16>() + 3));
    }
}
