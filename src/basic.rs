use once_cell::sync::Lazy;
use std::any::Any;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::ops::Range;

pub type WireValue = u8;
pub type LatencyValue = u16;

#[derive(Copy, Clone)]
pub struct Wire(pub usize);

#[derive(Copy, Clone)]
pub struct Reg(pub usize);

enum ExecuteSegment {
    Gates(Range<usize>),
    Externals(Range<usize>),
}

impl Debug for Wire {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("{}", self.get()))
    }
}
impl Debug for Reg {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("{}", self.out().get()))
    }
}
impl Debug for ExecuteSegment {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut r = f.debug_struct("ExecuteSegment");
        match self {
            ExecuteSegment::Gates(gates) => r.field("Gates", gates),
            ExecuteSegment::Externals(externals) => r.field("Externals", externals),
        };
        r.finish()
    }
}

static mut WIRES: Vec<WireValue> = Vec::new();
static mut LATENCIES: Vec<LatencyValue> = Vec::new();
static mut GATES_MAP: Lazy<HashMap<(usize, usize), Wire>> = Lazy::new(|| HashMap::new()); // (a, b) -> out
static mut GATES: Vec<Gate> = Vec::new();
static mut EXTERNALS: Vec<Box<dyn External>> = Vec::new();
static mut REGS: Vec<RegValue> = Vec::new();
static mut EXECUTE_SEGMENTS: Vec<ExecuteSegment> = Vec::new();

const WIRE_0: usize = 0;
const WIRE_1: usize = 1;

pub fn clear_all() {
    unsafe {
        WIRES.clear();
        LATENCIES.clear();
        GATES_MAP.clear();
        GATES.clear();
        EXTERNALS.clear();
        REGS.clear();
        EXECUTE_SEGMENTS.clear();

        WIRES.push(0); // => WIRE_0
        WIRES.push(1); // => WIRE_1
        LATENCIES.push(0); // => WIRE_0
        LATENCIES.push(0); // => WIRE_1
    }
}

#[derive(Debug, Copy, Clone)]
pub struct GateExport {
    pub wire_a_index: usize,
    pub wire_b_index: usize,
    pub wire_out_index: usize,
}
#[derive(Debug, Copy, Clone)]
pub struct RegExport {
    pub wire_in_index: usize,
    pub wire_out_index: usize,
}
pub struct ExportGateReg {
    pub wire_0_value: u8,
    pub wire_1_value: u8,
    pub wire_count: usize,
    pub gates: Vec<GateExport>,
    pub regs: Vec<RegExport>,
}
pub fn export_gate_reg() -> ExportGateReg {
    unsafe {
        let wire_0_value = WIRES[WIRE_0];
        let wire_1_value = WIRES[WIRE_1];

        let gates = GATES
            .iter()
            .map(|gate| GateExport {
                wire_a_index: gate.wire_a.0,
                wire_b_index: gate.wire_b.0,
                wire_out_index: gate.wire_out.0,
            })
            .collect::<Vec<_>>();

        let regs = REGS
            .iter()
            .map(|reg| RegExport {
                wire_in_index: reg.wire_in.unwrap().0,
                wire_out_index: reg.wire_out.0,
            })
            .collect::<Vec<_>>();

        ExportGateReg {
            wire_0_value,
            wire_1_value,
            wire_count: WIRES.len(),
            gates,
            regs,
        }
    }
}

pub trait External: Any {
    fn execute(&mut self);
    fn as_any(&self) -> &dyn Any;
}

pub fn external<E: External>(e: E) -> &'static E {
    before_new_external();
    unsafe {
        EXTERNALS.push(Box::new(e));
        let r = EXTERNALS.last().unwrap().as_ref();
        r.as_any().downcast_ref::<E>().unwrap()
    }
}

pub fn reg() -> Reg {
    let reg = RegValue {
        wire_in: None,
        wire_out: input(),
        temp_value: 0,
    };
    unsafe {
        let index = REGS.len();
        REGS.push(reg);
        Reg(index)
    }
}
impl Reg {
    pub fn set_in(self, wire: Wire) {
        unsafe {
            let mut reg = &mut REGS[self.0];
            assert!(reg.wire_in.is_none());
            reg.wire_in = Some(wire);
        }
    }
    pub fn out(self) -> Wire {
        unsafe { REGS[self.0].wire_out }
    }
}

pub fn input() -> Wire {
    unsafe {
        let index = WIRES.len();
        WIRES.push(0);
        LATENCIES.push(0);
        Wire(index)
    }
}

