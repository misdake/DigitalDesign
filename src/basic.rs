use std::any::Any;
use std::fmt::{Debug, Formatter};
use std::ops::Range;

pub type WireValue = u8;
pub type LatencyValue = u16;

#[derive(Copy, Clone)]
pub struct Wire(pub usize);

#[derive(Copy, Clone)]
pub struct Reg(pub usize);

#[derive(Debug)]
enum ExecuteSegment {
    Gate(Range<usize>),
    External(Range<usize>),
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

static mut WIRES: Vec<WireValue> = Vec::new();
static mut LATENCIES: Vec<LatencyValue> = Vec::new();
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
        GATES.clear();
        EXTERNALS.clear();
        REGS.clear();
        WIRES.push(0); // => WIRE_0
        WIRES.push(1); // => WIRE_1
        LATENCIES.push(0); // => WIRE_0
        LATENCIES.push(0); // => WIRE_1
        EXECUTE_SEGMENTS.clear();
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

pub fn nand(a: Wire, b: Wire) -> Wire {
    before_new_gate();
    unsafe {
        let out = input();
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

pub struct Gate {
    pub wire_a: Wire,
    pub wire_b: Wire,
    pub wire_out: Wire,
}

impl Gate {
    fn execute(&self) {
        let a = self.wire_a.get();
        let b = self.wire_b.get();
        let la = self.wire_a.get_latency();
        let lb = self.wire_b.get_latency();
        self.wire_out.set(!(a & b) & 1);
        self.wire_out.set_latency(la.max(lb) + 1);
    }
}

//region Execute Segments

fn before_new_gate() {
    unsafe {
        if let Some(ExecuteSegment::Gate(range)) = EXECUTE_SEGMENTS.last_mut() {
            range.end += 1;
        } else {
            let next = GATES.len();
            EXECUTE_SEGMENTS.push(ExecuteSegment::Gate(next..(next + 1)));
        }
    }
}
fn before_new_external() {
    unsafe {
        if let Some(ExecuteSegment::External(range)) = EXECUTE_SEGMENTS.last_mut() {
            range.end += 1;
        } else {
            let next = EXTERNALS.len();
            EXECUTE_SEGMENTS.push(ExecuteSegment::External(next..(next + 1)));
        }
    }
}

impl ExecuteSegment {
    fn execute(&self) {
        match self {
            ExecuteSegment::Gate(range) => unsafe {
                for gate in &GATES[range.start..range.end] {
                    gate.execute();
                }
            },
            ExecuteSegment::External(range) => unsafe {
                for external in &mut EXTERNALS[range.start..range.end] {
                    external.execute();
                }
            },
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

pub fn execute_gates() -> ExecutionResult {
    unsafe {
        LATENCIES.fill(0);

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
