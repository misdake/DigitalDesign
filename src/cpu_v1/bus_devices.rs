use std::sync::{Arc, Mutex};

// Device

#[repr(u8)]
enum DeviceType {
    Print = 0,
    Math = 1,
    // Graphics = 2,
}
trait Device: 'static {
    fn reset(&mut self);
    fn device_type(&self) -> DeviceType;
    fn exec(&mut self, opcode4: u8, reg0: u8, reg1: u8);
    fn read(&mut self, reg0: u8, reg1: u8) -> DeviceReadResult;
}
#[derive(Default)]
pub struct DeviceReadResult {
    pub out_data: u8,
    pub self_latency: u16,
}

// Devices

use once_cell::sync::Lazy;
static DEVICES: Lazy<Arc<Mutex<Devices>>> =
    Lazy::new(|| Arc::new(Mutex::new(Devices::create_empty())));
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
    pub fn execute(&mut self, bus_addr: u8, bus_opcode4: u8, reg0: u8, reg1: u8) {
        self.devices[bus_addr as usize].as_mut().map(|d| {
            d.exec(bus_opcode4, reg0, reg1);
        });
    }
    pub fn read(&mut self, bus_addr: u8, reg0: u8, reg1: u8) -> DeviceReadResult {
        self.devices[bus_addr as usize]
            .as_mut()
            .map_or(DeviceReadResult::default(), |d| d.read(reg0, reg1))
    }
}

struct DevicePrint {}
impl Device for DevicePrint {
    fn reset(&mut self) {}
    fn device_type(&self) -> DeviceType {
        DeviceType::Print
    }
    fn exec(&mut self, _opcode: u8, reg0: u8, _reg1: u8) {
        println!("DevicePrint exec {reg0}")
    }
    fn read(&mut self, reg0: u8, _reg1: u8) -> DeviceReadResult {
        println!("DevicePrint read {reg0}");
        DeviceReadResult {
            out_data: reg0,
            self_latency: 0,
        }
    }
}

struct DeviceMath {
    value: u8, // 4 bits
}
#[repr(u8)]
pub enum DeviceMathOpcode {
    Pop = 0,         // will write to reg0
    ExtractBits = 1, //
}
impl Device for DeviceMath {
    fn reset(&mut self) {
        self.value = 0;
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Math
    }
    fn exec(&mut self, opcode4: u8, _reg0: u8, _reg1: u8) {
        todo!() //TODO
    }
    fn read(&mut self, reg0: u8, _reg1: u8) -> DeviceReadResult {
        todo!() //TODO
    }
}
