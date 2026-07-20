//! interactive debugger session: step/breakpoints/variable inspection over a
//! compiled rcc program (binary image + `.dbg` debug info).

use crate::{DebugFunc, DebugInfo, DebugVar, Instruction, SimEnv, VarLoc, decode_binary, parse_debug};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

pub struct DebugSession {
    pub sim: SimEnv,
    pub debug: DebugInfo,
    pub breakpoints: HashSet<usize>,
    pub disasm: Vec<DisasmLine>,
    instructions: Vec<Instruction>,
    /// In-memory source files, used by the playground. Other files fall back
    /// to the paths recorded in `DebugInfo`.
    source_overrides: HashMap<u16, String>,
    /// halt signal once a halt instruction has executed
    pub last_halt: Option<u16>,
    /// shadow call stack, maintained by watching call/ret instructions
    pub call_stack: Vec<CallFrame>,
}

#[derive(Clone, Debug)]
pub struct CallFrame {
    /// address the call jumped to (function entry)
    pub func_addr: usize,
    /// address control returns to (pc after the call)
    pub return_addr: usize,
    /// function name resolved from the debug ranges
    pub func_name: String,
    /// ABI argument registers captured at the call boundary. Parameters are
    /// immutable in rcc, but r2-r7 are caller-save and may be overwritten by
    /// nested calls, so reading the live register later is not reliable.
    pub arg_values: [u16; 6],
}

pub struct DisasmLine {
    pub addr: usize,
    pub text: String,
    /// jump/call target address for drawing arrows (None for straight-line code)
    pub target: Option<usize>,
    /// function name at the target, when known
    pub target_name: Option<String>,
    /// header row: the padding halt before a function, showing its signature
    pub header: bool,
}

impl DebugSession {
    /// load a binary image plus its `.dbg` (and `.lst` if present)
    pub fn load(bin_path: &Path) -> Result<DebugSession, String> {
        let bytes = std::fs::read(bin_path).map_err(|e| format!("cannot read {}: {e}", bin_path.display()))?;
        let instructions = decode_binary(&bytes).ok_or_else(|| format!("{} is not an rcc binary", bin_path.display()))?;

        let dbg_path = bin_path.with_extension("dbg");
        let debug = match std::fs::read_to_string(&dbg_path) {
            Ok(text) => parse_debug(&text)?,
            Err(_) => DebugInfo::default(),
        };

        // disassembly: one row per image address. The padding halt between
        // functions doubles as the next function's header row; instruction
        // text comes from the listing when available (it has comments).
        let lst_path = bin_path.with_extension("lst");
        let listing = std::fs::read_to_string(&lst_path).unwrap_or_default();
        Ok(Self::from_compiled(instructions, listing, debug, HashMap::new()))
    }

    /// Build a debugger session directly from compiler output. `sources`
    /// maps debug file indices to source text and avoids temporary artifacts.
    pub fn from_compiled(
        instructions: Vec<Instruction>,
        listing: String,
        debug: DebugInfo,
        source_overrides: HashMap<u16, String>,
    ) -> DebugSession {
        let lst_text = parse_listing(&listing);
        let starts: HashMap<usize, &DebugFunc> =
            debug.functions.iter().map(|f| (f.addr.0, f)).collect();
        let in_function = |addr: usize| {
            debug
                .functions
                .iter()
                .any(|f| (f.addr.0..f.addr.1).contains(&addr))
        };
        let mut disasm: Vec<DisasmLine> = Vec::with_capacity(instructions.len());
        for (addr, inst) in instructions.iter().enumerate() {
            if let Some(f) = starts.get(&(addr + 1)) {
                if matches!(inst, Instruction::halt(0)) && !in_function(addr) {
                    disasm.push(DisasmLine {
                        addr,
                        text: signature(f),
                        target: None,
                        target_name: None,
                        header: true,
                    });
                    continue;
                }
            }
            disasm.push(DisasmLine {
                addr,
                text: lst_text.get(&addr).cloned().unwrap_or_else(|| inst.to_string()),
                target: None,
                target_name: None,
                header: false,
            });
        }
        annotate_targets(&mut disasm, &instructions, &debug);

        DebugSession {
            sim: SimEnv::new(&instructions),
            debug,
            breakpoints: HashSet::new(),
            disasm,
            instructions,
            source_overrides,
            last_halt: None,
            call_stack: vec![],
        }
    }

