//! CpuV3 lowering and symbolic linking with iterative branch relaxation.
//!
//! Control-flow references start in the two-word wide form (PFX12 + relative)
//! and the linker shrinks any branch, jump, or call whose final offset fits
//! the signed 8-bit relative encoding to one word, iterating until the layout
//! is stable.

#[cfg(test)]
mod constant_tests;
#[cfg(test)]
mod enum_method_tests;
mod inline;
mod options;
pub use inline::{
    try_compile_with_inline_report, InlineCompilationReport, InlineFrameCost, InlineOutcome,
    InlineRejection, InlineSourceOrigin,
};
#[cfg(test)]
mod privacy_tests;
#[cfg(test)]
pub(crate) mod resource_tests;
#[cfg(test)]
pub(crate) mod rustc_tests;

pub use options::CompilerOptions;

use rcc::*;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Compile target-independent RCC IR for the CpuV3 ABI and ISA, panicking on a
/// backend error. Compiler front ends should call [`try_compile`] and report
/// the error instead.
pub fn compile(
    program: rcc::frontend::Program,
    options: &CompilerOptions,
    main: FuncName,
) -> CpuV3Program {
    try_compile(program, options, main).unwrap_or_else(|error| panic!("{error}"))
}

/// Fallible form of [`compile`]: returns a [`BackendError`] for a missing
/// function or an invalid code image so the caller can display it.
pub fn try_compile(
    program: rcc::frontend::Program,
    options: &CompilerOptions,
    main: FuncName,
) -> Result<CpuV3Program, BackendError> {
    try_compile_with_inline_report(program, options, main).map(|(program, _)| program)
}

fn compile_ir(
    functions: HashMap<FuncName, IrFunc>,
    options: &CompilerOptions,
    main: FuncName,
    debug: rcc::frontend::FrontendDebug,
) -> Result<CpuV3Program, BackendError> {
    compile_ir_with_allocations(&functions, options, main, debug).map(|(program, _)| program)
}

#[derive(Clone, Debug)]
struct AllocationCost {
    frame: usize,
    gpr_spill: usize,
    fpu_spill: usize,
}
struct InlineSelection<'a> {
    originals: &'a HashMap<FuncName, IrFunc>,
    baseline_costs: &'a HashMap<FuncName, AllocationCost>,
    frames: Vec<InlineFrameCost>,
}
impl InlineSelection<'_> {
    fn retain(&mut self, name: FuncName, candidate: &AllocationCost) -> bool {
        let Some(baseline) = self.baseline_costs.get(name) else {
            return true;
        };
        let frame = InlineFrameCost {
            function: name,
            baseline_frame: baseline.frame,
            candidate_frame: candidate.frame,
            baseline_gpr_spill: baseline.gpr_spill,
            candidate_gpr_spill: candidate.gpr_spill,
            baseline_fpu_spill: baseline.fpu_spill,
            candidate_fpu_spill: candidate.fpu_spill,
        };
        let retain = frame.rejection().is_none();
        self.frames.push(frame);
        retain
    }
}
fn allocate_function(
    mut function: IrFunc,
    options: &CompilerOptions,
    entry: bool,
) -> Result<(IrFunc, Allocation), BackendError> {
    // If-conversion precedes constant-arm hoisting, which can dissolve diamonds.
    if !options.opt.is_disabled() {
        convert_diamonds(&mut function);
    }
    optimize(&mut function, &options.opt);
    let (function, mut allocation) =
        try_allocate_with_convention(&function, options.opt.coalesce, CPU_V3_REGISTER_CONVENTION)
            .map_err(BackendError::Allocation)?;
    if entry {
        allocation.callee_saved.clear();
    }
    Ok((function, allocation))
}
fn allocation_cost(allocation: &Allocation) -> AllocationCost {
    AllocationCost {
        frame: allocation.frame_size(),
        gpr_spill: allocation.spill_slots as usize,
        fpu_spill: allocation.fpu_area_words(),
    }
}
fn compile_ir_with_allocations(
    functions: &HashMap<FuncName, IrFunc>,
    options: &CompilerOptions,
    main: FuncName,
    debug: rcc::frontend::FrontendDebug,
) -> Result<(CpuV3Program, HashMap<FuncName, AllocationCost>), BackendError> {
    compile_ir_selected(functions, options, main, debug, None)
}
fn compile_ir_selected(
    functions: &HashMap<FuncName, IrFunc>,
    options: &CompilerOptions,
    main: FuncName,
    debug: rcc::frontend::FrontendDebug,
    mut selection: Option<&mut InlineSelection<'_>>,
) -> Result<(CpuV3Program, HashMap<FuncName, AllocationCost>), BackendError> {
    let mut pending = BTreeSet::from([main]);
    let mut visited = HashSet::new();
    let mut lowered = Vec::new();
    let mut costs = HashMap::new();
    while let Some(name) = pending.pop_first() {
        if !visited.insert(name) {
            continue;
        }
        let mut source = functions
            .get(name)
            .ok_or_else(|| BackendError::UnknownFunction(name.to_string()))?;
        let (mut function, mut allocation) =
            allocate_function(source.clone(), options, name == main)?;
        if let Some(selector) = selection.as_deref_mut() {
            if !selector.retain(name, &allocation_cost(&allocation)) {
                source = selector
                    .originals
                    .get(name)
                    .ok_or_else(|| BackendError::UnknownFunction(name.to_string()))?;
                (function, allocation) = allocate_function(source.clone(), options, name == main)?;
            }
        }
        // Discover calls only after optimizing/allocating the selected body.
        // Restored callees remain live; constant-disabled dependencies do not.
        for callee in function_callees(&function) {
            if !visited.contains(callee) {
                pending.insert(callee);
            }
        }
        costs.insert(name, allocation_cost(&allocation));
        lowered.push((
            name,
            lower_function(&function, &allocation, name == main, options.stack_init),
        ));
    }
    // Discovery order does not determine final code layout.
    lowered.sort_by_key(|(name, _)| (*name != main, *name));
    Ok((
        link(
            lowered.into_iter().map(|(_, body)| body).collect(),
            options,
            debug,
        )?,
        costs,
    ))
}
fn function_callees(function: &IrFunc) -> impl Iterator<Item = FuncName> + '_ {
    function
        .rpo()
        .into_iter()
        .flat_map(|block| function.blocks[block].insts.iter())
        .filter_map(|instruction| match instruction {
            Instr::Call { func, .. } | Instr::LoadFuncAddr { func, .. } => Some(*func),
            _ => None,
        })
}

fn reachable_functions(
    functions: &HashMap<FuncName, IrFunc>,
    main: FuncName,
) -> Result<HashSet<FuncName>, BackendError> {
    let mut reachable = HashSet::from([main]);
    let mut work = vec![main];
    while let Some(name) = work.pop() {
        let function = functions
            .get(name)
            .ok_or_else(|| BackendError::UnknownFunction(name.to_string()))?;
        for callee in function_callees(function) {
            if !functions.contains_key(callee) {
                return Err(BackendError::UnknownFunction(callee.to_string()));
            }
            if reachable.insert(callee) {
                work.push(callee);
            }
        }
    }
    Ok(reachable)
}

mod abi;
mod cseg;
mod linker;
mod lowering;
mod program;
use abi::CPU_V3_REGISTER_CONVENTION;
use cseg::link;
pub use linker::{validate_program, ProgramValidationError};
use lowering::lower_function;
pub use program::{
    BackendError, BackendResourceError, BackendResourceKind, CpuV3CodeSegment, CpuV3Program,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod cseg_tests;
