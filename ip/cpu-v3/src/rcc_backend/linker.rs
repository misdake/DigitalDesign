//! Branch relaxation, image layout, debug finalization and validation.
use super::{
    abi::CPU_V3_REGISTER_CONVENTION,
    lowering::{Line, LoweredFunction},
    options::CompilerOptions,
    program::{resource, BackendError, BackendResourceKind, CpuV3Program},
};
use crate as cpu_v3;
use crate::{TestCondition, Word};
use std::collections::HashMap;

/// One relaxation decision: shrink to the one-word form when the target is
/// reachable with a signed 8-bit offset from the following word.
fn relax_one(from: usize, target: usize, short: &mut bool, changed: &mut bool) {
    let offset = target as i64 - (from + 1) as i64;
    let fits = (-128..=127).contains(&offset);
    if *short != fits {
        *short = fits;
        *changed = true;
    }
}

/// Iterative branch relaxation: any branch, jump, or call whose final signed
/// 8-bit relative offset fits shrinks from the two-word wide form (PFX12 +
/// relative) to one word. Widths only ever shrink, so the layout converges.
pub(super) fn relax_branches(functions: &mut [LoweredFunction], code_base: usize) {
    loop {
        let mut function_addresses = HashMap::new();
        let mut cursor = code_base;
        for function in functions.iter() {
            function_addresses.insert(function.name, cursor);
            cursor += function.size_words();
        }
        let mut changed = false;
        for function in functions.iter_mut() {
            let start = function_addresses[&function.name];
            let mut labels = HashMap::new();
            let mut address = start;
            for line in &function.lines {
                if let Line::Label(label) = line {
                    labels.insert(*label, address);
                } else {
                    address += line.size();
                }
            }
            let mut address = start;
            for line in &mut function.lines {
                match line {
                    Line::Branch { target, short, .. } => {
                        relax_one(address, labels[target], short, &mut changed);
                    }
                    Line::Jump { target, short, .. } => {
                        relax_one(address, labels[target], short, &mut changed);
                    }
                    Line::Call {
                        function: target,
                        short,
                        ..
                    }
                    | Line::TailCall {
                        function: target,
                        short,
                        ..
                    } => {
                        relax_one(address, function_addresses[target], short, &mut changed);
                    }
                    _ => {}
                }
                address += line.size();
            }
        }
        if !changed {
            return;
        }
    }
}

/// A requirement violated by the generated CpuV3 code image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramValidationError {
    /// The word is not a legal instruction: a reserved opcode/function or a
    /// non-canonical unused field.
    InvalidInstruction { address: Word, word: Word },
    /// A `PFX12` word is not immediately followed by a prefix consumer.
    DanglingPrefix { address: Word, next: Option<Word> },
    /// A `PFX12` feeding a major-B relative consumer (function 0..=7) must
    /// carry only the low eight payload bits; `payload12[11:8]` is ignored by
    /// the ISA, and the backend promises to zero it.
    RelativePrefixHighBits { address: Word, payload: Word },
}

impl std::fmt::Display for ProgramValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInstruction { address, word } => {
                write!(f, "illegal instruction {word:#06x} at {address:#06x}")
            }
            Self::DanglingPrefix { address, next } => match next {
                Some(next) => write!(
                    f,
                    "PFX12 at {address:#06x} is not followed by a consumer ({next:#06x})"
                ),
                None => write!(f, "PFX12 at {address:#06x} is the last word"),
            },
            Self::RelativePrefixHighBits { address, payload } => write!(
                f,
                "relative PFX12 at {address:#06x} has non-zero payload12[11:8] ({payload:#05x})"
            ),
        }
    }
}

