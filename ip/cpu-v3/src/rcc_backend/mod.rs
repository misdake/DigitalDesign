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
use std::collections::{HashMap, HashSet};

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
    compile_ir_with_allocations(functions, options, main, debug).map(|(program, _)| program)
}

#[derive(Clone, Debug)]
struct AllocationCost {
    frame: usize,
    gpr_spill: usize,
    fpu_spill: usize,
}
fn compile_ir_with_allocations(
    functions: HashMap<FuncName, IrFunc>,
    options: &CompilerOptions,
    main: FuncName,
    debug: rcc::frontend::FrontendDebug,
) -> Result<(CpuV3Program, HashMap<FuncName, AllocationCost>), BackendError> {
    let reachable = reachable_functions(&functions, main)?;
    let mut order = vec![main];
    let mut rest = reachable
        .iter()
        .copied()
        .filter(|name| *name != main)
        .collect::<Vec<_>>();
    rest.sort_unstable();
    order.extend(rest);

    let mut lowered = Vec::with_capacity(order.len());
    let mut costs = HashMap::new();
    for name in order {
        let mut function = functions
            .get(name)
            .ok_or_else(|| BackendError::UnknownFunction(name.to_string()))?
            .clone();
        // Safe if-conversion (CpuV3): simple one-instruction diamonds become
        // Boolean comparisons or conditional moves. It changes the CFG and
        // debugger stepping shape, so it only runs with optimizations enabled;
        // it runs before the optimizer's constant-arm hoisting, which would
        // otherwise dissolve the diamond shape.
        if !options.opt.is_disabled() {
            convert_diamonds(&mut function);
        }
        optimize(&mut function, &options.opt);
        let (function, mut allocation) = try_allocate_with_convention(
            &function,
            options.opt.coalesce,
            CPU_V3_REGISTER_CONVENTION,
        )
        .map_err(BackendError::Allocation)?;
        if name == main {
            allocation.callee_saved.clear();
        }
        costs.insert(
            name,
            AllocationCost {
                frame: allocation.frame_size(),
                gpr_spill: allocation.spill_slots as usize,
                fpu_spill: allocation.fpu_area_words(),
            },
        );
        lowered.push(lower_function(
            &function,
            &allocation,
            name == main,
            options.stack_init,
        ));
    }

    Ok((link(lowered, options, debug)?, costs))
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
        for block in function.rpo() {
            for instruction in &function.blocks[block].insts {
                if let Instr::Call { func, .. } | Instr::LoadFuncAddr { func, .. } = instruction {
                    if !functions.contains_key(func) {
                        return Err(BackendError::UnknownFunction(func.to_string()));
                    }
                    if reachable.insert(*func) {
                        work.push(*func);
                    }
                }
            }
        }
    }
    Ok(reachable)
}

mod abi;
mod linker;
mod lowering;
mod program;
use abi::CPU_V3_REGISTER_CONVENTION;
use linker::link;
pub use linker::{validate_program, ProgramValidationError};
use lowering::lower_function;
pub use program::{BackendError, BackendResourceError, BackendResourceKind, CpuV3Program};

#[cfg(test)]
mod tests;
