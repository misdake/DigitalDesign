//! Multi-CSEG layout and ordinary-ABI far calls. No data-segment state is changed.
use super::{
    abi::{REG_LINK, REG_SP, REG_TMP},
    linker::{link_segment, place_near_calls, relax_branches, trim_interior_guards},
    lowering::{Line, LoweredFunction},
    BackendError, CompilerOptions, CpuV3CodeSegment, CpuV3Program,
};
use crate::{self as cpu, ImmediateOp, SpecialRegister};
use rcc::{frontend::FrontendDebug, FuncName};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const RETURN_WORDS: usize = 4;
// r15 is caller-saved and is outside the argument and return register ranges.
const REG_FAR_PC: u8 = 15;
fn word(word: u16) -> Line {
    Line::Word { word, line: None }
}
// Synthetic blocks have an unconditional continuation: JSEG, tail transfer,
// or a layout-proven fallthrough after removing an adjacent tail jump.
fn synthetic(name: FuncName, lines: Vec<Line>) -> LoweredFunction {
    LoweredFunction {
        name,
        cseg: None,
        lines,
        static_addresses: vec![],
        frame_size: 0,
        local_base: 0,
        trailing_guard: false,
    }
}
fn header(return_offset: u16) -> LoweredFunction {
    let mut lines = vec![];
    if return_offset != 0 {
        // A far-called default bank must also support cold entry at code_base.
        lines.push(word(cpu::jump_relative(RETURN_WORDS as i16)));
    }
    lines.extend([
        word(cpu::load(REG_TMP, REG_SP, 1)),
        word(cpu::load(REG_FAR_PC, REG_SP, 0)),
        word(cpu::immediate_unsigned(ImmediateOp::Add, REG_SP, 2)),
        word(cpu::jump_segment(REG_TMP, REG_FAR_PC)),
    ]);
    synthetic("<cseg-entry-return>", lines)
}
fn load_constant(lines: &mut Vec<Line>, dst: u8, value: u16) {
    if value < 16 {
        lines.push(word(cpu::immediate_unsigned(
            ImmediateOp::LoadUnsigned,
            dst,
            value as u8,
        )));
    } else {
        lines.extend(cpu::load_immediate16(dst, value).into_iter().map(word));
    }
}
fn save_return(lines: &mut Vec<Line>, return_pc: u16) {
    lines.extend([
        word(cpu::immediate_unsigned(ImmediateOp::Sub, REG_SP, 2)),
        word(cpu::store(REG_LINK, REG_SP, 0)),
        // The original link is safe on the stack; reuse r14 for the caller CSEG.
        // This leaves the destination registers r12/r15 intact for shared entry.
        word(cpu::read_special(REG_LINK, SpecialRegister::CodeSegment)),
        word(cpu::store(REG_LINK, REG_SP, 1)),
    ]);
    load_constant(lines, REG_LINK, return_pc);
}
fn shared_entry(name: FuncName, return_pc: u16) -> LoweredFunction {
    let mut lines = vec![];
    save_return(&mut lines, return_pc);
    lines.push(word(cpu::jump_segment(REG_TMP, REG_FAR_PC)));
    synthetic(name, lines)
}
fn stub(
    name: FuncName,
    target: FuncName,
    cseg: u16,
    return_pc: u16,
    shared: Option<FuncName>,
) -> LoweredFunction {
    let mut lines = vec![];
    if shared.is_none() {
        save_return(&mut lines, return_pc);
    }
    load_constant(&mut lines, REG_TMP, cseg);
    lines.push(Line::LoadFunctionAddress {
        function: target,
        dst: REG_FAR_PC,
        line: None,
        short: false,
    });
    if let Some(function) = shared {
        lines.push(Line::TailCall {
            function,
            line: None,
            short: false,
        });
    } else {
        lines.push(word(cpu::jump_segment(REG_TMP, REG_FAR_PC)));
    }
    synthetic(name, lines)
}
/// Relax absolute offsets together with each bank's local branches. All widths
/// shrink monotonically; a target crossing below 16 can unlock another shrink.
fn relax_banks(
    banks: &mut BTreeMap<u16, Vec<LoweredFunction>>,
    code_base: usize,
) -> HashMap<FuncName, usize> {
    // Function order is now fixed. A terminal tail jump to the very next block
    // has exactly the same control flow as falling through, without an opcode.
    for functions in banks.values_mut() {
        for i in 0..functions.len().saturating_sub(1) {
            let next = functions[i + 1].name;
            let function = &mut functions[i];
            if !function.trailing_guard
                && matches!(function.lines.last(),Some(Line::TailCall{function:target,..}) if *target==next)
            {
                function.lines.pop();
            }
        }
    }
    loop {
        let mut addresses = HashMap::new();
        for functions in banks.values_mut() {
            relax_branches(functions, code_base);
            let mut at = code_base;
            for function in functions {
                addresses.insert(function.name, at);
                at += function.size_words();
            }
        }
        let mut changed = false;
        for functions in banks.values_mut() {
            for function in functions {
                for line in &mut function.lines {
                    if let Line::LoadFunctionAddress {
                        function: target,
                        short,
                        ..
                    } = line
                    {
                        if !*short && addresses[target] < 16 {
                            *short = true;
                            changed = true;
                        }
                    }
                }
            }
        }
        if !changed {
            return addresses;
        }
    }
}
pub(super) fn link(
    mut functions: Vec<LoweredFunction>,
    options: &CompilerOptions,
    debug: FrontendDebug,
) -> Result<CpuV3Program, BackendError> {
    let Some(default) = debug.default_cseg else {
        if !options.opt.is_disabled() {
            trim_interior_guards(&mut functions);
            let bodies = 0..functions.len();
            place_near_calls(&mut functions, bodies, usize::from(options.code_base));
        }
        return link_segment(functions, options, debug, &HashMap::new());
    };
    if !options.separate_code_data_segments {
        return Err(BackendError::Cseg(
            "link_csegs requires separate code/data segments".into(),
        ));
    }
    if functions.first().and_then(|f| f.cseg) != Some(default) {
        return Err(BackendError::Cseg(
            "the entry function must use the default CSEG".into(),
        ));
    }
    let placement: HashMap<_, _> = functions
        .iter()
        .map(|f| (f.name, f.cseg.unwrap_or(default)))
        .collect();
    let mut incoming = BTreeSet::new();
    for function in &functions {
        for line in &function.lines {
            if let Line::Call {
                function: target, ..
            } = line
            {
                if placement[function.name] != placement[target] {
                    incoming.insert(placement[target]);
                }
            }
        }
    }
    // Only far-called banks need a return island. If default is never far-called,
    // it keeps its normal entry at code_base and other islands start at offset0.
    let return_offset = u16::from(incoming.contains(&default));
    if !incoming.is_empty()
        && usize::from(options.code_base) + RETURN_WORDS + usize::from(return_offset) > 65536
    {
        return Err(BackendError::Cseg(
            "code_base leaves no room for the CSEG entry/return header".into(),
        ));
    }
    let mut banks: BTreeMap<u16, Vec<LoweredFunction>> = BTreeMap::new();
    for function in functions {
        banks
            .entry(placement[function.name])
            .or_default()
            .push(function);
    }
    let multiple = banks.len() > 1;
    let mut body_ranges = Vec::new();
    for (&bank, functions) in &mut banks {
        if !options.opt.is_disabled() {
            trim_interior_guards(functions);
        }
        let mut targets = BTreeSet::new();
        for function in functions.iter() {
            for line in &function.lines {
                match line {
                    Line::Call {
                        function: target, ..
                    } if placement[target] != bank => {
                        targets.insert(*target);
                    }
                    Line::LoadFunctionAddress {
                        function: target, ..
                    } if multiple => {
                        return Err(BackendError::Cseg(format!(
                            "near function pointer `{target}` is unsupported in a multi-CSEG program; use direct calls"
                        )));
                    }
                    _ => {}
                }
            }
        }
        let stubs: HashMap<_, _> = targets
            .iter()
            .map(|target| {
                (
                    *target,
                    Box::leak(format!("<cseg-{bank}-call-{target}>").into_boxed_str()) as FuncName,
                )
            })
            .collect();
        for function in functions.iter_mut() {
            for line in &mut function.lines {
                if let Line::Call {
                    function: target, ..
                } = line
                {
                    if let Some(stub) = stubs.get(target) {
                        *target = *stub;
                    }
                }
            }
        }
        let ordinary_bodies = functions.len();
        let start = usize::from(incoming.contains(&bank));
        if start != 0 {
            functions.insert(0, header(return_offset));
        }
        let shared = (targets.len() > 1)
            .then(|| Box::leak(format!("<cseg-{bank}-enter>").into_boxed_str()) as FuncName);
        for target in targets {
            functions.push(stub(
                stubs[target],
                target,
                placement[target],
                options.code_base + return_offset,
                shared,
            ));
        }
        if let Some(name) = shared {
            functions.push(shared_entry(name, options.code_base + return_offset));
        }
        body_ranges.push((bank, start..start + ordinary_bodies));
    }
    let code_base = usize::from(options.code_base);
    // A smaller relative layout can still widen another bank's absolute stub
    // loads. Compare the fully relaxed banks before accepting the new order.
    let baseline = (multiple && !options.opt.is_disabled()).then(|| {
        let mut original = banks.clone();
        let addresses = relax_banks(&mut original, code_base);
        (original, addresses)
    });
    if !options.opt.is_disabled() {
        for (bank, range) in body_ranges {
            place_near_calls(banks.get_mut(&bank).unwrap(), range, code_base);
        }
    }
    let mut addresses = relax_banks(&mut banks, code_base);
    if let Some((original, original_addresses)) = baseline {
        let words = |functions: &[LoweredFunction]| {
            functions
                .iter()
                .map(LoweredFunction::size_words)
                .sum::<usize>()
        };
        if banks
            .iter()
            .any(|(bank, functions)| words(functions) > words(&original[bank]))
        {
            banks = original;
            addresses = original_addresses;
        }
    }
    let mut root = None;
    let mut segments = Vec::new();
    for (bank, functions) in banks {
        let mut linked = link_segment(functions, options, debug.clone(), &addresses)?;
        if bank == default {
            linked.cseg = Some(default);
            root = Some(linked);
        } else {
            segments.push(CpuV3CodeSegment {
                cseg: bank,
                code_base: linked.code_base,
                words: linked.words,
                listing: linked.listing,
                debug: linked.debug,
            });
        }
    }
    let mut root = root.expect("default CSEG contains entry");
    root.code_segments = segments;
    Ok(root)
}
