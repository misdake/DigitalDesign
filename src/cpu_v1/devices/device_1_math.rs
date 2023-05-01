use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceType};
use std::mem::transmute;

#[derive(Default)]
pub struct DeviceMath {
    value: Vec<u8>,
}
#[repr(u8)]
pub enum DeviceMathOpcode {
    Pop = 0,         // will write to reg0
    ExtractBits = 1, // push 4 bits of reg0 into stack
}
impl Device for DeviceMath {
    fn reset(&mut self) {
        self.value.clear();
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Math
    }
    fn exec(&mut self, opcode4: u8, reg0: u8, _reg1: u8) {
        let opcode: DeviceMathOpcode = unsafe { transmute(opcode4) };
        match opcode {
            DeviceMathOpcode::Pop => {
                unreachable!()
            }
            DeviceMathOpcode::ExtractBits => {
                self.value = vec![reg0 & 0b1000, reg0 & 0b0100, reg0 & 0b0010, reg0 & 0b0001];
            }
        }
    }
    fn read(&mut self, _reg0: u8, _reg1: u8) -> DeviceReadResult {
        if self.value.is_empty() {
            DeviceReadResult {
                out_data: 0,
                self_latency: 3,
            }
        } else {
            let v = self.value.pop().unwrap();
            DeviceReadResult {
                out_data: v,
                self_latency: 3,
            }
        }
    }
}