    pub fn reset(&mut self) {
        self.sim = SimEnv::new(&self.instructions);
        self.last_halt = None;
        self.call_stack.clear();
    }

    /// execute exactly one instruction, recording a halt if it happens and
    /// maintaining the shadow call stack
    fn step_change(&mut self) -> crate::StateChange {
        let pc = self.sim.state.pc;
        let inst = self.sim.inst[pc as usize];
        let change = self.sim.eval();
        // watch control flow for the shadow call stack
        use crate::Instruction as I;
        match inst {
            I::call_rel(..) | I::call_abs(..) | I::call_reg(_) => {
                let arg_values = crate::ARG_REGS
                    .map(|r| self.sim.state.reg[r as usize]);
                let func_addr = change.pc_next as usize;
                let func_name = self
                    .debug
                    .functions
                    .iter()
                    .find(|f| (f.addr.0..f.addr.1).contains(&func_addr))
                    .map(|f| f.name.clone())
                    .unwrap_or_else(|| format!("0x{func_addr:04x}"));
                self.call_stack.push(CallFrame {
                    func_addr,
                    return_addr: (pc + 1) as usize,
                    func_name,
                    arg_values,
                });
            }
            I::jmp_reg(13) => {
                self.call_stack.pop();
            }
            _ => {}
        }
        if change.halt.is_some() {
            self.last_halt = change.halt;
        }
        self.sim.commit(change);
        change
    }

    /// shadow call depth (used by step-over/step-out)
    pub fn depth(&self) -> usize {
        self.call_stack.len()
    }

    /// step over: run until the source line changes without going deeper
    /// (calls are executed but not stepped into)
    pub fn step_over(&mut self, max_cycles: usize) -> Option<u16> {
        if self.last_halt.is_some() {
            return self.last_halt;
        }
        let cur = self.current_line();
        let depth0 = self.depth();
        for _ in 0..max_cycles {
            self.step_change();
            if let Some(sig) = self.last_halt {
                return Some(sig);
            }
            if self.breakpoints.contains(&(self.sim.state.pc as usize)) {
                return None;
            }
            if self.depth() <= depth0 && self.current_line() != cur {
                return None;
            }
        }
        None
    }

    /// step out: run until the current function returns
    pub fn step_out(&mut self, max_cycles: usize) -> Option<u16> {
        if self.last_halt.is_some() {
            return self.last_halt;
        }
        if self.depth() == 0 {
            return None;
        }
        let depth0 = self.depth();
        for _ in 0..max_cycles {
            self.step_change();
            if let Some(sig) = self.last_halt {
                return Some(sig);
            }
            if self.breakpoints.contains(&(self.sim.state.pc as usize)) {
                return None;
            }
            if self.depth() < depth0 {
                return None;
            }
        }
        None
    }

    /// execute exactly one instruction
    pub fn step(&mut self) {
        if self.last_halt.is_some() {
            return;
        }
        self.step_change();
    }

    /// run until a breakpoint is hit, the program halts, or `max_cycles` pass.
    /// returns (hit_breakpoint?, halted signal)
    pub fn continue_run(&mut self, max_cycles: usize) -> (Option<usize>, Option<u16>) {
        if let Some(sig) = self.last_halt {
            return (None, Some(sig));
        }
        // the first step always runs, so a breakpoint at the current pc does
        // not immediately re-trigger
        let change = self.step_change();
        if let Some(sig) = change.halt {
            return (None, Some(sig));
        }
        for _ in 0..max_cycles {
            if self.breakpoints.contains(&(self.sim.state.pc as usize)) {
                return (Some(self.sim.state.pc as usize), None);
            }
            let change = self.step_change();
            if let Some(sig) = change.halt {
                return (None, Some(sig));
            }
        }
        (None, None)
    }

    pub fn halted(&self) -> Option<u16> {
        self.last_halt
    }

