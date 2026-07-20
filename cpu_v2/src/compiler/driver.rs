//! Compiler: drives the new pipeline per function (allocate -> codegen) and
//! lays out/Links functions via the shared Assembler/Linker.

use crate::compiler::codegen::compile_function;
use crate::compiler::debug::{DebugFunc, DebugInfo, VarLoc};
use crate::compiler::ir::*;
use crate::compiler::options::CompilerOptions;
use crate::compiler::passes::optimize;
use crate::compiler::regalloc::allocate;
use crate::compiler::{Assembler, FuncDecl, FuncName, Linker};
use crate::frontend::FrontendDebug;
use crate::Instruction;
use std::collections::{HashMap, HashSet};

fn reachable_functions(funcs: &HashMap<FuncName, IrFunc>, main: FuncName) -> HashSet<FuncName> {
    let mut reachable = HashSet::from([main]);
    let mut work = vec![main];
    while let Some(name) = work.pop() {
        let function = funcs
            .get(name)
            .unwrap_or_else(|| panic!("unknown function `{name}`"));
        for block in &function.blocks {
            for instruction in &block.insts {
                let target = match instruction {
                    Instr::Call { func, .. } | Instr::LoadFuncAddr { func, .. } => Some(*func),
                    _ => None,
                };
                if let Some(target) = target {
                    assert!(funcs.contains_key(target), "unknown function `{target}`");
                    if reachable.insert(target) {
                        work.push(target);
                    }
                }
            }
        }
    }
    reachable
}

fn block_is_in_cycle(function: &IrFunc, start: BlockId) -> bool {
    let mut seen = HashSet::new();
    let mut work = function.successors(start);
    while let Some(block) = work.pop() {
        if block == start {
            return true;
        }
        if seen.insert(block) {
            work.extend(function.successors(block));
        }
    }
    false
}

fn table_init_cost(entries: usize, needs_table_base_load: bool) -> usize {
    if entries == 0 {
        0
    } else {
        // Two address loads + one store_sp per target. Loading sp=0xff00 is
        // free only when it is already the requested entry stack value.
        3 * entries + if needs_table_base_load { 2 } else { 0 }
    }
}

fn select_function_table(
    funcs: &HashMap<FuncName, IrFunc>,
    reachable: &HashSet<FuncName>,
    config: &crate::FunctionTableConfig,
    needs_table_base_load: bool,
) -> HashMap<FuncName, u8> {
    #[derive(Default)]
    struct Stats {
        calls: usize,
        hot: bool,
    }

    let mut stats: HashMap<FuncName, Stats> = HashMap::new();
    for &caller in reachable {
        let function = &funcs[caller];
        for (block_id, block) in function.blocks.iter().enumerate() {
            let in_cycle = block_is_in_cycle(function, block_id);
            for instruction in &block.insts {
                if let Instr::Call { func, .. } = instruction {
                    let entry = stats.entry(*func).or_default();
                    entry.calls += 1;
                    entry.hot |= *func == caller || in_cycle;
                }
            }
        }
    }

    let mut candidates: Vec<(FuncName, usize)> = stats
        .iter()
        .map(|(&name, stat)| {
            (
                name,
                if stat.hot {
                    stat.calls.max(4)
                } else {
                    stat.calls
                },
            )
        })
        .collect();
    candidates.sort_by(|(name_a, weight_a), (name_b, weight_b)| {
        weight_b.cmp(weight_a).then_with(|| name_a.cmp(name_b))
    });

    let selected: Vec<FuncName> = match config {
        crate::FunctionTableConfig::Disabled => vec![],
        crate::FunctionTableConfig::Auto => {
            let mut best_len = 0;
            let mut best_gain = 0isize;
            let mut call_weight = 0usize;
            for (index, (_, weight)) in candidates
                .iter()
                .take(crate::FUNCTION_TABLE_CAPACITY)
                .enumerate()
            {
                call_weight += *weight;
                let entries = index + 1;
                let gain = (2 * call_weight) as isize
                    - table_init_cost(entries, needs_table_base_load) as isize;
                if gain > best_gain {
                    best_gain = gain;
                    best_len = entries;
                }
            }
            candidates
                .into_iter()
                .take(best_len)
                .map(|(name, _)| name)
                .collect()
        }
        crate::FunctionTableConfig::All => {
            assert!(
                candidates.len() <= crate::FUNCTION_TABLE_CAPACITY,
                "{} directly-called functions exceed the {}-entry function table",
                candidates.len(),
                crate::FUNCTION_TABLE_CAPACITY
            );
            candidates.into_iter().map(|(name, _)| name).collect()
        }
        crate::FunctionTableConfig::Functions(names) => {
            assert!(
                names.len() <= crate::FUNCTION_TABLE_CAPACITY,
                "{} configured functions exceed the {}-entry function table",
                names.len(),
                crate::FUNCTION_TABLE_CAPACITY
            );
            let mut selected = vec![];
            for configured in names {
                let Some((&name, _)) = stats.iter().find(|(name, _)| **name == configured) else {
                    panic!("function-table entry `{configured}` is not a directly-called reachable function");
                };
                if !selected.contains(&name) {
                    selected.push(name);
                }
            }
            selected
        }
    };

    selected
        .into_iter()
        .enumerate()
        .map(|(index, name)| (name, index as u8))
        .collect()
}

#[derive(Default)]
pub struct Compiler {
    pub opts: CompilerOptions,
    funcs: HashMap<FuncName, IrFunc>,
    debug: Option<FrontendDebug>,
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

    /// attach frontend debug data (variables/files) to be enriched with
    /// addresses and the line table in `finish_with_debug`
    pub fn set_debug(&mut self, debug: FrontendDebug) {
        self.debug = Some(debug);
    }

