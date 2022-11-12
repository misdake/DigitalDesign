use std::any::Any;
use std::fmt::{Debug, Formatter};

pub type WireValue = u8;
pub type LatencyValue = u16;

#[derive(Copy, Clone)]
pub struct Wire(pub usize);

#[derive(Copy, Clone)]
pub struct Reg(pub usize);

static mut WIRES: Vec<WireValue> = Vec::new();
static mut LATENCIES: Vec<LatencyValue> = Vec::new();
static mut GATES: Vec<Gate> = Vec::new();
static mut EXTERNALS: Vec<Box<dyn External>> = Vec::new();
static mut REGS: Vec<RegValue> = Vec::new();

pub fn clear_all() {
    unsafe {
        WIRES.clear();
        LATENCIES.clear();
        GATES.clear();
        EXTERNALS.clear();
        REGS.clear();
    }
}

pub trait External: Any {
    fn execute(&mut self);
    fn as_any(&self) -> &dyn Any;
}

pub fn external<E: External>(e: E) -> &'static E {
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
    unsafe {
        let index = WIRES.len();
        WIRES.push((value > 0).into());
        LATENCIES.push(0);
        Wire(index)
    }
}

pub fn nand(a: Wire, b: Wire) -> Wire {
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

impl Debug for Wire {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wire").field("index", &self.0).finish()
    }
}

impl Wire {
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

#[derive(Debug, Clone)]
pub struct ExecutionResult {
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
        let mut max_latency: LatencyValue = 0;
        LATENCIES.fill(0);

        for gate in &GATES {
            gate.execute();
            max_latency = max_latency.max(gate.wire_out.get_latency());
        }

        ExecutionResult {
            gate_count: GATES.len(),
            max_latency,
        }
    }
}

pub fn clock_tick() {
    unsafe {
        EXTERNALS.iter_mut().for_each(|external| external.execute());
        REGS.iter_mut()
            .for_each(|reg| reg.temp_value = reg.wire_in.unwrap().get());
        REGS.iter_mut()
            .for_each(|reg| reg.wire_out.set(reg.temp_value));
    }
}