    pub fn toggle_breakpoint(&mut self, addr: usize, on: bool) {
        if on {
            self.breakpoints.insert(addr);
        } else {
            self.breakpoints.remove(&addr);
        }
    }

    /// toggle a breakpoint on the first instruction mapped to a source line.
    /// returns the instruction address if the line has one.
    pub fn toggle_breakpoint_line(&mut self, file: u16, line: u32, on: bool) -> Option<usize> {
        let addr = self
            .debug
            .lines
            .iter()
            .filter(|(_, f, l)| *f == file && *l == line)
            .map(|&(a, _, _)| a)
            .min()?;
        self.toggle_breakpoint(addr, on);
        Some(addr)
    }

    /// line numbers (of the current file) that have a breakpoint mapped to them
    fn breakpoint_lines(&self, file: u16) -> Vec<u32> {
        self.debug
            .lines
            .iter()
            .filter(|(a, f, _)| *f == file && self.breakpoints.contains(a))
            .map(|&(_, _, l)| l)
            .collect()
    }

    /// step until the current source line changes (or the program halts)
    pub fn next_line(&mut self, max_cycles: usize) -> Option<u16> {
        if self.last_halt.is_some() {
            return self.last_halt;
        }
        let cur = self.current_line();
        for _ in 0..max_cycles {
            self.step_change();
            if let Some(sig) = self.last_halt {
                return Some(sig);
            }
            if self.breakpoints.contains(&(self.sim.state.pc as usize)) {
                return None;
            }
            if self.current_line() != cur {
                return None;
            }
        }
        None
    }

    /// the function containing `pc`, if any
    pub fn current_func(&self) -> Option<&DebugFunc> {
        let pc = self.sim.state.pc as usize;
        self.debug
            .functions
            .iter()
            .find(|f| (f.addr.0..f.addr.1).contains(&pc))
    }

    /// Current source line (file index, line) closest to `pc` within the
    /// current function. Restricting by address range is essential: several
    /// functions commonly share a source file, and a prologue has no line
    /// entry of its own.
    pub fn current_line(&self) -> Option<(u16, u32)> {
        let pc = self.sim.state.pc as usize;
        let func = self.current_func()?;
        let entries = self
            .debug
            .lines
            .iter()
            .filter(|(addr, file, _)| {
                *file == func.file && (func.addr.0..func.addr.1).contains(addr)
            });
        let mut best: Option<&(usize, u16, u32)> = None;
        for entry in entries {
            if entry.0 <= pc && best.is_none_or(|b| entry.0 >= b.0) {
                best = Some(entry);
            }
        }
        if best.is_none() {
            best = self
                .debug
                .lines
                .iter()
                .filter(|(addr, file, _)| {
                    *file == func.file && (func.addr.0..func.addr.1).contains(addr)
                })
                .filter(|(a, _, _)| *a >= pc)
                .min_by_key(|(a, _, _)| *a);
        }
        best.map(|&(_, f, l)| (f, l))
    }

    /// value of a variable (best effort by its location)
    pub fn var_value(&self, v: &DebugVar) -> VarValue {
        match v.loc {
            VarLoc::Global(addr) => VarValue::Mem(addr, self.preview(addr, &v.ty)),
            VarLoc::Frame(slot) => {
                let addr = self.sim.state.reg[14].wrapping_add(slot as u16);
                VarValue::Mem(addr, self.preview(addr, &v.ty))
            }
            VarLoc::Param(r) => {
                let saved = self.current_func().and_then(|func| {
                    self.call_stack
                        .last()
                        .filter(|frame| frame.func_addr == func.addr.0)
                        .and_then(|frame| {
                            crate::ARG_REGS
                                .iter()
                                .position(|arg_reg| *arg_reg == r)
                                .map(|i| frame.arg_values[i])
                        })
                });
                VarValue::Reg(r, saved.unwrap_or(self.sim.state.reg[r as usize]))
            }
            VarLoc::Ssa => VarValue::Unavailable,
        }
    }

