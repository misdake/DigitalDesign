use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceType};
use std::time::Duration;

#[derive(Default)]
pub struct DeviceTerminal {}
impl Device for DeviceTerminal {
    fn device_type(&self) -> DeviceType {
        DeviceType::Terminal
    }
    fn exec(&mut self, opcode: u8, reg0: u8, _reg1: u8) -> DeviceReadResult {
        let opcode: DeviceTerminalOp = unsafe { std::mem::transmute(opcode) };
        match opcode {
            DeviceTerminalOp::Print => {
                println!("DeviceTerminal print: {reg0}");
            }
            DeviceTerminalOp::Halt => {
                std::process::exit(0);
            }
            DeviceTerminalOp::Sleep => {
                std::thread::sleep(Duration::from_millis(reg0 as u64));
            }
        }
        DeviceReadResult {
            reg0_write_data: reg0,
            self_latency: 1,
        }
    }
}

#[allow(unused)]
#[repr(u8)]
pub enum DeviceTerminalOp {
    Print = 0,
    Halt,
    Sleep,
}

#[test]
fn test_print() {
    use crate::cpu_v1::devices::test_device;
    use crate::cpu_v1::isa::*;

    test_device(
        &[
            inst_load_imm(DeviceType::Terminal as u8),
            inst_set_bus_addr0(),
            inst_load_imm(1),
            inst_bus0(DeviceTerminalOp::Print as u8), // => print 1
            inst_load_imm(2),
            inst_bus0(DeviceTerminalOp::Print as u8), // => print 2
            inst_load_imm(DeviceType::Terminal as u8),
            inst_set_bus_addr1(),
            inst_load_imm(3),
            inst_bus1(DeviceTerminalOp::Print as u8), // => print 3
            inst_load_imm(4),
            inst_bus1(DeviceTerminalOp::Print as u8), // => print 4
        ],
        20,
        [4, 0, 0, 0],
    );
}
