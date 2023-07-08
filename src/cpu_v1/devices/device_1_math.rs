use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceType};

#[derive(Default)]
pub struct DeviceMath {
    value: Vec<u8>,
}
#[repr(u8)]
#[allow(unused)]
pub enum DeviceMathOpcode {
    Pop = 0,           // will pop to reg0
    ExtractBits = 1,   // split each bit of reg0 and push onto stack (& only), pop order low to high
    ExtractBits01 = 2, // push 4 bits of reg0 into stack (& then >>), pop order low to high
}
impl Device for DeviceMath {
    fn device_type(&self) -> DeviceType {
        DeviceType::Math
    }
    fn exec(&mut self, opcode3: u8, reg0: u8, _reg1: u8) -> DeviceReadResult {
        let opcode: DeviceMathOpcode = unsafe { std::mem::transmute(opcode3) };
        match opcode {
            DeviceMathOpcode::Pop => {
                return if let Some(v) = self.value.pop() {
                    println!("DeviceMath Pop {}", v);
                    DeviceReadResult {
                        reg0_write_data: v,
                        self_latency: 3,
                    }
                } else {
                    println!("DeviceMath Pop empty!");
                    DeviceReadResult {
                        reg0_write_data: 0,
                        self_latency: 3,
                    }
                }
            }
            DeviceMathOpcode::ExtractBits => {
                print!("DeviceMath ExtractBits reg0 {:04b}", reg0);
                self.value = vec![reg0 & 0b1000, reg0 & 0b0100, reg0 & 0b0010, reg0 & 0b0001];
            }
            DeviceMathOpcode::ExtractBits01 => {
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
        DeviceReadResult {
            reg0_write_data: reg0,
            self_latency: 3,
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
            inst_set_bus_addr0(),
            inst_load_imm(0b1010),
            inst_bus0(DeviceMathOpcode::ExtractBits01 as u8),
            inst_bus0(DeviceMathOpcode::Pop as u8), // & 0b0001
            inst_mov(0, 3),
            inst_bus0(DeviceMathOpcode::Pop as u8), // & 0b0010, >> 1
            inst_mov(0, 2),
            inst_bus0(DeviceMathOpcode::Pop as u8), // & 0b0100, >> 2
            inst_mov(0, 1),
            inst_bus0(DeviceMathOpcode::Pop as u8), // & 0b1000, >> 3
        ],
        15,
        [1, 0, 1, 0],
    );
}
