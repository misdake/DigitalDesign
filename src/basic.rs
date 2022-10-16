use std::fmt::{Debug, Formatter};

pub type WireValue = u8;
pub type LatencyValue = u16;

#[derive(Copy, Clone)]
pub struct Wire(pub usize);

static mut WIRES: Vec<WireValue> = Vec::new();
static mut LATENCIES: Vec<LatencyValue> = Vec::new();
static mut GATES: Vec<Gate> = Vec::new();

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

pub struct Gate {
    pub wire_a: Wire,
    pub wire_b: Wire,
    pub wire_out: Wire,
}

impl Gate {
    fn execute(&self) {
        assert_eq!(
            self.wire_out.get_latency(),
            0,
            "wire should not be written twice!"
        );

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

pub fn execute_all_gates() -> ExecutionResult {
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