    fn local_is_visible(&self, func: &DebugFunc, v: &DebugVar) -> bool {
        let in_scope = v.scope.is_none_or(|(start, end)| {
            self.current_line()
                .is_some_and(|(_, line)| (start..=end).contains(&line))
        });
        if !in_scope || !matches!(v.loc, VarLoc::Frame(_)) {
            return in_scope;
        }

        let pc = self.sim.state.pc as usize;
        let first_source = self
            .debug
            .lines
            .iter()
            .filter(|(addr, file, _)| {
                *file == func.file && (func.addr.0..func.addr.1).contains(addr)
            })
            .map(|(addr, _, _)| *addr)
            .min();
        if first_source.is_none_or(|first| pc < first) {
            return false;
        }

        // sp_add has already restored the caller's SP when execution reaches
        // the return jump, so frame-relative locals no longer have a valid
        // address at that point.
        !matches!(self.instructions.get(pc), Some(Instruction::jmp_reg(13)))
            || !matches!(self.instructions.get(pc.saturating_sub(1)), Some(Instruction::sp_add(..)))
    }

    /// read a word, interpreting it by the type string
    fn preview(&self, addr: u16, ty: &str) -> Vec<u16> {
        let mem = self.sim.state.mem.as_slice();
        let n = if ty.starts_with('[') {
            // [T; N] arrays: preview up to 8 elements
            ty.split(';')
                .nth(1)
                .and_then(|t| t.trim_end_matches(']').trim().parse::<usize>().ok())
                .unwrap_or(1)
                .min(8)
        } else {
            1
        };
        (0..n as u16)
            .map(|i| mem[addr.wrapping_add(i) as usize])
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum VarValue {
    Mem(u16, Vec<u16>),
    Reg(u8, u16),
    Unavailable,
}

/// compute jump/call targets for the arrow overlay
fn annotate_targets(disasm: &mut [DisasmLine], instructions: &[Instruction], debug: &DebugInfo) {
    use crate::Instruction as I;
    for line in disasm.iter_mut() {
        if line.header {
            continue;
        }
        let addr = line.addr;
        if addr >= instructions.len() {
            continue;
        }
        let (target, is_call) = match instructions[addr] {
            I::jg(hi, lo) | I::je(hi, lo) | I::jge(hi, lo) | I::jl(hi, lo) | I::jne(hi, lo)
            | I::jle(hi, lo) | I::jmp(hi, lo) => {
                let off = crate::isa::imm8_as_i16(hi, lo) as i32;
                let t = addr as i32 + off;
                if (0..65536).contains(&t) {
                    (Some(t as usize), false)
                } else {
                    (None, false)
                }
            }
            I::call_rel(hi, lo) => {
                let off = crate::isa::imm8_as_i16(hi, lo) as i32;
                let t = addr as i32 + off;
                if (0..65536).contains(&t) {
                    (Some(t as usize), true)
                } else {
                    (None, false)
                }
            }
            I::jmp_reg(r) | I::call_reg(r) => {
                // far jump/call: load_lo + load_hi immediately before
                let is_call = matches!(instructions[addr], I::call_reg(_));
                let _ = r;
                if addr >= 2 {
                    let (I::load_hi(hi2, lo2, _), I::load_lo(hi1, lo1, _)) =
                        (instructions[addr - 1], instructions[addr - 2])
                    else {
                        continue;
                    };
                    let t = ((hi2 as usize) << 12)
                        | ((lo2 as usize) << 8)
                        | ((hi1 as usize) << 4)
                        | lo1 as usize;
                    (Some(t), is_call)
                } else {
                    (None, false)
                }
            }
            _ => (None, false),
        };
        line.target = target;
        if is_call {
            if let Some(t) = target {
                line.target_name = debug
                    .functions
                    .iter()
                    .find(|f| (f.addr.0..f.addr.1).contains(&t))
                    .map(|f| f.name.clone());
            }
        } else {
            let _ = is_call;
        }
    }
}

/// parse the `.lst` listing into an address -> text map
fn parse_listing(text: &str) -> HashMap<usize, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.len() >= 5 && t.as_bytes()[4] == b':' {
            if let Ok(addr) = usize::from_str_radix(&t[..4], 16) {
                out.insert(addr, t[5..].trim().to_string());
            }
        }
    }
    out
}

