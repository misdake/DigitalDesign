//! interactive debugger session: step/breakpoints/variable inspection over a
//! compiled rcc program (binary image + `.dbg` debug info).

use crate::{DebugFunc, DebugInfo, DebugVar, Instruction, SimEnv, VarLoc, decode_binary, parse_debug};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

pub struct DebugSession {
    pub sim: SimEnv,
    pub debug: DebugInfo,
    pub breakpoints: HashSet<usize>,
    pub disasm: Vec<DisasmLine>,
    instructions: Vec<Instruction>,
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
}

pub struct DisasmLine {
    pub addr: usize,
    pub text: String,
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

        // disassembly: prefer the listing file (has comments), else decode
        let lst_path = bin_path.with_extension("lst");
        let disasm = match std::fs::read_to_string(&lst_path) {
            Ok(text) => parse_listing(&text),
            Err(_) => instructions
                .iter()
                .enumerate()
                .map(|(addr, i)| DisasmLine {
                    addr,
                    text: i.to_string(),
                })
                .collect(),
        };

        Ok(DebugSession {
            sim: SimEnv::new(&instructions),
            debug,
            breakpoints: HashSet::new(),
            disasm,
            instructions,
            last_halt: None,
            call_stack: vec![],
        })
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

    /// current source line (file index, line) closest to `pc`: the nearest
    /// entry at or below pc, else the first entry above it (before the
    /// function prologue)
    pub fn current_line(&self) -> Option<(u16, u32)> {
        let pc = self.sim.state.pc as usize;
        let mut best: Option<&(usize, u16, u32)> = None;
        for entry in &self.debug.lines {
            if entry.0 <= pc && best.is_none_or(|b| entry.0 >= b.0) {
                best = Some(entry);
            }
        }
        if best.is_none() {
            best = self
                .debug
                .lines
                .iter()
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
            VarLoc::Param(r) => VarValue::Reg(self.sim.state.reg[r as usize]),
            VarLoc::Ssa => VarValue::Unavailable,
        }
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
    Reg(u16),
    Unavailable,
}

/// parse the `.lst` listing into disassembly lines
fn parse_listing(text: &str) -> Vec<DisasmLine> {
    let mut out = vec![];
    for line in text.lines() {
        let t = line.trim_start();
        if t.len() >= 5 && t.as_bytes()[4] == b':' {
            if let Ok(addr) = usize::from_str_radix(&t[..4], 16) {
                out.push(DisasmLine {
                    addr,
                    text: t[5..].trim().to_string(),
                });
            }
        }
    }
    out
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
                format!(
                    "{{\"func\":\"{}\",\"ret\":{}}}",
                    esc(&f.func_name),
                    f.return_addr
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
                format!(
                    "{{\"addr\":{},\"text\":\"{}\"}}",
                    d.addr,
                    esc(&d.text)
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
            VarValue::Reg(x) => format!("{{\"reg\":{x}}}"),
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
