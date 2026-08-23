//! Whole-program layout and final machine-code encoding.

use crate::isa::*;
use crate::rcc_backend::codegen::{
    line_size, relax, CallSite, EmittedFunc, InitSection, MachineLine,
};
use crate::{FuncName, Instruction, TMP_REG};
use std::collections::{HashMap, HashSet};

pub(crate) struct LinkedProgram {
    pub instructions: Vec<Instruction>,
    pub layout: Vec<(FuncName, (usize, usize))>,
    pub comments: Vec<(usize, String)>,
    pub line_map: Vec<(usize, u32)>,
    pub init_sections: Vec<InitSection>,
    /// Address of the instruction that actually performs each call.
    pub calls: HashMap<usize, FuncName>,
    /// Non-table direct calls that use their three-word far encoding.
    pub far_calls: Vec<FarCall>,
}

#[derive(Clone, Copy)]
pub(crate) struct FarCall {
    pub target: FuncName,
    pub weight: usize,
}

struct LayoutPass {
    functions: Vec<(FuncName, Vec<MachineLine>)>,
    ranges: HashMap<FuncName, (usize, usize)>,
    end: usize,
}

pub(crate) fn link(functions: &[EmittedFunc]) -> LinkedProgram {
    let mut near_calls = HashSet::new();
    let mut pass;

    loop {
        pass = layout(functions, &near_calls);
        let mut discovered = vec![];
        for (caller, lines) in &pass.functions {
            let mut addr = pass.ranges[caller].0;
            for line in lines {
                if let MachineLine::DirectCall { func, id, .. } = line {
                    let site = CallSite { caller, id: *id };
                    if !near_calls.contains(&site)
                        && call_rel_offset(addr, pass.ranges[func].0).is_some()
                    {
                        discovered.push(site);
                    }
                }
                addr += line_size(line, caller, &near_calls);
            }
        }
        if discovered.is_empty() {
            break;
        }
        near_calls.extend(discovered);
    }

    // The last discovery pass may have changed line sizes. Recompute branches
    // and addresses once with the stable near-call set.
    pass = layout(functions, &near_calls);
    encode(pass, near_calls)
}

fn layout(functions: &[EmittedFunc], near_calls: &HashSet<CallSite>) -> LayoutPass {
    let mut laid_out = Vec::with_capacity(functions.len());
    let mut ranges = HashMap::new();
    let mut cursor = 0usize;

    for function in functions {
        let mut lines = function.lines.clone();
        relax(&mut lines, function.name, near_calls);
        let len = lines
            .iter()
            .map(|line| line_size(line, function.name, near_calls))
            .sum::<usize>();
        let end = cursor
            .checked_add(len)
            .unwrap_or_else(|| panic!("instruction layout overflow in `{}`", function.name));
        assert!(end < 65536, "program exceeds 64K instruction memory");
        ranges.insert(function.name, (cursor, end));
        laid_out.push((function.name, lines));
        // One halt word between functions and after the final function.
        cursor = end + 1;
    }

    assert!(cursor <= 65536, "program exceeds 64K instruction memory");
    LayoutPass {
        functions: laid_out,
        ranges,
        end: cursor,
    }
}

