use std::any::Any;

use crate::{CircuitWires, External, Wire, WireValue, Wires, WiresU8};

pub struct Logger {
    name: String,
    wire: Wire,
    values: Vec<WireValue>,
}
impl External for Logger {
    fn execute(&mut self, circuit: &mut CircuitWires) {
        self.values.push(circuit.get_wire(self.wire));
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl Logger {
    pub fn new(name: String, wire: Wire) -> Logger {
        Self {
            name,
            wire,
            values: Vec::new(),
        }
    }
    pub fn print(&self) {
        print!("{}:", self.name);
        for v in &self.values {
            print!("{}", *v);
        }
        println!();
    }
    pub fn get_values(&self) -> &Vec<WireValue> {
        &self.values
    }
}

pub struct LoggerU8<const W: usize>
where
    Wires<W>: WiresU8,
{
    name: String,
    wires: Wires<W>,
    values: Vec<u8>,
}
impl<const W: usize> External for LoggerU8<W>
where
    Wires<W>: WiresU8,
{
    fn execute(&mut self, circuit: &mut CircuitWires) {
        self.values.push(self.wires.get_u8(circuit));
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl<const W: usize> LoggerU8<W>
where
    Wires<W>: WiresU8,
{
    pub fn new(name: String, wires: Wires<W>) -> LoggerU8<W> {
        Self {
            name,
            wires,
            values: Vec::new(),
        }
    }
    pub fn print(&self) {
        print!("{}:", self.name);
        for v in &self.values {
            print!(" {}", *v);
        }
        println!();
    }
    pub fn get_values(&self) -> &Vec<u8> {
        &self.values
    }
}

#[test]
fn test_logger() {
    use crate::*;
    let (mut circuit, (a, b, logger)) = build_circuit(|| {
        let a = input();
        let b = input();
        let c = a | b;
        let logger = external(Logger::new("or".to_string(), c));
        (a, b, logger)
    });

    for i in 0..10 {
        circuit.set_wire(a, if i % 3 == 0 { 1 } else { 0 });
        circuit.set_wire(b, if i % 2 == 0 { 1 } else { 0 });
        circuit.simulate();
    }
    assert_eq!(logger.get_values(), &vec![1, 0, 1, 1, 1, 0, 1, 0, 1, 1]);
}
#[test]
fn test_logger_u8() {
    use crate::*;

    let (mut circuit, logger) = build_circuit(|| {
        let one = Wires::<4>::parse_u8(1);
        let curr = reg_w::<4>();
        curr.set_in(add_naive(curr.out, one).sum);
        external(LoggerU8::new("inc".to_string(), curr.out))
    });
    for _ in 0..=16 {
        circuit.simulate();
    }
    assert_eq!(
        logger.get_values(),
        &vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0]
    );
}
