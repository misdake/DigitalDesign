use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceType};
use std::mem::transmute;

#[derive(Default)]
pub struct DeviceMath {
    value: Vec<u8>,
}
#[repr(u8)]
#[allow(unused)]
pub enum DeviceMathOpcode {
    Pop = 0,              // will write to reg0, or use inst_bus_read()
    ExtractBits = 1,      // split each bit of reg0 and push onto stack (& only)
    ExtractBitsShift = 2, // push 4 bits of reg0 into stack (& then >>)
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
                print!("DeviceMath ExtractBits reg0 {:04b}", reg0);
                self.value = vec![reg0 & 0b1000, reg0 & 0b0100, reg0 & 0b0010, reg0 & 0b0001];
            }
            DeviceMathOpcode::ExtractBitsShift => {
                print!("DeviceMath ExtractBitsShift reg0 {:04b}", reg0);
                self.value = vec![
                    (reg0 & 0b1000) >> 3,
                    (reg0 & 0b0100) >> 2,
                    (reg0 & 0b0010) >> 1,
                    reg0 & 0b0001,
                ];
            }
        }
        let values = self
            .value
            .iter()
            .map(|i| format!("{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        println!(" => [{values}]");
    }
    fn read(&mut self, _reg0: u8, _reg1: u8) -> DeviceReadResult {
        if self.value.is_empty() {
            DeviceReadResult {
                out_data: 0,
                self_latency: 3,
            }
        } else {
            let v = self.value.pop().unwrap();
            println!("DeviceMath Pop {}", v);
            DeviceReadResult {
                out_data: v,
                self_latency: 3,
            }
        }
    }
}

#[test]
fn test_device_extract_bits() {
    use crate::cpu_v1::devices::test_device;
    use crate::cpu_v1::isa::*;

    test_device(
        &[
            inst_load_imm(DeviceType::Math as u8),
            inst_set_bus_addr(),
            inst_load_imm(0b1010),
            inst_bus(DeviceMathOpcode::ExtractBitsShift as u8),
            inst_bus(DeviceMathOpcode::Pop as u8), // & 0b0001
            inst_mov(0, 3),
            inst_bus(DeviceMathOpcode::Pop as u8), // & 0b0010, >> 1
            inst_mov(0, 2),
            inst_bus(DeviceMathOpcode::Pop as u8), // & 0b0100, >> 2
            inst_mov(0, 1),
            inst_bus(DeviceMathOpcode::Pop as u8), // & 0b1000, >> 3
        ],
        1000,
        [1, 0, 1, 0],
    );
}
