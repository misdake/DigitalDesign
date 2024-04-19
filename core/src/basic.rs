use once_cell::sync::Lazy;
use std::any::Any;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::ops::{DerefMut, Range};
use std::sync::Mutex;

pub type WireValue = u8;
pub type LatencyValue = u16;

#[derive(Copy, Clone)]
pub struct Wire(pub usize);

#[derive(Copy, Clone)]
pub struct Reg(pub usize);

#[derive(Clone)]
enum ExecuteSegment {
    Gates(Range<usize>),
    Externals(Range<usize>),
}

//TODO how?
// impl Debug for Wire {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         f.write_str(&format!("{}", self.get()))
//     }
// }
// impl Debug for Reg {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         f.write_str(&format!("{}", self.out().get()))
//     }
// }
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

pub struct Circuit {
    pub wires: Vec<WireValue>,
    pub latencies: Vec<LatencyValue>,
    gates_map: HashMap<(usize, usize), Wire>,
    gates: Vec<Gate>,
    externals: Vec<Box<dyn External>>,
    pub regs: Vec<RegValue>,
    execute_segments: Vec<ExecuteSegment>,
}
impl Circuit {
    fn new() -> Self {
        Self {
            wires: vec![0, 1],
            latencies: vec![0, 0],
            gates_map: HashMap::new(),
            gates: Vec::new(),
            externals: Vec::new(),
            regs: Vec::new(),
            execute_segments: Vec::new(),
        }
    }

    fn clear(&mut self) {
        *self = Self::new();
    }
}

static mut CIRCUIT_LOCK: Mutex<()> = Mutex::new(());
static mut CIRCUIT: Option<Circuit> = None;
pub fn build_circuit<R>(f: impl FnOnce() -> R) -> (Circuit, R) {
    unsafe {
        let _lock = CIRCUIT_LOCK.lock().unwrap();
        CIRCUIT = Some(Circuit::new());
        let r = f();
        let new_circuit = CIRCUIT.take().unwrap();
        (new_circuit, r)
    }
}
fn circuit_mut() -> &'static mut Circuit {
    unsafe { CIRCUIT.as_mut().unwrap() }
}

const WIRE_0: usize = 0;
const WIRE_1: usize = 1;

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

impl Circuit {
    fn external<E: External>(&mut self, e: E) -> &E {
        self.before_new_external();
        self.externals.push(Box::new(e));
        let r = self.externals.last().unwrap().as_ref();
        r.as_any().downcast_ref::<E>().unwrap()
    }
    fn reg(&mut self) -> Reg {
        let reg = RegValue {
            wire_in: None,
            wire_out: input(),
            temp_value: 0,
        };
        let index = self.regs.len();
        self.regs.push(reg);
        Reg(index)
    }
    fn input(&mut self) -> Wire {
        let index = self.wires.len();
        self.wires.push(0);
        self.latencies.push(0);
        Wire(index)
    }
    fn find_gate(&self, a: Wire, b: Wire) -> Option<Wire> {
        let v1 = self.gates_map.get(&(a.0, b.0));
        if v1.is_some() {
            return v1.copied();
        }
        let v2 = self.gates_map.get(&(b.0, a.0));
        if v2.is_some() {
            return v2.copied();
        }
        None
    }
    fn nand(&mut self, a: Wire, b: Wire) -> Wire {
        // deduplicate
        let duplicated = self.find_gate(a, b);
        if let Some(out) = duplicated {
            return out;
        }

        self.before_new_gate();
        let out = input();
        self.gates_map.insert((a.0, b.0), out);
        out.set_latency(self, a.get_latency(self).max(b.get_latency(self)) + 1);
        self.gates.push(Gate {
            wire_a: a,
            wire_b: b,
            wire_out: out,
        });
        out
    }

