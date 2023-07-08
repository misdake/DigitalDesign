mod device_0_print;
mod device_1_math;
mod device_2_and_3_util;
mod device_2_gamepad;
mod device_3_graphics_v1;

// Device

#[repr(u8)]
pub enum DeviceType {
    Print = 1,
    Math,
    Gamepad,
    GraphicsV1,
}
pub trait Device: 'static {
    fn device_type(&self) -> DeviceType;
    fn exec(&mut self, opcode3: u8, reg0: u8, reg1: u8) -> DeviceReadResult;
}
#[derive(Default)]
pub struct DeviceReadResult {
    pub reg0_write_data: u8,
    pub self_latency: u16,
}

// Devices

use crate::cpu_v1::devices::device_0_print::DevicePrint;
use crate::cpu_v1::devices::device_1_math::DeviceMath;
use crate::cpu_v1::devices::device_2_and_3_util::create_device_gamepad_graphics_v1_start;

pub struct Devices {
    generators: [Option<Box<dyn FnOnce(&mut Devices)>>; 16],
    devices: [Option<Box<dyn Device>>; 16],
}
unsafe impl Send for Devices {}

impl Devices {
    pub fn new() -> Self {
        let mut devices = Self {
            generators: [0; 16].map(|_| None),
            devices: [0; 16].map(|_| None),
        };
        devices.register_default();
        devices
    }
    fn register_default(&mut self) {
        const WINDOW_WIDTH: usize = 512;
        const WINDOW_HEIGHT: usize = 512;

        self.register(DeviceType::Print, |d| d.set_device(DevicePrint::default()));
        self.register(DeviceType::Math, |d| d.set_device(DeviceMath::default()));
        self.register(DeviceType::Gamepad, |d| {
            let (gamepad, fb) =
                create_device_gamepad_graphics_v1_start(WINDOW_WIDTH, WINDOW_HEIGHT);
            d.set_device(gamepad);
            d.set_device(fb);
        });
        self.register(DeviceType::GraphicsV1, |d| {
            let (gamepad, fb) =
                create_device_gamepad_graphics_v1_start(WINDOW_WIDTH, WINDOW_HEIGHT);
            d.set_device(gamepad);
            d.set_device(fb);
        });
    }
    fn set_device(&mut self, device: impl Device) {
        let device_type = device.device_type();
        self.devices[device_type as u8 as usize] = Some(Box::new(device));
    }
    pub fn register(
        &mut self,
        device_type: DeviceType,
        device: impl FnOnce(&mut Devices) + 'static,
    ) {
        self.generators[device_type as u8 as usize] = Some(Box::new(device));
    }
    pub fn execute(
        &mut self,
        bus_addr: u8,
        bus_opcode3: u8,
        reg0: u8,
        reg1: u8,
    ) -> DeviceReadResult {
        if self.devices[bus_addr as usize].is_none() {
            if let Some(generator) = self.generators[bus_addr as usize].take() {
                generator(self);
            }
        }
        let device = &mut self.devices[bus_addr as usize];
        device.as_mut().map_or(DeviceReadResult::default(), |d| {
            d.exec(bus_opcode3, reg0, reg1)
        })
    }
}

#[cfg(test)]
use crate::cpu_v1::InstBinary;

#[cfg(test)]
fn test_device(inst: &[InstBinary], max_cycle: u32, regs_ref: [u8; 4]) {
    use crate::cpu_v1::*;
    use crate::*;

    let mut inst_rom = [0u8; 256];
    inst.iter()
        .enumerate()
        .for_each(|(i, inst)| inst_rom[i] = inst.binary);

    // let (state, _internal) = cpu_v1_build(inst_rom);
    let (_state1, state, _internal1, _internal2) = cpu_v1_build_with_ref(inst_rom);

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
