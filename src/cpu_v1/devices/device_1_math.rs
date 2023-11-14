use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceType};

#[derive(Default)]
pub struct DeviceMath {
    value: Vec<u8>,
}
#[repr(u8)]
#[allow(unused)]
pub enum DeviceMathOpcode {
    Pop = 0,        // will pop to reg0
    PushBits = 1,   // split each bit of reg0 and push onto stack (& only), pop order low to high
    PushBits01 = 2, // push 4 bits of reg0 into stack (& then >>), pop order low to high
    ShiftLeft = 3,
    ShiftRight = 4,
}
impl Device for DeviceMath {
    fn device_type(&self) -> DeviceType {
        DeviceType::Math
    }
    fn exec(&mut self, opcode3: u8, reg0: u8, _reg1: u8) -> DeviceReadResult {
        let mut reg0 = reg0;
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
            DeviceMathOpcode::PushBits => {
                print!("DeviceMath ExtractBits reg0 {:04b}", reg0);
                self.value = vec![reg0 & 0b1000, reg0 & 0b0100, reg0 & 0b0010, reg0 & 0b0001];
            }
            DeviceMathOpcode::PushBits01 => {
                print!("DeviceMath ExtractBitsShift reg0 {:04b}", reg0);
                self.value = vec![
                    (reg0 & 0b1000) >> 3,
                    (reg0 & 0b0100) >> 2,
                    (reg0 & 0b0010) >> 1,
                    reg0 & 0b0001,
                ];
            }
            DeviceMathOpcode::ShiftLeft => {
                reg0 = reg0 * 2;
            }
            DeviceMathOpcode::ShiftRight => {
                reg0 = reg0 / 2;
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
    use crate::cpu_v1::devices::device_0_terminal::DeviceTerminalOp;
    use crate::cpu_v1::devices::test_device;
    use crate::cpu_v1::isa::Instruction::*;
    use crate::cpu_v1::isa::RegisterIndex::*;

    test_device(
        &[
            load_imm(DeviceType::Terminal as u8),
            set_bus_addr1(()),
            load_imm(DeviceType::Math as u8),
            set_bus_addr0(()),
            load_imm(0b1010),
            bus0(DeviceMathOpcode::PushBits01 as u8),
            bus0(DeviceMathOpcode::Pop as u8),   // & 0b0001 => 0
            bus1(DeviceTerminalOp::Print as u8), // print 0
            mov((Reg0, Reg3)),
            bus0(DeviceMathOpcode::Pop as u8), // & 0b0010, >> 1 => 1
            bus1(DeviceTerminalOp::Print as u8), // print 1
            mov((Reg0, Reg2)),
            bus0(DeviceMathOpcode::Pop as u8), // & 0b0100, >> 2 => 0
            bus1(DeviceTerminalOp::Print as u8), // print 0
            mov((Reg0, Reg1)),
            bus0(DeviceMathOpcode::Pop as u8), // & 0b1000, >> 3 => 1
            bus1(DeviceTerminalOp::Print as u8), // print 1
        ],
        20,
        [1, 0, 1, 0],
    );
}
