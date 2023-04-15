use std::sync::{Arc, Mutex};

#[repr(u8)]
enum DeviceType {
    Print = 0,
    // Math = 1, TODO
    // Graphics = 2,
}
trait Device: 'static {
    fn reset(&mut self) {}
    fn device_type(&self) -> DeviceType;
    fn execute(&mut self, opcode: u8, reg0: u8, reg1: u8);
    fn read(&mut self, reg0: u8, reg1: u8) -> (u8, u16); // value, latency
}
pub struct Devices {
    devices: [Option<Box<dyn Device>>; 16],
}
unsafe impl Send for Devices {}

impl Devices {
    pub fn visit(f: impl FnOnce(&mut Devices)) {
        let mut result = DEVICES.lock().unwrap();
        f(&mut *result)
    }

    fn create_empty() -> Self {
        let mut devices = Self {
            devices: [0; 16].map(|_| None),
        };
        devices.register_default();
        devices
    }
    fn register_default(&mut self) {
        self.register(DevicePrint {});
    }
    fn register(&mut self, device: impl Device) {
        let device_type = device.device_type();
        self.devices[device_type as u8 as usize] = Some(Box::new(device));
    }
    pub fn reset(&mut self) {
        self.devices.iter_mut().for_each(|device| {
            device.as_mut().map(|d| d.reset());
        })
    }
    pub fn execute(&mut self, bus_addr: u8, bus_opcode: u8, reg0: u8, reg1: u8) {
        self.devices[bus_addr as usize].as_mut().map(|d| {
            d.execute(bus_opcode, reg0, reg1);
        });
    }
    pub fn read(&mut self, bus_addr: u8, reg0: u8, reg1: u8) -> (u8, u16) {
        self.devices[bus_addr as usize]
            .as_mut()
            .map_or((0, 1), |d| d.read(reg0, reg1))
    }
}

use once_cell::sync::Lazy;
static DEVICES: Lazy<Arc<Mutex<Devices>>> =
    Lazy::new(|| Arc::new(Mutex::new(Devices::create_empty())));

struct DevicePrint {}
impl Device for DevicePrint {
    fn device_type(&self) -> DeviceType {
        DeviceType::Print
    }

    fn execute(&mut self, _opcode: u8, reg0: u8, _reg1: u8) {
        println!("DevicePrint exec {reg0}")
    }

    fn read(&mut self, reg0: u8, _reg1: u8) -> (u8, u16) {
        println!("DevicePrint read {reg0}");
        (reg0, 1)
    }
}
