// pub enum Assert<const CHECK: bool> {}
//
// pub trait IsTrue {}
//
// impl IsTrue for Assert<true> {}
//
// pub struct Wire<const W: usize> where Assert<{ W - 1 > 0 }>: IsTrue {
//     values: [bool; W - 1],
// }
//
// pub struct Wire<const W: usize> {
//     values: [bool; W],
// }

pub type WireValue = u8;
pub type LatencyValue = u16;

#[derive(Copy, Clone)]
pub struct WireRef(usize);

static mut LATENCIES: Vec<LatencyValue> = Vec::new();
static mut WIRES: Vec<WireValue> = Vec::new();
static mut GATES: Vec<Gate> = Vec::new();

pub fn input() -> WireRef {
    unsafe {
        let index = WIRES.len();
        WIRES.push(0);
        LATENCIES.push(0);
        WireRef(index)
    }
}

pub fn nand(a: WireRef, b: WireRef) -> WireRef {
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

impl WireRef {
    pub fn get(self) -> WireValue {
        unsafe {
            WIRES[self.0]
        }
    }
    pub fn set(self, value: WireValue) {
        unsafe {
            WIRES[self.0] = value;
        }
    }
    pub fn get_latency(self) -> LatencyValue {
        unsafe {
            LATENCIES[self.0]
        }
    }
    pub fn set_latency(self, value: LatencyValue) {
        unsafe {
            LATENCIES[self.0] = value;
        }
    }
}

pub struct Gate {
    pub wire_a: WireRef,
    pub wire_b: WireRef,
    pub wire_out: WireRef,
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
