//! Compiler: drives the pipeline per function, then hands symbolic machine
//! objects to the whole-program linker.

use crate::frontend::FrontendDebug;
use crate::rcc_backend::codegen::compile_function;
use crate::rcc_backend::linker::link;
use crate::rcc_backend::options::CompilerOptions;
use crate::Instruction;
use rcc::*;
use rcc::{allocate_with_convention, optimize, Allocation, FuncName};
use rcc::{DebugFunc, DebugInfo, VarLoc};
use std::collections::{HashMap, HashSet};

struct PreparedFunction {
    name: FuncName,
    ir: IrFunc,
    alloc: Allocation,
}

/// CpuV2 has no hardware multiply: rewrite integer `BinOp::Mul` into a call to
/// the rcc_std `mul_16x16` library function (always linked from rcc_std).
fn rewrite_mul_as_library_calls(funcs: &mut HashMap<FuncName, IrFunc>) {
    let mut uses_mul = false;
    for f in funcs.values() {
        for block in &f.blocks {
            for inst in &block.insts {
                if matches!(inst, Instr::Bin { op: BinOp::Mul, .. }) {
                    uses_mul = true;
                }
            }
        }
    }
    if uses_mul {
        assert!(
            funcs.contains_key("mul_16x16"),
            "integer multiply on CpuV2 requires the rcc_std library (compile through the frontend)"
        );
    }
    for f in funcs.values_mut() {
        for block in &mut f.blocks {
            for inst in &mut block.insts {
                if let Instr::Bin { dst, op: BinOp::Mul, lhs, rhs } = inst {
                    *inst = Instr::Call {
                        func: "mul_16x16",
                        args: vec![*lhs, *rhs],
                        rets: vec![*dst],
                    };
                }
            }
        }
    }
}

