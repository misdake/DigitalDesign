use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceType};

#[derive(Default)]
pub struct DevicePrint {}
impl Device for DevicePrint {
    fn device_type(&self) -> DeviceType {
        DeviceType::Print
    }
    fn exec(&mut self, _opcode: u8, reg0: u8, _reg1: u8) -> DeviceReadResult {
        println!("DevicePrint {reg0}");
        DeviceReadResult {
            reg0_write_data: reg0,
            self_latency: 1,
        }
    }
}

#[test]
fn test_device_print() {
    use crate::cpu_v1::devices::test_device;
    use crate::cpu_v1::isa::*;

    test_device(
        &[
            inst_load_imm(DeviceType::Print as u8),
            inst_set_bus_addr0(),
            inst_load_imm(1),
            inst_bus0(0), // => print 1
            inst_load_imm(2),
            inst_bus0(0), // => print 2
            inst_load_imm(DeviceType::Print as u8),
            inst_set_bus_addr1(),
            inst_load_imm(3),
            inst_bus1(0), // => print 3
            inst_load_imm(4),
            inst_bus1(0), // => print 4
        ],
        20,
        [4, 0, 0, 0],
    );
}
