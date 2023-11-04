use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceTerminalOp, DeviceType};

static mut ROM: Vec<u8> = Vec::new();

fn get_rom_content() -> &'static [u8] {
    unsafe { ROM.as_slice() }
}
pub fn set_rom_content(content: &[u8]) {
    if content.len() > 256 {
        println!("rom content is too long {}. max 256.", content.len())
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
                    r = content[self.cursor];
                    // println!("read from rom: cursor {}, data {}", self.cursor, r);
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

#[test]
fn test_copy_rom_to_mem() {
    use crate::cpu_v1::devices::*;
    use crate::cpu_v1::isa::*;

    let mut rom = [0; 256];
    for i in 0..256 {
        rom[i] = (i / 16) as u8;
    }

    set_rom_content(rom.as_slice());

    test_device_full(
        &[
            inst_load_imm(DeviceType::Terminal as u8),
            inst_set_bus_addr1(),
            inst_load_imm(DeviceType::Rom as u8),
            inst_set_bus_addr0(),
            // inputs
            inst_load_imm(0), // cursor high
            inst_bus0(DeviceRomOpcode::SetCursorHigh as u8),
            inst_load_imm(0), // cursor low
            inst_bus0(DeviceRomOpcode::SetCursorLow as u8),
            inst_load_imm(0), // start mem page
            inst_mov(0, 3),   // write page to reg3
            inst_load_imm(0),
            inst_mov(0, 1), // reg1 <- 0
            // page loop
            inst_mov(3, 0),      // reg0 <- reg3
            inst_set_mem_page(), // set page <- reg0
            inst_inc(3),         // reg3++
            // inner loop
            inst_bus0(DeviceRomOpcode::ReadNext as u8), // reg0 <- rom[cursor++]
            inst_store_mem(0),                          // mem[page][reg1] <- reg0
            inst_inc(1),                                // reg1++
            inst_jne_offset(16 - 3),                    // jmp to inner loop if reg1 != 0 (overflow)
            // inner loop finish
            inst_mov(3, 3),          // set flags of reg3
            inst_jne_offset(16 - 8), // jmp to page loop if reg3 != 0 (overflow)
        ],
        1500,
        None,
        Some(rom),
    );
}