fn reachable_functions(funcs: &HashMap<FuncName, IrFunc>, main: FuncName) -> HashSet<FuncName> {
    let mut reachable = HashSet::from([main]);
    let mut work = vec![main];
    while let Some(name) = work.pop() {
        let function = funcs
            .get(name)
            .unwrap_or_else(|| panic!("unknown function `{name}`"));
        for block in function.rpo() {
            for instruction in &function.blocks[block].insts {
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
) -> HashMap<FuncName, u8> {
    #[derive(Default)]
    struct Stats {
        calls: usize,
        hot: bool,
    }

    let mut stats: HashMap<FuncName, Stats> = HashMap::new();
    for &caller in reachable {
        let function = &funcs[caller];
        for block_id in function.rpo() {
            let block = &function.blocks[block_id];
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
        // Auto is chosen after a baseline whole-program layout, where the
        // linker knows which individual call sites really need three words.
        crate::FunctionTableConfig::Auto => vec![],
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

fn select_auto_function_table(
    far_calls: &[crate::rcc_backend::linker::FarCall],
    needs_table_base_load: bool,
) -> HashMap<FuncName, u8> {
    let mut weights: HashMap<FuncName, usize> = HashMap::new();
    for call in far_calls {
        *weights.entry(call.target).or_default() += call.weight;
    }
    let mut candidates: Vec<_> = weights.into_iter().collect();
    candidates.sort_by(|(name_a, weight_a), (name_b, weight_b)| {
        weight_b.cmp(weight_a).then_with(|| name_a.cmp(name_b))
    });

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
        let gain =
            (2 * call_weight) as isize - table_init_cost(entries, needs_table_base_load) as isize;
        if gain > best_gain {
            best_gain = gain;
            best_len = entries;
        }
    }

    candidates
        .into_iter()
        .take(best_len)
        .enumerate()
        .map(|(index, (name, _))| (name, index as u8))
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
        let mut funcs = self.funcs.clone();
        rewrite_mul_as_library_calls(&mut funcs);
        let reachable = reachable_functions(&funcs, main);
        let mut prepared = vec![];
        let mut called: HashSet<FuncName> = HashSet::new();
        let mut next = vec![main];
        called.insert(main);

        while let Some(name) = next.pop() {
            let f = funcs
                .get(&name)
                .unwrap_or_else(|| panic!("unknown function `{name}`"));
            let mut f = f.clone();
            optimize(&mut f, &self.opts.opt);
            let (allocated_ir, mut alloc) =
                allocate_with_convention(&f, self.opts.opt.coalesce, super::V2_REGISTER_CONVENTION);
            if name == main {
                // The entry function never returns to a caller, so its
                // incoming callee-saved registers and return address do not
                // need to be preserved. Locals and spills still get a frame.
                alloc.callee_saved.clear();
            }
            for block in allocated_ir.rpo() {
                for instruction in &allocated_ir.blocks[block].insts {
                    let target = match instruction {
                        Instr::Call { func, .. } | Instr::LoadFuncAddr { func, .. } => Some(*func),
                        _ => None,
                    };
                    if let Some(target) = target {
                        if called.insert(target) {
                            next.push(target);
                        }
                    }
                }
            }
            prepared.push(PreparedFunction {
                name,
                ir: allocated_ir,
                alloc,
            });
        }

        let lower = |function_table: &HashMap<FuncName, u8>| {
            prepared
                .iter()
                .map(|function| {
                    let stack_init = if function.name == main {
                        if !function_table.is_empty() && self.opts.stack_init == 0 {
                            crate::FUNCTION_TABLE_BASE
                        } else {
                            self.opts.stack_init
                        }
                    } else {
                        0
                    };
                    compile_function(
                        &function.ir,
                        &function.alloc,
                        stack_init,
                        function_table,
                        function.name == main,
                    )
                })
                .collect::<Vec<_>>()
        };

        let mut function_table =
            select_function_table(&funcs, &reachable, &self.opts.function_table);
        let validate_function_table = |table: &HashMap<FuncName, u8>| {
            if !table.is_empty() && self.opts.stack_init > crate::FUNCTION_TABLE_BASE {
                panic!(
                    "stack_init {:#06x} overlaps function-table memory at {:#06x}",
                    self.opts.stack_init,
                    crate::FUNCTION_TABLE_BASE
                );
            }
        };
        let linked = if matches!(self.opts.function_table, crate::FunctionTableConfig::Auto) {
            let baseline = link(&lower(&function_table));
            function_table = select_auto_function_table(
                &baseline.far_calls,
                self.opts.stack_init != crate::FUNCTION_TABLE_BASE,
            );
            validate_function_table(&function_table);
            if function_table.is_empty() {
                baseline
            } else {
                link(&lower(&function_table))
            }
        } else {
            validate_function_table(&function_table);
            link(&lower(&function_table))
        };

        let instructions = linked.instructions;
        let layout = linked.layout;
        let all_comments = linked.comments;
        let line_map = linked.line_map;
        let init_sections = linked.init_sections;
        let calls = linked.calls;
        let fn_debug: Vec<_> = prepared
            .iter()
            .map(|function| {
                let name = function.name;
                let range = layout
                    .iter()
                    .find(|(function, _)| *function == name)
                    .map(|(_, range)| *range)
                    .expect("linked function missing from layout");
                (
                    name,
                    range,
                    function.alloc.frame_size(),
                    function.alloc.callee_saved.len(),
                )
            })
            .collect();

        // disassembly listing: per function with signature; block roles and
        // call targets annotated as `; comment`
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
                                match v.loc {
                                    VarLoc::Frame(slot) => {
                                        v.loc = VarLoc::Frame(*callee_base as u8 + slot);
                                    }
                                    VarLoc::ParamIndex(index) => {
                                        v.loc = VarLoc::Param(
                                            super::V2_REGISTER_CONVENTION.argument_registers
                                                [index as usize],
                                        );
                                    }
                                    _ => {}
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