    /// compile everything reachable from `main` and return the instruction
    /// image plus a disassembly listing (per function, with source-level
    /// comments: signatures, block roles, call targets)
    pub fn finish(self, main: FuncName) -> (Vec<Instruction>, String) {
        let (instructions, listing, _) = self.finish_impl(main);
        (instructions, listing)
    }

    /// like `finish`, and also produces debugger info when `set_debug` was called
    pub fn finish_with_debug(self, main: FuncName) -> (Vec<Instruction>, String, DebugInfo) {
        let (instructions, listing, debug) = self.finish_impl(main);
        (instructions, listing, debug.unwrap_or_default())
    }

    fn finish_impl(self, main: FuncName) -> (Vec<Instruction>, String, Option<DebugInfo>) {
        let mut asm = Assembler::default();
        let mut linker = Linker::default();
        let mut cursor = 0usize;
        let mut layout: Vec<(FuncName, (usize, usize))> = vec![];
        let mut all_comments: Vec<(usize, String)> = vec![];
        let mut fn_debug: Vec<(FuncName, (usize, usize), usize, usize)> = vec![];
        let mut line_map: Vec<(usize, u32)> = vec![];
        let mut init_sections = vec![];

        let reachable = reachable_functions(&self.funcs, main);
        let function_table = select_function_table(
            &self.funcs,
            &reachable,
            &self.opts.function_table,
            self.opts.stack_init != crate::FUNCTION_TABLE_BASE,
        );
        if !function_table.is_empty() && self.opts.stack_init > crate::FUNCTION_TABLE_BASE {
            panic!(
                "stack_init {:#06x} overlaps function-table memory at {:#06x}",
                self.opts.stack_init,
                crate::FUNCTION_TABLE_BASE
            );
        }

        let mut called: HashSet<FuncName> = HashSet::new();
        let mut next = vec![main];
        called.insert(main);

        while let Some(name) = next.pop() {
            let f = self
                .funcs
                .get(&name)
                .unwrap_or_else(|| panic!("unknown function `{name}`"));
            let mut f = f.clone();
            optimize(&mut f, &self.opts.opt);
            let (allocated_ir, alloc) = allocate(&f, self.opts.opt.coalesce);
            let stack_init = if name == main {
                if !function_table.is_empty() && self.opts.stack_init == 0 {
                    crate::FUNCTION_TABLE_BASE
                } else {
                    self.opts.stack_init
                }
            } else {
                0
            };
            let emitted = compile_function(
                &allocated_ir,
                &alloc,
                &mut asm,
                cursor,
                stack_init,
                &function_table,
                name == main,
            );
            let end = cursor + emitted.len;
            fn_debug.push((
                name,
                (cursor, end),
                alloc.frame_size(),
                alloc.callee_saved.len(),
            ));
            line_map.extend(emitted.line_map);
            init_sections.extend(emitted.init_sections);

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
        if !init_sections.is_empty() {
            listing.push_str("global initialization {\n");
            for section in &init_sections {
                listing.push_str(&format!(
                    "  {:04x}..{:04x} {:18} ; {}\n",
                    section.addr.0, section.addr.1, section.name, section.detail
                ));
            }
            listing.push_str("}\n");
        }
        if !function_table.is_empty() {
            let mut entries: Vec<_> = function_table
                .iter()
                .map(|(&name, &index)| (index, name))
                .collect();
            entries.sort_by_key(|&(index, _)| index);
            listing.push_str("function table {\n");
            for (index, name) in entries {
                let address = layout
                    .iter()
                    .find(|(function, _)| *function == name)
                    .map(|(_, range)| range.0)
                    .unwrap();
                listing.push_str(&format!("  [{index:02x}] {name} @ 0x{address:04x}\n"));
            }
            listing.push_str("}\n");
        }
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

        let debug = self.debug.map(|fd| {
            let mut info = DebugInfo {
                files: fd.files,
                function_table: {
                    let mut entries: Vec<_> = function_table
                        .iter()
                        .map(|(&name, &index)| (index, name.to_string()))
                        .collect();
                    entries.sort_by_key(|(index, _)| *index);
                    entries
                },
                init_sections: init_sections
                    .iter()
                    .map(|section| crate::DebugInitSection {
                        name: section.name.clone(),
                        detail: section.detail.clone(),
                        addr: section.addr,
                    })
                    .collect(),
                globals: fd.globals,
                consts: fd.consts,
                ..DebugInfo::default()
            };
            for (name, range, frame, callee_base) in &fn_debug {
                let fdbg = fd.funcs.iter().find(|d| d.name == *name);
                // frame-local slots follow the callee-save area in the frame
                let locals = fdbg
                    .map(|d| {
                        d.locals
                            .iter()
                            .map(|v| {
                                let mut v = v.clone();
                                if let VarLoc::Frame(slot) = v.loc {
                                    v.loc = VarLoc::Frame(*callee_base as u8 + slot);
                                }
                                v
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                info.functions.push(DebugFunc {
                    name: name.to_string(),
                    file: fdbg.map(|d| d.file).unwrap_or(0),
                    addr: *range,
                    frame_size: *frame,
                    locals,
                });
            }
            for (addr, line) in &line_map {
                // attach each line entry to the function containing the address
                let file = fn_debug
                    .iter()
                    .find(|(_, (s, e), _, _)| (s..e).contains(&addr))
                    .and_then(|(name, _, _, _)| fd.funcs.iter().find(|d| d.name == *name))
                    .map(|d| d.file)
                    .unwrap_or(0);
                info.lines.push((*addr, file, *line));
            }
            info.lines.sort();
            info
        });

        (instructions, listing, debug)
    }
}