/// `fn name(x: i16, y: u16)` signature for a disassembly header row
fn signature(f: &DebugFunc) -> String {
    let mut params: Vec<&DebugVar> = f
        .locals
        .iter()
        .filter(|v| matches!(v.loc, VarLoc::Param(_)))
        .collect();
    params.sort_by_key(|v| match v.loc {
        VarLoc::Param(r) => r,
        _ => 0,
    });
    let params: Vec<String> = params
        .iter()
        .map(|v| format!("{}: {}", v.name, v.ty))
        .collect();
    format!("fn {}({})", f.name, params.join(", "))
}

// ---------------------------------------------------------------------------
// JSON state snapshot for the web UI (hand-rolled, no serde)
// ---------------------------------------------------------------------------

fn esc(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn ty_to_json(ty: &str) -> String {
    format!("\"{}\"", esc(ty))
}

impl DebugSession {
    pub fn state_json(&self) -> String {
        let mut out = String::from("{");
        let st = &self.sim.state;
        let _ = write!(out, "\"pc\":{},\"cycles\":{},", st.pc, st.cycles);
        let halted = self.halted();
        match halted {
            Some(h) => {
                let _ = write!(out, "\"halted\":{h},");
            }
            None => {
                let _ = write!(out, "\"halted\":null,");
            }
        }
        let _ = write!(out, "\"flags\":{},", st.flags);
        let _ = write!(
            out,
            "\"regs\":[{}],",
            st.reg.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(",")
        );
        let _ = write!(
            out,
            "\"breakpoints\":[{}],",
            self.breakpoints.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(",")
        );

        // current function and its locals
        match self.current_func() {
            Some(f) => {
                let _ = write!(
                    out,
                    "\"func\":{{\"name\":\"{}\",\"addr\":[{},{}],\"frame\":{},\"file\":{},",
                    esc(&f.name), f.addr.0, f.addr.1, f.frame_size, f.file
                );
                let _ = write!(out, "\"locals\":[");
                let locals: Vec<String> = f
                    .locals
                    .iter()
                    .filter(|v| self.local_is_visible(f, v))
                    .map(|v| self.var_json(v))
                    .collect();
                let _ = write!(out, "{}", locals.join(","));
                let _ = write!(out, "]}},");
            }
            None => {
                let _ = write!(out, "\"func\":null,");
            }
        }

        // globals and consts
        let globals: Vec<String> = self.debug.globals.iter().map(|v| self.var_json(v)).collect();
        let _ = write!(out, "\"globals\":[{}],", globals.join(","));
        let consts: Vec<String> = self
            .debug
            .consts
            .iter()
            .map(|(n, ty, v)| format!("{{\"name\":\"{}\",\"ty\":{},\"value\":{}}}", esc(n), ty_to_json(ty), v))
            .collect();
        let _ = write!(out, "\"consts\":[{}],", consts.join(","));

        // current line + source snippet
        match self.current_line() {
            Some((file, line)) => {
                let snippet = self.source_line(file, line);
                let _ = write!(
                    out,
                    "\"line\":{{\"file\":{},\"line\":{},\"text\":\"{}\"}},",
                    file,
                    line,
                    esc(&snippet)
                );
            }
            None => {
                let _ = write!(out, "\"line\":null,");
            }
        }

        // shadow call stack (innermost last)
        let stack: Vec<String> = self
            .call_stack
            .iter()
            .map(|f| {
                // the call site is the instruction before the return address
                let site_addr = f.return_addr.saturating_sub(1);
                let caller = self
                    .debug
                    .functions
                    .iter()
                    .find(|func| (func.addr.0..func.addr.1).contains(&site_addr));
                let site = self
                    .debug
                    .lines
                    .iter()
                    .filter(|(a, file, _)| {
                        caller.is_some_and(|func| {
                            *file == func.file
                                && (func.addr.0..func.addr.1).contains(a)
                                && *a <= site_addr
                        })
                    })
                    .max_by_key(|(a, _, _)| *a);
                let (sf, sl) = site.map(|&(_, f, l)| (f, l)).unwrap_or((0, 0));
                format!(
                    "{{\"func\":\"{}\",\"ret\":{},\"site_file\":{},\"site_line\":{}}}",
                    esc(&f.func_name),
                    f.return_addr,
                    sf,
                    sl
                )
            })
            .collect();
        let _ = write!(out, "\"stack\":[{}],", stack.join(","));

        // current source file with line numbers and breakpoint lines
        match self.current_line() {
            Some((file, _)) => {
                let lines = self.source_lines(file);
                let bps = self.breakpoint_lines(file);
                let _ = write!(out, "\"src\":{{\"file\":{},\"lines\":[", file);
                let items: Vec<String> = lines
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        let n = i + 1;
                        let cur = self.current_line() == Some((file, n as u32));
                        let bp = bps.contains(&(n as u32));
                        format!(
                            "{{\"n\":{},\"text\":\"{}\",\"cur\":{},\"bp\":{}}}",
                            n,
                            esc(t),
                            cur,
                            bp
                        )
                    })
                    .collect();
                let _ = write!(out, "{}", items.join(","));
                // address->line map of this file (for source<->disasm highlight)
                let map: Vec<String> = self
                    .debug
                    .lines
                    .iter()
                    .filter(|(_, f, _)| *f == file)
                    .map(|&(a, _, l)| format!("[{},{}]", a, l))
                    .collect();
                let _ = write!(out, "],\"map\":[{}]}},", map.join(","));
            }
            None => {
                let _ = write!(out, "\"src\":null,");
            }
        }

        // disassembly with pc/breakpoint markers
        let _ = write!(out, "\"disasm\":[");
        let items: Vec<String> = self
            .disasm
            .iter()
            .map(|d| {
                let target = match (d.target, &d.target_name) {
                    (Some(t), Some(n)) => format!(",\"target\":{},\"tname\":\"{}\"", t, esc(n)),
                    (Some(t), None) => format!(",\"target\":{}", t),
                    _ => String::new(),
                };
                let header = if d.header { ",\"header\":true" } else { "" };
                format!(
                    "{{\"addr\":{},\"text\":\"{}\"{}{}}}",
                    d.addr,
                    esc(&d.text),
                    target,
                    header
                )
            })
            .collect();
        let _ = write!(out, "{}", items.join(","));
        out.push_str("]}");
        out
    }

    fn var_json(&self, v: &DebugVar) -> String {
        let value = match self.var_value(v) {
            VarValue::Mem(addr, words) => format!(
                "{{\"addr\":{},\"words\":[{}]}}",
                addr,
                words.iter().map(|w| w.to_string()).collect::<Vec<_>>().join(",")
            ),
            VarValue::Reg(reg, word) => format!("{{\"reg\":{reg},\"word\":{word}}}"),
            VarValue::Unavailable => "null".to_string(),
        };
        format!(
            "{{\"name\":\"{}\",\"ty\":{},\"value\":{}}}",
            esc(&v.name),
            ty_to_json(&v.ty),
            value
        )
    }

    fn source_line(&self, file: u16, line: u32) -> String {
        if let Some(text) = self.source_overrides.get(&file) {
            return text
                .lines()
                .nth(line as usize - 1)
                .unwrap_or("")
                .trim()
                .to_string();
        }
        let Some(path) = self.debug.files.get(file as usize) else {
            return String::new();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return String::new();
        };
        text.lines().nth(line as usize - 1).unwrap_or("").trim().to_string()
    }

    /// full lines of a source file (empty when unreadable)
    fn source_lines(&self, file: u16) -> Vec<String> {
        if let Some(text) = self.source_overrides.get(&file) {
            return text.lines().map(|line| line.to_string()).collect();
        }
        let Some(path) = self.debug.files.get(file as usize) else {
            return vec![];
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return vec![];
        };
        text.lines().map(|l| l.to_string()).collect()
    }

    pub fn mem_json(&self, addr: u16, len: u16) -> String {
        let mem = self.sim.state.mem.as_slice();
        let words: Vec<String> = (0..len)
            .map(|i| mem[addr.wrapping_add(i) as usize].to_string())
            .collect();
        format!("{{\"addr\":{addr},\"words\":[{}]}}", words.join(","))
    }
}