/// The single final check of the CpuV3 backend's output. It asserts the
/// encoding requirements the frontend, optimizer, and linker are jointly
/// responsible for, after linking and branch relaxation:
///
/// - every word is a legal instruction with canonical unused fields, so no
///   reserved major/function, `JREG`/`JALR` non-canonical nibble, or invalid
///   `MFSR`/`MTSR` selector can reach the image;
/// - every `PFX12` is immediately consumed by the next word;
/// - a relative (`B 0..=7`) prefixed consumer is given `payload12[11:8] == 0`.
///
/// Constant shift amounts, immediate widths, and `PFX12` payload widths are
/// bounded before encoding (`shift_operand` rejects constant shift amounts
/// above 15, and the encoding helpers assert their field widths), so they need
/// no separate image re-check once the words exist.
pub fn validate_program(words: &[Word], code_base: Word) -> Result<(), ProgramValidationError> {
    let address_of = |index: usize| code_base.wrapping_add(index as Word);
    let mut index = 0;
    while index < words.len() {
        let word = words[index];
        if let cpu_v3::Instruction::Prefix { payload } = cpu_v3::decode(word) {
            let Some(&next) = words.get(index + 1) else {
                return Err(ProgramValidationError::DanglingPrefix {
                    address: address_of(index),
                    next: None,
                });
            };
            if !cpu_v3::is_prefix_consumer(next) {
                return Err(ProgramValidationError::DanglingPrefix {
                    address: address_of(index),
                    next: Some(next),
                });
            }
            if next >> 12 == 0xb && (next >> 8) & 0xf <= 7 && payload >> 8 != 0 {
                return Err(ProgramValidationError::RelativePrefixHighBits {
                    address: address_of(index),
                    payload,
                });
            }
            index += 2;
            continue;
        }
        // An FPU v2 instruction is two physical words; a reserved pair is an
        // illegal image and a lone first word is truncated.
        if cpu_v3::FpuOpcode::from_word0(word).is_some() {
            let Some(&next) = words.get(index + 1) else {
                return Err(ProgramValidationError::InvalidInstruction {
                    address: address_of(index),
                    word,
                });
            };
            if matches!(
                cpu_v3::decode_fpu_pair(word, next),
                cpu_v3::Instruction::FpuReserved { .. }
            ) {
                return Err(ProgramValidationError::InvalidInstruction {
                    address: address_of(index),
                    word,
                });
            }
            index += 2;
            continue;
        }
        if matches!(cpu_v3::decode(word), cpu_v3::Instruction::Invalid { .. }) {
            return Err(ProgramValidationError::InvalidInstruction {
                address: address_of(index),
                word,
            });
        }
        index += 1;
    }
    Ok(())
}

