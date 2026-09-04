//! interactive debugger session for CpuV3 programs: step/breakpoints/variable
//! inspection over the instruction-level `Machine` and a `.dbg`-derived
//! `DebugInfo`.
//!
//! The session drives the architectural `Machine` directly (registers, FPU
//! vectors, physical memory) and never touches the RTL. Addresses in the
//! debug info are code-base-relative word offsets, which match `Machine::pc`
//! while the code segment is zero (segment switching is outside the debugger's
//! scope).

use cpu_v3::rcc_backend::CpuV3Program;
use cpu_v3::{Fault, FaultKind, Instruction, Machine, PhysicalWordAddress, StepOutcome, Word};
use rcc::{DebugFunc, DebugInfo, DebugVar, VarLoc};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// ABI argument registers r2-r7, in call order.
const ARG_REGS: [u8; 6] = [2, 3, 4, 5, 6, 7];
const STACK_REGISTER: u8 = 13;
const LINK_REGISTER: u8 = 14;
/// upper bound on the steps taken while executing a library call as a unit
const LIBRARY_STEP_LIMIT: usize = 10_000_000;

pub struct V3DebugSession {
    pub machine: Machine,
    pub debug: DebugInfo,
    pub breakpoints: HashSet<usize>,
    pub disasm: Vec<DisasmLine>,
    code_base: Word,
    words: Vec<Word>,
    /// in-memory source files (tests); empty in the standalone debugger
    source_overrides: HashMap<u16, String>,
    /// directory of the main source file, for resolving bare module names
    source_root: PathBuf,
    /// halt signal once a halt instruction has executed
    pub last_halt: Option<u16>,
    /// fault that stopped execution, if any
    pub fault: Option<Fault>,
    /// shadow call stack, maintained by watching call/return instructions
    pub call_stack: Vec<CallFrame>,
    /// session step counter (displayed as cycles)
    steps: u64,
}

#[derive(Clone, Debug)]
pub struct CallFrame {
    /// address the call jumped to (function entry)
    pub func_addr: usize,
    /// address control returns to (pc after the call)
    pub return_addr: usize,
    pub func_name: String,
    /// ABI argument registers captured at the call boundary. Arguments are
    /// caller-save, so reading the live register later is not reliable.
    pub arg_values: [u16; 6],
    /// the callee's source file is an `rcc_std/` library file
    pub library: bool,
}

pub struct DisasmLine {
    pub addr: usize,
    pub text: String,
    pub wide: bool,
    /// true for call instructions (JALREL/JALR)
    pub call: bool,
    /// static jump/call target, when the offset is known at load time
    pub target: Option<usize>,
    /// function name at the target, when known
    pub target_name: Option<String>,
    /// header row showing a function signature
    pub header: bool,
    /// the function's source file is an `rcc_std/` library file
    pub library: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum VarValue {
    Mem(u16, Vec<u16>),
    Reg(u8, u16),
    Unavailable,
}

/// what changed in a single machine step, for the step-over/step-out loops
struct StepChange {
    entered_library: bool,
}

impl V3DebugSession {
    pub fn from_program(program: CpuV3Program) -> V3DebugSession {
        Self::from_program_with_sources(program, HashMap::new())
    }

    pub fn from_program_with_sources(
        program: CpuV3Program,
        mut source_overrides: HashMap<u16, String>,
    ) -> V3DebugSession {
        // inject the embedded standard library sources so library tabs render
        for (name, text) in rcc::frontend::std_sources() {
            if let Some(index) = program.debug.files.iter().position(|f| f == name) {
                source_overrides.entry(index as u16).or_insert_with(|| text.to_string());
            }
        }
        let code_base = program.code_base;
        let words = program.words;
        let source_root = program
            .debug
            .files
            .first()
            .map(Path::new)
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut machine = Machine::default();
        load_machine(&mut machine, code_base, &words);
        let disasm = build_disasm(&words, code_base, &program.debug);

        V3DebugSession {
            machine,
            debug: program.debug,
            breakpoints: HashSet::new(),
            disasm,
            code_base,
            words,
            source_overrides,
            source_root,
            last_halt: None,
            fault: None,
            call_stack: vec![],
            steps: 0,
        }
    }