fn encode(pass: LayoutPass, near_calls: HashSet<CallSite>) -> LinkedProgram {
    let mut instructions = vec![halt(0); pass.end];
    let mut layout = Vec::with_capacity(pass.functions.len());
    let mut comments = vec![];
    let mut line_map = vec![];
    let mut init_sections = vec![];
    let mut calls = HashMap::new();
    let mut far_calls = vec![];

    for (name, lines) in &pass.functions {
        let range = pass.ranges[name];
        layout.push((*name, range));
        let labels = label_addresses(lines, name, range.0, &near_calls);
        let mut addr = range.0;
        let mut open_section: Option<(String, String, usize)> = None;

        for line in lines {
            match line {
                MachineLine::Label(_) => {}
                MachineLine::Comment(text) => comments.push((addr, text.clone())),
                MachineLine::SectionStart { name, detail } => {
                    assert!(open_section.is_none(), "nested initialization sections");
                    comments.push((addr, format!("global init: {detail}")));
                    open_section = Some((name.clone(), detail.clone(), addr));
                }
                MachineLine::SectionEnd => {
                    let (name, detail, start) = open_section
                        .take()
                        .expect("initialization section end without start");
                    init_sections.push(InitSection {
                        name,
                        detail,
                        addr: (start, addr),
                    });
                }
                MachineLine::Inst(inst, line) => {
                    map_line(&mut line_map, addr, 1, *line);
                    instructions[addr] = *inst;
                    addr += 1;
                }
                MachineLine::Branch { cond, target, line } => {
                    map_line(&mut line_map, addr, 1, *line);
                    let (hi, lo) = relative_offset(addr, labels[target], name);
                    instructions[addr] = match cond {
                        Cond::Never => panic!("branch with Cond::Never"),
                        Cond::Greater => jg(hi, lo),
                        Cond::Equal => je(hi, lo),
                        Cond::Less => jl(hi, lo),
                        Cond::GreaterEqual => jge(hi, lo),
                        Cond::LessEqual => jle(hi, lo),
                        Cond::NotEqual => jne(hi, lo),
                        Cond::Always => jmp(hi, lo),
                    };
                    addr += 1;
                }
                MachineLine::Jump { target, line } => {
                    map_line(&mut line_map, addr, 1, *line);
                    let (hi, lo) = relative_offset(addr, labels[target], name);
                    instructions[addr] = jmp(hi, lo);
                    addr += 1;
                }
                MachineLine::AbsJump { target, line } => {
                    map_line(&mut line_map, addr, 3, *line);
                    encode_absolute_target(&mut instructions, addr, labels[target], false);
                    addr += 3;
                }
                MachineLine::DirectCall {
                    func,
                    id,
                    weight,
                    line,
                } => {
                    let site = CallSite {
                        caller: name,
                        id: *id,
                    };
                    let target = pass.ranges[func].0;
                    if near_calls.contains(&site) {
                        map_line(&mut line_map, addr, 1, *line);
                        let offset = call_rel_offset(addr, target)
                            .expect("stable near call moved out of range")
                            as u8;
                        instructions[addr] = call_rel(offset >> 4, offset & 0xf);
                        calls.insert(addr, *func);
                        addr += 1;
                    } else {
                        map_line(&mut line_map, addr, 3, *line);
                        encode_absolute_target(&mut instructions, addr, target, true);
                        calls.insert(addr + 2, *func);
                        far_calls.push(FarCall {
                            target: func,
                            weight: *weight,
                        });
                        addr += 3;
                    }
                }
                MachineLine::CallAbs1 { func, index, line } => {
                    map_line(&mut line_map, addr, 1, *line);
                    let (hi, lo) = hi_lo(*index);
                    instructions[addr] = call_abs(hi, lo);
                    calls.insert(addr, *func);
                    addr += 1;
                }
                MachineLine::LoadAddr2 { func, reg, line } => {
                    map_line(&mut line_map, addr, 2, *line);
                    let target = pass.ranges[func].0;
                    let (hi, lo) = hi_lo(target as u8);
                    instructions[addr] = load_lo(hi, lo, *reg);
                    let (hi, lo) = hi_lo((target >> 8) as u8);
                    instructions[addr + 1] = load_hi(hi, lo, *reg);
                    addr += 2;
                }
            }
        }
        assert!(
            open_section.is_none(),
            "unterminated initialization section"
        );
        assert_eq!(addr, range.1, "layout changed while encoding `{name}`");
    }

    LinkedProgram {
        instructions,
        layout,
        comments,
        line_map,
        init_sections,
        calls,
        far_calls,
    }
}

fn label_addresses(
    lines: &[MachineLine],
    function: FuncName,
    start: usize,
    near_calls: &HashSet<CallSite>,
) -> HashMap<usize, usize> {
    let mut labels = HashMap::new();
    let mut addr = start;
    for line in lines {
        if let MachineLine::Label(label) = line {
            labels.insert(*label, addr);
        } else {
            addr += line_size(line, function, near_calls);
        }
    }
    labels
}