pub(super) fn link_segment(
    mut functions: Vec<LoweredFunction>,
    options: &CompilerOptions,
    frontend_debug: rcc::frontend::FrontendDebug,
    external_addresses: &HashMap<rcc::FuncName, usize>,
) -> Result<CpuV3Program, BackendError> {
    let code_base = usize::from(options.code_base);
    relax_branches(&mut functions, code_base);
    let mut function_addresses = HashMap::new();
    let mut cursor = code_base;
    for function in &functions {
        function_addresses.insert(function.name, cursor);
        cursor += function.size_words();
    }
    for (&name, &address) in external_addresses {
        function_addresses.entry(name).or_insert(address);
    }
    if cursor > 1 << 16 {
        return Err(resource(
            BackendResourceKind::CodeWindow,
            format!(
                "CpuV3 program at code_base {:#06x} exceeds the 64K code-segment window",
                options.code_base
            ),
        ));
    }
    if !options.separate_code_data_segments && cursor > usize::from(options.data_base) {
        return Err(resource(
            BackendResourceKind::CodeDataOverlap,
            format!(
                "CpuV3 unified-memory image uses {cursor} code words and crosses data_base {:#06x}",
                options.data_base
            ),
        ));
    }
    let stack_limit = if options.stack_init == 0 {
        1 << 16
    } else {
        usize::from(options.stack_init)
    };
    if !options.separate_code_data_segments && cursor > stack_limit {
        return Err(resource(BackendResourceKind::CodeStackOverlap, format!("CpuV3 unified-memory image uses {cursor} code words and crosses stack top {stack_limit:#07x}")));
    }
    let heap_end = options
        .heap_begin
        .checked_add(options.heap_size)
        .ok_or_else(|| {
            resource(
                BackendResourceKind::HeapRange,
                "CpuV3 heap range wraps the address space".into(),
            )
        })?;
    if usize::from(heap_end) > stack_limit {
        return Err(resource(
            BackendResourceKind::HeapStackOverlap,
            format!(
                "CpuV3 heap {:#06x}..{heap_end:#06x} overlaps the stack top {stack_limit:#07x}",
                options.heap_begin
            ),
        ));
    }
    let static_addresses = functions
        .iter()
        .flat_map(|f| f.static_addresses.iter().copied())
        .collect::<Vec<_>>();
    if !options.separate_code_data_segments {
        if let Some(address) = static_addresses
            .iter()
            .copied()
            .find(|address| usize::from(*address) < cursor)
        {
            return Err(resource(BackendResourceKind::StaticCodeOverlap,format!("CpuV3 unified-memory image uses {cursor} code words but static data starts at {address:#06x}; select a non-overlapping data_base")));
        }
    }
    if let Some(address) = static_addresses
        .iter()
        .copied()
        .find(|address| *address >= options.heap_begin)
    {
        return Err(resource(
            BackendResourceKind::StaticHeapOverlap,
            format!(
                "CpuV3 static data reaches {address:#06x}, overlapping heap_begin {:#06x}",
                options.heap_begin
            ),
        ));
    }

    let mut words = Vec::with_capacity(cursor - code_base);
    let mut listing = String::new();
    let mut debug_lines = Vec::new();
    let mut debug_functions = Vec::new();
    for function in &functions {
        let local_start = words.len();
        let start = code_base + local_start;
        let file = frontend_debug
            .funcs
            .iter()
            .find(|d| d.name == function.name)
            .map(|d| d.file)
            .unwrap_or(0);
        let mut labels = HashMap::new();
        let mut address = start;
        for line in &function.lines {
            if let Line::Label(label) = line {
                assert!(
                    labels.insert(*label, address).is_none(),
                    "duplicate CpuV3 label"
                );
            } else {
                address += line.size();
            }
        }
        listing.push_str(&format!("{} @ {start:#06x}\n", function.name));
        for line in &function.lines {
            if let Line::Label(_) = line {
                continue;
            }
            let emitted = words.len();
            let (count, src_line) = match line {
                Line::Word { word, line } => {
                    words.push(*word);
                    (1, *line)
                }
                Line::Label(_) => continue,
                Line::Branch {
                    condition,
                    target,
                    line,
                    short,
                } => {
                    let span = if *short { 1 } else { 2 };
                    let offset = relative_offset(code_base + words.len() + span, labels[target]);
                    if *short {
                        words.push(cpu_v3::branch(*condition, offset));
                        (1, *line)
                    } else {
                        words.extend(wide_branch(*condition, offset));
                        (2, *line)
                    }
                }
                Line::Jump {
                    target,
                    line,
                    short,
                } => {
                    let span = if *short { 1 } else { 2 };
                    let offset = relative_offset(code_base + words.len() + span, labels[target]);
                    if *short {
                        words.push(cpu_v3::jump_relative(offset));
                        (1, *line)
                    } else {
                        words.extend(wide_jump(offset));
                        (2, *line)
                    }
                }
                Line::Call {
                    function: target,
                    line: source_line,
                    short,
                }
                | Line::TailCall {
                    function: target,
                    line: source_line,
                    short,
                } => {
                    let span = if *short { 1 } else { 2 };
                    let offset =
                        relative_offset(code_base + words.len() + span, function_addresses[target]);
                    let link = matches!(line, Line::Call { .. });
                    if *short {
                        words.push(if link {
                            cpu_v3::jump_and_link_relative(offset)
                        } else {
                            cpu_v3::jump_relative(offset)
                        });
                    } else if link {
                        words.extend(wide_call(offset));
                    } else {
                        words.extend(wide_jump(offset));
                    }
                    (span, *source_line)
                }
                Line::LoadFunctionAddress {
                    function: target,
                    dst,
                    line,
                    short,
                } => {
                    let address = function_addresses[target];
                    if *short {
                        assert!(address < 16, "short function address is out of range");
                        words.push(cpu_v3::immediate_unsigned(
                            cpu_v3::ImmediateOp::LoadUnsigned,
                            *dst,
                            address as u8,
                        ));
                        (1, *line)
                    } else {
                        words.extend(cpu_v3::load_immediate16(*dst, address as u16));
                        (2, *line)
                    }
                }
            };
            let emitted_count = words.len() - emitted;
            debug_assert_eq!(emitted_count, count);
            if let Some(source_line) = src_line {
                for word in emitted..words.len() {
                    debug_lines.push((code_base + word, file, source_line));
                }
            }
        }
        let end = code_base + words.len();
        if function.trailing_guard {
            words.push(cpu_v3::halt());
        }
        debug_functions.push(rcc::DebugFunc {
            name: function.name.to_string(),
            file,
            addr: (start, end),
            frame_size: function.frame_size,
            locals: frontend_debug
                .funcs
                .iter()
                .find(|d| d.name == function.name)
                .map(|d| {
                    d.locals
                        .iter()
                        .map(|v| {
                            let mut v = v.clone();
                            match v.loc {
                                rcc::VarLoc::Frame(slot) => {
                                    v.loc = rcc::VarLoc::Frame(function.callee_saved as u8 + slot);
                                }
                                rcc::VarLoc::ParamIndex(index) => {
                                    v.loc = rcc::VarLoc::Param(
                                        CPU_V3_REGISTER_CONVENTION.argument_registers
                                            [index as usize],
                                    );
                                }
                                _ => {}
                            }
                            v
                        })
                        .collect()
                })
                .unwrap_or_default(),
        });
        // mnemonic listing: wide (prefixed) operations occupy one line
        for line in cpu_v3::disassemble_words(&words[local_start..], start as u16) {
            let span = if line.wide { 2 } else { 1 };
            let raw: Vec<String> = (0..span)
                .map(|i| {
                    format!(
                        "{:04x}",
                        words[usize::from(line.address) - (start - local_start) + i]
                    )
                })
                .collect();
            listing.push_str(&format!(
                "  {:04x}: {:<11} {}\n",
                line.address,
                raw.join(" "),
                line.text
            ));
        }
    }
    validate_program(&words, options.code_base)?;
    debug_lines.sort();
    Ok(CpuV3Program {
        cseg: None,
        code_segments: vec![],
        gpu_debug: frontend_debug.gpu_debug,
        code_base: options.code_base,
        words,
        listing,
        debug: rcc::DebugInfo {
            files: frontend_debug.files,
            function_table: vec![],
            init_sections: vec![],
            functions: debug_functions,
            globals: frontend_debug.globals,
            types: frontend_debug.types,
            consts: frontend_debug.consts,
            lines: debug_lines,
        },
    })
}

fn relative_offset(from: usize, to: usize) -> i16 {
    (to as u16).wrapping_sub(from as u16) as i16
}

fn wide_branch(condition: TestCondition, offset: i16) -> [Word; 2] {
    cpu_v3::prefixed_branch(cpu_v3::branch(condition, 0), offset as u16)
}

fn wide_jump(offset: i16) -> [Word; 2] {
    cpu_v3::prefixed_branch(cpu_v3::jump_relative(0), offset as u16)
}

fn wide_call(offset: i16) -> [Word; 2] {
    cpu_v3::prefixed_branch(cpu_v3::jump_and_link_relative(0), offset as u16)
}