    pub fn reset(&mut self) {
        let mut machine = Machine::default();
        load_machine(&mut machine, self.code_base, &self.words);
        self.machine = machine;
        self.last_halt = None;
        self.fault = None;
        self.call_stack.clear();
        self.steps = 0;
    }

    /// execute one logical instruction (a wide operation is one unit),
    /// recording a halt/fault and maintaining the shadow call stack
    fn step_change(&mut self) -> StepChange {
        let mut inst = self.fetch_inst(self.machine.pc());
        // a wide operation starts with a prefix word; skip it so call/return
        // detection and stepping observe whole instructions, not half of one
        if matches!(inst, Instruction::Prefix { .. }) {
            self.machine_step();
            inst = self.fetch_inst(self.machine.pc());
        }

        let is_call = matches!(
            inst,
            Instruction::JumpRelative { link: true, .. } | Instruction::JumpAndLinkRegister { .. }
        );
        let is_return = matches!(inst, Instruction::JumpRegister { target: LINK_REGISTER });

        let arg_values = if is_call {
            let regs = self.machine.registers();
            std::array::from_fn(|i| regs[usize::from(ARG_REGS[i])])
        } else {
            [0; 6]
        };

        self.machine_step();
        if self.last_halt.is_some() || self.fault.is_some() {
            return StepChange {
                entered_library: false,
            };
        }

        if is_call {
            let func_addr = self.machine.pc() as usize;
            let return_addr = self.machine.register(LINK_REGISTER).unwrap_or(0) as usize;
            let func_name = self
                .debug
                .functions
                .iter()
                .find(|f| (f.addr.0..f.addr.1).contains(&func_addr))
                .map(|f| f.name.clone())
                .unwrap_or_else(|| format!("0x{func_addr:04x}"));
            let library = self
                .debug
                .functions
                .iter()
                .find(|f| (f.addr.0..f.addr.1).contains(&func_addr))
                .is_some_and(|f| self.is_library(f));
            self.call_stack.push(CallFrame {
                func_addr,
                return_addr,
                func_name,
                arg_values,
                library,
            });
        } else if is_return {
            self.call_stack.pop();
        }

        StepChange {
            entered_library: is_call && self.call_stack.last().is_some_and(|f| f.library),
        }
    }

    fn fetch_inst(&self, pc: Word) -> Instruction {
        let word = self.machine.physical_memory(PhysicalWordAddress::from_segment_offset(
            self.machine.code_segment(),
            pc,
        ));
        cpu_v3::decode(word)
    }

    fn machine_step(&mut self) {
        match self.machine.step() {
            Ok(StepOutcome::Halted { signal }) => self.last_halt = Some(signal),
            Ok(StepOutcome::Running) => {}
            Err(fault) => self.fault = Some(fault),
        }
        self.steps += 1;
    }

    /// one step, draining a library call so it appears as a single unit
    fn step_once(&mut self, max: usize) {
        if self.last_halt.is_some() || self.fault.is_some() {
            return;
        }
        let change = self.step_change();
        if change.entered_library {
            let target_depth = self.depth().saturating_sub(1);
            for _ in 0..max {
                self.step_change();
                if self.last_halt.is_some() || self.fault.is_some() || self.depth() <= target_depth {
                    break;
                }
            }
        }
    }

    /// shadow call depth (used by step-over/step-out)
    pub fn depth(&self) -> usize {
        self.call_stack.len()
    }

    pub fn step(&mut self) {
        self.step_once(LIBRARY_STEP_LIMIT);
    }

    /// step until the current source line changes (or the program halts)
    pub fn next_line(&mut self, max_cycles: usize) {
        if self.last_halt.is_some() || self.fault.is_some() {
            return;
        }
        let cur = self.current_line();
        for _ in 0..max_cycles {
            self.step_once(max_cycles);
            if self.last_halt.is_some() || self.fault.is_some() {
                return;
            }
            if self.breakpoints.contains(&(self.machine.pc() as usize)) {
                return;
            }
            if self.current_line() != cur {
                return;
            }
        }
    }

