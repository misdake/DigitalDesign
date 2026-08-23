//! CPU V1 device-bus boundary.
//!
//! Concrete terminal, graphics, gamepad, and ROM devices belong to a system.

use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DeviceReadResult {
    pub reg0_write_data: u8,
    pub self_latency: u16,
}

pub trait DeviceBus {
    fn execute(&mut self, bus_addr: u8, bus_opcode3: u8, reg0: u8, reg1: u8) -> DeviceReadResult;
}

pub type SharedDeviceBus = Rc<RefCell<Box<dyn DeviceBus>>>;

#[derive(Default)]
pub struct NullDeviceBus;

impl DeviceBus for NullDeviceBus {
    fn execute(
        &mut self,
        _bus_addr: u8,
        _bus_opcode3: u8,
        _reg0: u8,
        _reg1: u8,
    ) -> DeviceReadResult {
        DeviceReadResult::default()
    }
}

pub fn shared_device_bus(bus: impl DeviceBus + 'static) -> SharedDeviceBus {
    Rc::new(RefCell::new(Box::new(bus)))
}

pub fn null_device_bus() -> SharedDeviceBus {
    shared_device_bus(NullDeviceBus)
}
