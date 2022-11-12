use crate::{External, Wire, WireValue};
use std::any::Any;

pub struct Printer {
    name: String,
    wire: Wire,
}
impl Printer {
    pub fn new(name: String, wire: Wire) -> Self {
        Self { name, wire }
    }
}

impl External for Printer {
    fn execute(&mut self) {
        println!("{}: {}", self.name, self.wire.get());
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

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
