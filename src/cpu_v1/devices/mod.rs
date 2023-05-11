mod device_0_print;
mod device_1_math;
mod device_2_gamepad;
mod device_3_graphics_v1;

// Device

#[repr(u8)]
pub enum DeviceType {
    Print = 0,
    Math = 1,
    Gamepad = 2,
    GraphicsV1 = 3,
}
pub trait Device: 'static {
    fn device_type(&self) -> DeviceType;
    fn exec(&mut self, opcode4: u8, reg0: u8, reg1: u8) -> DeviceReadResult;
}
#[derive(Default)]
pub struct DeviceReadResult {
    pub reg0_write_data: u8,
    pub self_latency: u16,
}

// Devices

use crate::cpu_v1::devices::device_0_print::DevicePrint;
use crate::cpu_v1::devices::device_1_math::DeviceMath;
use once_cell::sync::Lazy;
use std::sync::{Arc, Mutex};

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
        self.register(DevicePrint::default());
        self.register(DeviceMath::default());
    }
    fn register(&mut self, device: impl Device) {
        let device_type = device.device_type();
        self.devices[device_type as u8 as usize] = Some(Box::new(device));
    }
    pub fn execute(
        &mut self,
        bus_addr: u8,
        bus_opcode4: u8,
        reg0: u8,
        reg1: u8,
    ) -> DeviceReadResult {
        self.devices[bus_addr as usize]
            .as_mut()
            .map_or(DeviceReadResult::default(), |d| {
                d.exec(bus_opcode4, reg0, reg1)
            })
    }
}

#[cfg(test)]
use crate::cpu_v1::InstBinary;

#[cfg(test)]
fn test_device(inst: &[InstBinary], max_cycle: u32, regs_ref: [u8; 4]) {
    use crate::cpu_v1::cpu_v1_build;
    use crate::*;

    let mut inst_rom = [0u8; 256];
    inst.iter()
        .enumerate()
        .for_each(|(i, inst)| inst_rom[i] = inst.binary);

    let (state, _internal) = cpu_v1_build(inst_rom);

    for _ in 0..max_cycle {
        let pc = state.pc.out.get_u8();
        if pc as usize >= inst.len() {
            break;
        }
        let inst = inst[pc as usize];
        println!(
            "pc {:08b}: inst {} {:08b}",
            pc,
            inst.desc.name(),
            inst.binary
        );
        execute_gates();
        clock_tick();
    }

    for i in 0..4 {
        assert_eq!(state.reg[i].out.get_u8(), regs_ref[i]);
    }
}
