use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceTerminalOp, DeviceType};

static mut ROM: Vec<u8> = Vec::new();

fn get_rom_content() -> &'static [u8] {
    unsafe { ROM.as_slice() }
}
pub fn set_rom_content(content: &[u8]) {
    if content.len() > 255 {
        println!("rom content is too long {}. max 255.", content.len())
    }
    unsafe {
        ROM = Vec::from(content);
    }
}

#[derive(Default)]
pub struct DeviceRom {
    cursor: usize,
}
#[repr(u8)]
#[allow(unused)]
pub enum DeviceRomOpcode {
    ReadNext = 0,
    SetCursorHigh,
    SetCursorLow,
    Skip,
    GetCursorHigh,
    GetCursorLow,
}
impl Device for DeviceRom {
    fn device_type(&self) -> DeviceType {
        DeviceType::Rom
    }
    fn exec(&mut self, opcode3: u8, reg0: u8, _reg1: u8) -> DeviceReadResult {
        let opcode: DeviceRomOpcode = unsafe { std::mem::transmute(opcode3) };
        let mut r = reg0;
        match opcode {
            DeviceRomOpcode::ReadNext => {
                let content = get_rom_content();
                let len = content.len();
                if (0..len).contains(&self.cursor) {
                    r = content[self.cursor]
                } else {
                    println!("reading rom oob!");
                };
                self.cursor += 1;
            }
            DeviceRomOpcode::SetCursorHigh => {
                self.cursor = (self.cursor & 0b00001111) | (reg0 << 4) as usize;
            }
            DeviceRomOpcode::SetCursorLow => {
                self.cursor = (self.cursor & 0b11110000) | reg0 as usize;
            }
            DeviceRomOpcode::Skip => {
                self.cursor += reg0 as usize;
            }
            DeviceRomOpcode::GetCursorHigh => {
                r = ((self.cursor >> 4) & 0b1111) as u8;
            }
            DeviceRomOpcode::GetCursorLow => {
                r = (self.cursor & 0b1111) as u8;
            }
        }
        DeviceReadResult {
            reg0_write_data: r,
            self_latency: 4,
        }
    }
}

#[test]
fn test_device_rom() {
    use crate::cpu_v1::devices::test_device;
    use crate::cpu_v1::isa::*;

    set_rom_content(&[0x1, 0x2, 0x3, 0x4]);

    test_device(
        &[
            inst_load_imm(DeviceType::Terminal as u8),
            inst_set_bus_addr1(),
            inst_load_imm(DeviceType::Rom as u8),
            inst_set_bus_addr0(),
            inst_bus0(DeviceRomOpcode::ReadNext as u8), // reg0 <= 1, cursor = 1
            inst_bus1(DeviceTerminalOp::Print as u8),   // print 1
            inst_load_imm(2),
            inst_bus0(DeviceRomOpcode::Skip as u8), // cursor = 3
            inst_bus0(DeviceRomOpcode::ReadNext as u8), // reg0 <= 4, cursor = 4(oob)
            inst_bus1(DeviceTerminalOp::Print as u8), // print 4
            inst_bus0(DeviceRomOpcode::GetCursorLow as u8), // reg0 <= 4
            inst_bus1(DeviceTerminalOp::Print as u8), // print 4
        ],
        13,
        [4, 0, 0, 0],
    );
}