pub fn input_const(value: WireValue) -> Wire {
    let index = match value {
        0 => WIRE_0,
        1 => WIRE_1,
        _ => {
            unreachable!()
        }
    };
    Wire(index)
}

fn find_gate(a: Wire, b: Wire) -> Option<Wire> {
    unsafe {
        let v1 = GATES_MAP.get(&(a.0, b.0));
        if v1.is_some() {
            return v1.copied();
        }
        let v2 = GATES_MAP.get(&(b.0, a.0));
        if v2.is_some() {
            return v2.copied();
        }
    }
    None
}

pub fn nand(a: Wire, b: Wire) -> Wire {
    // deduplicate
    let duplicated = find_gate(a, b);
    if let Some(out) = duplicated {
        return out;
    }

    before_new_gate();
    unsafe {
        let out = input();
        GATES_MAP.insert((a.0, b.0), out);
        out.set_latency(a.get_latency().max(b.get_latency()) + 1);
        GATES.push(Gate {
            wire_a: a,
            wire_b: b,
            wire_out: out,
        });
        out
    }
}

impl Wire {
    pub fn is_one(self) -> bool {
        unsafe { WIRES[self.0] > 0 }
    }
    pub fn get(self) -> WireValue {
        unsafe { WIRES[self.0] }
    }
    pub fn set(self, value: WireValue) {
        unsafe {
            WIRES[self.0] = value;
        }
    }
    pub fn get_latency(self) -> LatencyValue {
        unsafe { LATENCIES[self.0] }
    }
    pub fn set_latency(self, value: LatencyValue) {
        unsafe {
            LATENCIES[self.0] = value;
        }
    }
}

pub struct RegValue {
    wire_in: Option<Wire>,
    pub wire_out: Wire,
    temp_value: WireValue,
}

#[derive(Debug, Copy, Clone)]
pub struct Gate {
    pub wire_a: Wire,
    pub wire_b: Wire,
    pub wire_out: Wire,
}

impl Gate {
    fn execute(&self) {
        let a = self.wire_a.get();
        let b = self.wire_b.get();
        self.wire_out.set(!(a & b) & 1);
    }
}

//region Execute Segments

fn before_new_gate() {
    unsafe {
        if let Some(ExecuteSegment::Gates(range)) = EXECUTE_SEGMENTS.last_mut() {
            range.end += 1;
        } else {
            let next = GATES.len();
            EXECUTE_SEGMENTS.push(ExecuteSegment::Gates(next..(next + 1)));
        }
    }
}
fn before_new_external() {
    unsafe {
        if let Some(ExecuteSegment::Externals(range)) = EXECUTE_SEGMENTS.last_mut() {
            range.end += 1;
        } else {
            let next = EXTERNALS.len();
            EXECUTE_SEGMENTS.push(ExecuteSegment::Externals(next..(next + 1)));
        }
    }
}

impl ExecuteSegment {
    fn execute(&self) {
        match self {
            ExecuteSegment::Gates(range) => {
                let gates = unsafe { &GATES[range.start..range.end] };
                gates.iter().for_each(|gate| gate.execute());
            }
            ExecuteSegment::Externals(range) => {
                let externals = unsafe { &mut EXTERNALS[range.start..range.end] };
                externals.iter_mut().for_each(|external| external.execute());
            }
        }
    }
}

//endregion

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub wire_count: usize,
    pub gate_count: usize,
    pub max_latency: LatencyValue,
}

pub fn simulate() -> ExecutionResult {
    let result = execute_gates();
    clock_tick();
    result
}

pub fn get_statistics() -> ExecutionResult {
    unsafe {
        ExecutionResult {
            wire_count: WIRES.len(),
            gate_count: GATES.len(),
            max_latency: *LATENCIES.iter().max().unwrap_or(&0),
        }
    }
}

pub fn execute_gates() -> ExecutionResult {
    unsafe {
        // println!("execute segments {:?}", EXECUTE_SEGMENTS);
        for segment in &EXECUTE_SEGMENTS {
            segment.execute()
        }

        ExecutionResult {
            wire_count: WIRES.len(),
            gate_count: GATES.len(),
            max_latency: *LATENCIES.iter().max().unwrap_or(&0),
        }
    }
}

pub fn clock_tick() {
    unsafe {
        REGS.iter_mut().for_each(|reg| {
            reg.temp_value = reg.wire_in.map(|w| w.get()).unwrap_or_else(|| {
                // println!("reg without in");
                0
            })
        });
        REGS.iter_mut()
            .for_each(|reg| reg.wire_out.set(reg.temp_value));
    }
}
