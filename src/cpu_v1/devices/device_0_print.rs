use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceType};

#[derive(Default)]
pub struct DevicePrint {}
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

#[test]
fn test_device_print() {
    use crate::cpu_v1::devices::test_device;
    use crate::cpu_v1::isa::*;

    test_device(
        &[
            inst_load_imm(DeviceType::Print as u8),
            inst_set_bus_addr(),
            inst_load_imm(1),
            inst_bus_read(),
        ],
        5,
        [1, 0, 0, 0],
    );
}