    /// step over: run until the source line changes without going deeper
    pub fn step_over(&mut self, max_cycles: usize) {
        if self.last_halt.is_some() || self.fault.is_some() {
            return;
        }
        let cur = self.current_line();
        let depth0 = self.depth();
        for _ in 0..max_cycles {
            self.step_once(max_cycles);
            if self.last_halt.is_some() || self.fault.is_some() {
                return;
            }
            if self.breakpoints.contains(&(self.machine.pc() as usize)) {
                return;
            }
            if self.depth() <= depth0 && self.current_line() != cur {
                return;
            }
        }
    }

    /// step out: run until the current function returns
    pub fn step_out(&mut self, max_cycles: usize) {
        if self.last_halt.is_some() || self.fault.is_some() {
            return;
        }
        if self.depth() == 0 {
            return;
        }
        let depth0 = self.depth();
        for _ in 0..max_cycles {
            self.step_once(max_cycles);
            if self.last_halt.is_some() || self.fault.is_some() {
                return;
            }
            if self.breakpoints.contains(&(self.machine.pc() as usize)) {
                return;
            }
            if self.depth() < depth0 {
                return;
            }
        }
    }

    /// run until a breakpoint is hit, the program halts, or `max_cycles` pass
    pub fn continue_run(&mut self, max_cycles: usize) -> (Option<usize>, Option<u16>) {
        if let Some(sig) = self.last_halt {
            return (None, Some(sig));
        }
        if self.fault.is_some() {
            return (None, None);
        }
        self.step_once(max_cycles);
        if self.last_halt.is_some() || self.fault.is_some() {
            return (None, self.last_halt);
        }
        for _ in 0..max_cycles {
            if self.breakpoints.contains(&(self.machine.pc() as usize)) {
                return (Some(self.machine.pc() as usize), None);
            }
            self.step_once(max_cycles);
            if self.last_halt.is_some() || self.fault.is_some() {
                return (None, self.last_halt);
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

    /// toggle a breakpoint on the first instruction mapped to a source line
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

    /// the function containing `pc`, if any
    pub fn current_func(&self) -> Option<&DebugFunc> {
        let pc = self.machine.pc() as usize;
        self.debug
            .functions
            .iter()
            .find(|f| (f.addr.0..f.addr.1).contains(&pc))
    }

    /// current source line (file index, line) closest to `pc` within the
    /// current function, as in the v2 driver
    pub fn current_line(&self) -> Option<(u16, u32)> {
        let pc = self.machine.pc() as usize;
        let func = self.current_func()?;
        let entries = self.debug.lines.iter().filter(|(addr, file, _)| {
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
                let addr = self
                    .machine
                    .registers()[usize::from(STACK_REGISTER)]
                    .wrapping_add(slot as u16);
                VarValue::Mem(addr, self.preview(addr, &v.ty))
            }
            VarLoc::Param(r) => {
                let saved = self.current_func().and_then(|func| {
                    self.call_stack
                        .last()
                        .filter(|frame| frame.func_addr == func.addr.0)
                        .and_then(|frame| {
                            ARG_REGS
                                .iter()
                                .position(|arg_reg| *arg_reg == r)
                                .map(|i| frame.arg_values[i])
                        })
                });
                VarValue::Reg(r, saved.unwrap_or(self.machine.registers()[usize::from(r)]))
            }
            VarLoc::ParamIndex(_) => VarValue::Unavailable,
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

        let pc = self.machine.pc() as usize;
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

        // the SP restore already ran by the time the return jump executes, so
        // frame-relative locals no longer have a valid address at that point.
        let word = self.instruction_word(pc);
        let is_return = matches!(word, Some(Instruction::JumpRegister { target: LINK_REGISTER }));
        let prev_restores_sp = self
            .instruction_word(pc.wrapping_sub(1))
            .is_some_and(|inst| matches!(inst, Instruction::Immediate { op: cpu_v3::ImmediateOp::Add, dst: STACK_REGISTER, .. }));
        !(is_return && prev_restores_sp)
    }

    fn instruction_word(&self, pc: usize) -> Option<Instruction> {
        let offset = pc.checked_sub(self.code_base as usize)?;
        let word = self.words.get(offset)?;
        Some(cpu_v3::decode(*word))
    }

    /// read words, interpreting the type string (array preview like v2)
    fn preview(&self, addr: u16, ty: &str) -> Vec<u16> {
        let n = if ty.starts_with('[') {
            ty.split(';')
                .nth(1)
                .and_then(|t| t.trim_end_matches(']').trim().parse::<usize>().ok())
                .unwrap_or(1)
                .min(8)
        } else {
            1
        };
        (0..n as u16)
            .map(|i| self.data_word(addr.wrapping_add(i)))
            .collect()
    }

    fn data_word(&self, offset: u16) -> u16 {
        self.machine
            .physical_memory(PhysicalWordAddress::from_segment_offset(
                self.machine.data_segment(),
                offset,
            ))
    }

    fn is_library(&self, func: &DebugFunc) -> bool {
        self.debug
            .files
            .get(func.file as usize)
            .is_some_and(|f| f.starts_with("rcc_std/"))
    }
}

/// load a CpuV3 program into a fresh machine and enter it directly at its
/// code base (no register bootstrap, so the pc starts at the first source line)
fn load_machine(machine: &mut Machine, code_base: Word, words: &[Word]) {
    machine.load_program(code_base, words).expect("load program");
    machine.set_pc(code_base);
}

/// build the disassembly view: merged wide operations, function header rows,
/// and static jump/call target annotation
fn build_disasm(words: &[Word], code_base: Word, debug: &DebugInfo) -> Vec<DisasmLine> {
    let starts: HashMap<usize, &DebugFunc> =
        debug.functions.iter().map(|f| (f.addr.0, f)).collect();
    let mut disasm = Vec::new();
    for line in cpu_v3::disassemble_words(words, code_base) {
        let addr = usize::from(line.address);
        if let Some(f) = starts.get(&addr) {
            disasm.push(DisasmLine {
                addr,
                text: signature(f),
                wide: false,
                call: false,
                target: None,
                target_name: None,
                header: true,
                library: debug
                    .files
                    .get(f.file as usize)
                    .is_some_and(|name| name.starts_with("rcc_std/")),
            });
        }
        let base_index = addr - usize::from(code_base);
        let inst = cpu_v3::decode(words[base_index + usize::from(line.wide)]);
        let (call, target) = annotate_target(&inst, line.wide, addr, words, base_index);
        let target_name = if call {
            target.and_then(|t| {
                debug
                    .functions
                    .iter()
                    .find(|f| (f.addr.0..f.addr.1).contains(&t))
                    .map(|f| f.name.clone())
            })
        } else {
            None
        };
        disasm.push(DisasmLine {
            addr,
            text: line.text,
            wide: line.wide,
            call,
            target,
            target_name,
            header: false,
            library: false,
        });
    }
    disasm
}

/// resolve the static target of a branch/jump/call word, if it has one
fn annotate_target(
    inst: &Instruction,
    wide: bool,
    addr: usize,
    words: &[Word],
    base_index: usize,
) -> (bool, Option<usize>) {
    let is_call = matches!(
        inst,
        Instruction::JumpRelative { link: true, .. } | Instruction::JumpAndLinkRegister { .. }
    );
    let offset = match inst {
        Instruction::Branch { offset, .. } | Instruction::JumpRelative { offset, .. } => {
            if wide {
                // the prefix word sits at the wide operation's first word; its
                // low byte is the offset's high byte
                let prefix = words[base_index];
                (((prefix & 0xff) << 8) | (*offset as u16 & 0xff)) as i16
            } else {
                *offset
            }
        }
        // indirect jumps and register calls have no static target
        _ => return (is_call, None),
    };
    let next = addr + if wide { 2 } else { 1 };
    let target = next as i32 + offset as i32;
    if (0..=0xffff).contains(&target) {
        (is_call, Some(target as usize))
    } else {
        (is_call, None)
    }
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

fn fault_kind_name(fault: &Fault) -> String {
    match fault.kind {
        FaultKind::InvalidInstruction => "invalid instruction".to_string(),
        FaultKind::FpuDomain(error) => format!("fpu domain error: {error:?}"),
        FaultKind::MisalignedFpuVectorAddress { offset } => {
            format!("misaligned fpu vector address {offset:#06x}")
        }
        FaultKind::PhysicalAddressOutOfRange { address } => {
            format!("physical address out of range {address:?}")
        }
    }
}

fn ordering_name(ordering: std::cmp::Ordering) -> &'static str {
    match ordering {
        std::cmp::Ordering::Less => "lt",
        std::cmp::Ordering::Equal => "eq",
        std::cmp::Ordering::Greater => "gt",
    }
}

impl V3DebugSession {
    pub fn state_json(&self, view_file: Option<u16>) -> String {
        let mut out = String::from("{");
        let pc = self.machine.pc();
        let _ = write!(
            out,
            "\"pc\":{},\"cseg\":{},\"dseg\":{},\"cycles\":{},\"retired\":{},",
            pc,
            self.machine.code_segment(),
            self.machine.data_segment(),
            self.steps,
            self.machine.retired_words()
        );
        match self.halted() {
            Some(h) => {
                let _ = write!(out, "\"halted\":{h},");
            }
            None => {
                let _ = write!(out, "\"halted\":null,");
            }
        }
        match &self.fault {
            Some(fault) => {
                let _ = write!(
                    out,
                    "\"fault\":{{\"kind\":\"{}\",\"address\":{}}},",
                    esc(&fault_kind_name(fault)),
                    fault.address
                );
            }
            None => {
                let _ = write!(out, "\"fault\":null,");
            }
        }
        match self.machine.pending_test() {
            Some(ordering) => {
                let _ = write!(out, "\"test\":\"{}\",", ordering_name(ordering));
            }
            None => {
                let _ = write!(out, "\"test\":null,");
            }
        }
        let regs = self.machine.registers();
        let _ = write!(
            out,
            "\"regs\":[{}],",
            regs.iter()
                .map(|r| r.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let fpu = self.machine.fpu_registers();
        let _ = write!(
            out,
            "\"fpu\":[{}],",
            fpu.iter()
                .map(|v| format!(
                    "[{}]",
                    v.iter().map(|lane| lane.to_string()).collect::<Vec<_>>().join(",")
                ))
                .collect::<Vec<_>>()
                .join(",")
        );
        let _ = write!(out, "\"acc\":{},", self.machine.fpu_accumulator());
        let _ = write!(
            out,
            "\"breakpoints\":[{}],",
            self.breakpoints
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );

        match self.current_func() {
            Some(f) => {
                let _ = write!(
                    out,
                    "\"func\":{{\"name\":\"{}\",\"addr\":[{},{}],\"frame\":{},\"file\":{},",
                    esc(&f.name),
                    f.addr.0,
                    f.addr.1,
                    f.frame_size,
                    f.file
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

        let globals: Vec<String> = self
            .debug
            .globals
            .iter()
            .map(|v| self.var_json(v))
            .collect();
        let _ = write!(out, "\"globals\":[{}],", globals.join(","));
        let consts: Vec<String> = self
            .debug
            .consts
            .iter()
            .map(|(n, ty, v)| {
                format!(
                    "{{\"name\":\"{}\",\"ty\":{},\"value\":{}}}",
                    esc(n),
                    ty_to_json(ty),
                    v
                )
            })
            .collect();
        let _ = write!(out, "\"consts\":[{}],", consts.join(","));

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

        // shadow call stack (innermost last), library frames omitted
        let stack: Vec<String> = self
            .call_stack
            .iter()
            .filter(|f| !f.library)
            .map(|f| {
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
                    "{{\"func\":\"{}\",\"ret\":{},\"site\":{},\"site_file\":{},\"site_line\":{}}}",
                    esc(&f.func_name),
                    f.return_addr,
                    site_addr,
                    sf,
                    sl
                )
            })
            .collect();
        let _ = write!(out, "\"stack\":[{}],", stack.join(","));

        // source view: the selected file (default the current line's file)
        let src_file = view_file
            .or_else(|| self.current_line().map(|(f, _)| f))
            .unwrap_or(0);
        let files: Vec<String> = self
            .debug
            .files
            .iter()
            .enumerate()
            .map(|(i, name)| format!("[{i},\"{}\"]", esc(name)))
            .collect();
        let lines = self.source_lines(src_file);
        let bps = self.breakpoint_lines(src_file);
        let _ = write!(
            out,
            "\"src\":{{\"file\":{},\"files\":[{}],\"lines\":[",
            src_file,
            files.join(",")
        );
        let items: Vec<String> = lines
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let n = i + 1;
                let cur = self.current_line() == Some((src_file, n as u32));
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
        let map: Vec<String> = self
            .debug
            .lines
            .iter()
            .filter(|(_, f, _)| *f == src_file)
            .map(|&(a, _, l)| format!("[{a},{l}]"))
            .collect();
        let _ = write!(out, "],\"map\":[{}]}},", map.join(","));

        let _ = write!(out, "\"disasm\":[");
        let items: Vec<String> = self
            .disasm
            .iter()
            .map(|d| {
                let target = match (d.target, &d.target_name) {
                    (Some(t), Some(n)) => format!(",\"target\":{t},\"tname\":\"{}\"", esc(n)),
                    (Some(t), None) => format!(",\"target\":{t}"),
                    _ => String::new(),
                };
                let header = if d.header { ",\"header\":true" } else { "" };
                let call = if d.call { ",\"call\":true" } else { "" };
                let lib = if d.library { ",\"lib\":true" } else { "" };
                let wide = if d.wide { ",\"wide\":true" } else { "" };
                format!(
                    "{{\"addr\":{},\"text\":\"{}\"{}{}{}{}{}}}",
                    d.addr,
                    esc(&d.text),
                    target,
                    call,
                    header,
                    lib,
                    wide
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
                words
                    .iter()
                    .map(|w| w.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
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

    fn breakpoint_lines(&self, file: u16) -> Vec<u32> {
        self.debug
            .lines
            .iter()
            .filter(|(a, f, _)| *f == file && self.breakpoints.contains(a))
            .map(|&(_, _, l)| l)
            .collect()
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
        let Some(path) = self.resolve_source(file) else {
            return String::new();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return String::new();
        };
        text.lines()
            .nth(line as usize - 1)
            .unwrap_or("")
            .trim()
            .to_string()
    }

    fn source_lines(&self, file: u16) -> Vec<String> {
        if let Some(text) = self.source_overrides.get(&file) {
            return text.lines().map(|line| line.to_string()).collect();
        }
        let Some(path) = self.resolve_source(file) else {
            return vec![];
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return vec![];
        };
        text.lines().map(|l| l.to_string()).collect()
    }

    /// resolve a recorded source file to a readable path. Module sources are
    /// recorded as bare `name.rs`, so they are resolved like the rcc CLI,
    /// rooted at the main source file's directory.
    fn resolve_source(&self, file: u16) -> Option<PathBuf> {
        let name = self.debug.files.get(file as usize)?;
        let direct = PathBuf::from(name);
        if direct.is_file() {
            return Some(direct);
        }
        let module = name.strip_suffix(".rs").unwrap_or(name.as_str());
        [
            self.source_root.join(format!("{module}.rs")),
            self.source_root.join(format!("{module}.dsl.rs")),
            self.source_root.join(module).join("mod.rs"),
        ]
        .into_iter()
        .find(|candidate| candidate.is_file())
    }

    pub fn mem_json(&self, addr: u16, len: u16) -> String {
        let words: Vec<String> = (0..len)
            .map(|i| self.data_word(addr.wrapping_add(i)).to_string())
            .collect();
        format!("{{\"addr\":{addr},\"words\":[{}]}}", words.join(","))
    }
}