    fn before_new_gate(&mut self) {
        if let Some(ExecuteSegment::Gates(range)) = self.execute_segments.last_mut() {
            range.end += 1;
        } else {
            let next = self.gates.len();
            self.execute_segments
                .push(ExecuteSegment::Gates(next..(next + 1)));
        }
    }
    fn before_new_external(&mut self) {
        if let Some(ExecuteSegment::Externals(range)) = self.execute_segments.last_mut() {
            range.end += 1;
        } else {
            let next = self.externals.len();
            self.execute_segments
                .push(ExecuteSegment::Externals(next..(next + 1)));
        }
    }
    pub fn get_statistics(&mut self) -> ExecutionResult {
        ExecutionResult {
            wire_count: self.wires.len(),
            gate_count: self.gates.len(),
            max_latency: *self.latencies.iter().max().unwrap_or(&0),
        }
    }

    pub fn execute_gates(&mut self) {
        // println!("execute segments {:?}", EXECUTE_SEGMENTS);
        for segment in &self.execute_segments {
            match &segment {
                ExecuteSegment::Gates(range) => {
                    let gates = &self.gates[range.start..range.end];
                    gates.iter().for_each(|gate| gate.execute(self));
                }
                ExecuteSegment::Externals(range) => {
                    let externals = &mut self.externals[range.start..range.end];
                    externals.iter_mut().for_each(|external| external.execute());
                }
            }
        }
    }

    pub fn clock_tick(&mut self) {
        self.regs.iter_mut().for_each(|reg| {
            reg.temp_value = reg
                .wire_in
                .map(|w| w.get(self))
                .expect("reg with no input!")
        });
        self.regs
            .iter_mut()
            .for_each(|reg| reg.wire_out.set(self, reg.temp_value));
    }
    pub fn simulate(&mut self) {
        self.execute_gates();
        self.clock_tick();
    }

    pub fn export_gate_reg(&self) -> ExportGateReg {
        let wire_0_value = self.wires[WIRE_0];
        let wire_1_value = self.wires[WIRE_1];

        let gates = self
            .gates
            .iter()
            .map(|gate| GateExport {
                wire_a_index: gate.wire_a.0,
                wire_b_index: gate.wire_b.0,
                wire_out_index: gate.wire_out.0,
            })
            .collect::<Vec<_>>();

        let regs = self
            .regs
            .iter()
            .map(|reg| RegExport {
                wire_in_index: reg.wire_in.unwrap().0,
                wire_out_index: reg.wire_out.0,
            })
            .collect::<Vec<_>>();

        assert!(
            self.externals.is_empty(),
            "Export wire/reg only! Externals are not supported!"
        );

        ExportGateReg {
            wire_0_value,
            wire_1_value,
            wire_count: self.wires.len(),
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
    circuit_mut().external(e)
}

pub fn reg() -> Reg {
    circuit_mut().reg()
}
impl Reg {
    pub fn set_in(self, wire: Wire) {
        let circuit = circuit_mut();
        let reg = &mut circuit.regs[self.0];
        assert!(reg.wire_in.is_none());
        reg.wire_in = Some(wire);
    }
    pub fn out(self) -> Wire {
        circuit_mut().regs[self.0].wire_out
    }
}

pub fn input() -> Wire {
    circuit_mut().input()
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
    circuit_mut().nand(a, b)
}

//TODO call from Circuit?
impl Wire {
    pub fn is_one(self, circuit: &Circuit) -> bool {
        circuit.wires[self.0] > 0
    }
    pub fn get(self, circuit: &Circuit) -> WireValue {
        circuit.wires[self.0]
    }
    pub fn set(self, circuit: &mut Circuit, value: WireValue) {
        circuit.wires[self.0] = value;
    }
    pub fn get_latency(self, circuit: &Circuit) -> LatencyValue {
        circuit.latencies[self.0]
    }
    pub fn set_latency(self, circuit: &mut Circuit, value: LatencyValue) {
        circuit.latencies[self.0] = value;
    }
}

pub struct RegValue {
    wire_in: Option<Wire>,
    pub wire_out: Wire,
    temp_value: WireValue,
}

#[derive(Copy, Clone)]
pub struct Gate {
    pub wire_a: Wire,
    pub wire_b: Wire,
    pub wire_out: Wire,
}

impl Gate {
    fn execute(&self, circuit: &mut Circuit) {
        let a = self.wire_a.get(circuit);
        let b = self.wire_b.get(circuit);
        self.wire_out.set(circuit, !(a & b) & 1);
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub wire_count: usize,
    pub gate_count: usize,
    pub max_latency: LatencyValue,
}
