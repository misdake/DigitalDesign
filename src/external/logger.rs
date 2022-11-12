use crate::{External, Wire, WireValue, Wires, WiresU8};
use std::any::Any;

pub struct Logger {
    name: String,
    wire: Wire,
    values: Vec<WireValue>,
}
impl External for Logger {
    fn execute(&mut self) {
        self.values.push(self.wire.get());
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
    fn execute(&mut self) {
        self.values.push(self.wires.get_u8());
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