fn map_line(map: &mut Vec<(usize, u32)>, addr: usize, size: usize, line: Option<u32>) {
    if let Some(line) = line {
        for slot in 0..size {
            map.push((addr + slot, line));
        }
    }
}

fn encode_absolute_target(
    instructions: &mut [Instruction],
    addr: usize,
    target: usize,
    call: bool,
) {
    let (hi, lo) = hi_lo(target as u8);
    instructions[addr] = load_lo(hi, lo, TMP_REG);
    let (hi, lo) = hi_lo((target >> 8) as u8);
    instructions[addr + 1] = load_hi(hi, lo, TMP_REG);
    instructions[addr + 2] = if call {
        call_reg(TMP_REG)
    } else {
        jmp_reg(TMP_REG)
    };
}

fn hi_lo(value: u8) -> (u8, u8) {
    (value >> 4, value & 0xf)
}

fn relative_offset(from: usize, to: usize, function: &str) -> (u8, u8) {
    let offset = to as i64 - from as i64;
    assert!(
        (-128..=127).contains(&offset) && offset != 0,
        "branch still out of range after relaxation in function {function}: \
         from 0x{from:04x} to 0x{to:04x} (offset {offset})"
    );
    let value = offset as i8 as u8;
    (value >> 4, value & 0xf)
}

fn call_rel_offset(from: usize, to: usize) -> Option<i8> {
    let offset = to as i64 - from as i64;
    if (-128..=127).contains(&offset) && offset != 0 {
        Some(offset as i8)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct_call(target: FuncName, id: usize) -> MachineLine {
        MachineLine::DirectCall {
            func: target,
            id,
            weight: 1,
            line: Some(1),
        }
    }

    fn padding(count: usize) -> Vec<MachineLine> {
        (0..count)
            .map(|_| MachineLine::Inst(mov(0, 0), Some(2)))
            .collect()
    }

    fn target_function() -> EmittedFunc {
        EmittedFunc {
            name: "target",
            lines: vec![MachineLine::Inst(halt(0), Some(4))],
        }
    }

    #[test]
    fn direct_call_relaxation_reaches_a_fixed_point() {
        // With all calls at three words, offsets are 130, 127, and 124.
        // The latter two shrink first; their four removed words then make the
        // first call reachable on the next layout pass.
        let mut lines = vec![
            direct_call("target", 0),
            direct_call("target", 1),
            direct_call("target", 2),
        ];
        lines.extend(padding(120));
        let main = EmittedFunc {
            name: "main",
            lines,
        };

        let linked = link(&[main, target_function()]);
        assert_eq!(
            linked
                .instructions
                .iter()
                .filter(|inst| matches!(inst, Instruction::call_rel(..)))
                .count(),
            3
        );
        assert!(linked.far_calls.is_empty());
        assert_eq!(linked.layout[0].1, (0, 123));
    }

    #[test]
    fn out_of_range_direct_call_keeps_the_three_word_encoding() {
        let mut lines = vec![direct_call("target", 0)];
        lines.extend(padding(128));
        let main = EmittedFunc {
            name: "main",
            lines,
        };

        let linked = link(&[main, target_function()]);
        assert!(matches!(linked.instructions[0], Instruction::load_lo(..)));
        assert!(matches!(linked.instructions[1], Instruction::load_hi(..)));
        assert!(matches!(linked.instructions[2], Instruction::call_reg(15)));
        assert_eq!(linked.calls.get(&2), Some(&"target"));
        assert_eq!(linked.far_calls.len(), 1);
    }

    #[test]
    fn call_shrinking_can_restore_a_short_branch() {
        let end = 99;
        let mut lines = vec![MachineLine::Branch {
            cond: Cond::Always,
            target: end,
            line: Some(1),
        }];
        lines.extend([
            direct_call("target", 0),
            direct_call("target", 1),
            direct_call("target", 2),
        ]);
        lines.extend(padding(120));
        lines.push(MachineLine::Label(end));
        let main = EmittedFunc {
            name: "main",
            lines,
        };

        let linked = link(&[main, target_function()]);
        assert!(matches!(linked.instructions[0], Instruction::jmp(..)));
        assert!(linked.far_calls.is_empty());
        assert_eq!(linked.layout[0].1, (0, 124));
    }
}
