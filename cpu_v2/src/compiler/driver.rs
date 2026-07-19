//! Compiler: drives the new pipeline per function (allocate -> codegen) and
//! lays out/Links functions via the shared Assembler/Linker.

use crate::compiler::{Assembler, FuncDecl, FuncName, Linker};
use crate::compiler::codegen::compile_function;
use crate::compiler::ir::*;
use crate::compiler::passes::{Opts, optimize};
use crate::compiler::regalloc::allocate;
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

    /// compile everything reachable from `main` and return the instruction
    /// image plus a disassembly listing (per function, with source-level
    /// comments: signatures, block roles, call targets)
    pub fn finish(self, main: FuncName) -> (Vec<Instruction>, String) {
        let mut asm = Assembler::default();
        let mut linker = Linker::default();
        let mut cursor = 0usize;
        let mut layout: Vec<(FuncName, (usize, usize))> = vec![];
        let mut all_comments: Vec<(usize, String)> = vec![];

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
            all_comments.extend(emitted.comments);
            // 1 halt word between functions: disassembly boundary + pc runaway guard
            cursor = end + 1;
            asm.set_cursor(cursor);
        }

        linker.relocate_all(&mut asm);
        let end = asm.get_cursor();
        let instructions = asm.slice_ref()[0..end].to_vec();

        // disassembly listing: per function with signature; block roles and
        // call targets annotated as `; comment`
        let calls = linker.get_all_calls();
        let mut comments: HashMap<usize, Vec<&str>> = HashMap::new();
        for (addr, text) in &all_comments {
            comments.entry(*addr).or_default().push(text.as_str());
        }
        let mut listing = String::new();
        for (name, (start, end)) in &layout {
            let f = &self.funcs[name];
            let params = f.param_names.join(", ");
            let rets = f.ret_names.join(", ");
            match (f.param_names.is_empty(), f.ret_names.is_empty()) {
                (true, true) => listing.push_str(&format!("{name} {{\n")),
                (_, true) => listing.push_str(&format!("fn {name}({params}) {{\n")),
                _ => listing.push_str(&format!("fn {name}({params}) -> ({rets}) {{\n")),
            }
            for (i, inst) in instructions[*start..*end].iter().enumerate() {
                let addr = start + i;
                let mut comment = String::new();
                if let Some(cs) = comments.get(&addr) {
                    comment = cs.join("; ");
                }
                if let Some(target) = calls.get(&addr) {
                    let call_text = format!("call {target}");
                    if comment.is_empty() {
                        comment = call_text;
                    } else if !comment.contains(&call_text) {
                        comment = format!("{comment} -----> {target}");
                    }
                }
                if comment.is_empty() {
                    listing.push_str(&format!("  {addr:04x}: {inst}\n"));
                } else {
                    listing.push_str(&format!("  {addr:04x}: {inst:30} ; {comment}\n"));
                }
            }
            listing.push_str("}\n");
        }

        (instructions, listing)
    }
}
