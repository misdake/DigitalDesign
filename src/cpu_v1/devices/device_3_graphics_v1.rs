use crate::cpu_v1::devices::device_2_and_3_util::{FrameBuffer, FrameBufferController};
use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceType};
use std::time::Duration;

pub struct DeviceGraphicsV1 {
    fb_controller: FrameBufferController,
    last_frame_id: usize,

    width: usize,
    height: usize,
    buffer: Vec<u32>,
    palette: [u32; 16],
    cursor_x: usize,
    cursor_y: usize,
}

#[repr(u8)]
#[allow(unused)]
pub enum DeviceGraphicsV1Opcode {
    WaitNextFrame = 0,
    Resize,       // width = regA, height = regB
    Clear,        // clear with regA color
    SetPalette,   // palette[regB] = palette_src[regA]
    SetCursor,    // x = regA, y = regB
    SetColor,     // buffer[x, y] = palette[regA]
    SetColorNext, // set color, then next position
    PresentFrame, // send buffer to window
}

impl Device for DeviceGraphicsV1 {
    fn device_type(&self) -> DeviceType {
        DeviceType::GraphicsV1
    }
    fn exec(&mut self, opcode3: u8, reg0: u8, reg1: u8) -> DeviceReadResult {
        let opcode: DeviceGraphicsV1Opcode = unsafe { std::mem::transmute(opcode3) };
        match opcode {
            DeviceGraphicsV1Opcode::WaitNextFrame => {
                while self.last_frame_id == self.fb_controller.get_presented_frame_id() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                self.last_frame_id = self.fb_controller.get_presented_frame_id();
            }
            DeviceGraphicsV1Opcode::Resize => {
                self.resize(reg0, reg1);
            }
            DeviceGraphicsV1Opcode::Clear => {
                self.set_position(0, 0);
                self.clear_with_color(reg0);
            }
            DeviceGraphicsV1Opcode::SetPalette => {
                self.set_palette(reg1, reg0);
            }
            DeviceGraphicsV1Opcode::SetCursor => {
                self.set_position(reg0, reg1);
            }
            DeviceGraphicsV1Opcode::SetColor => {
                self.set_color(reg0);
            }
            DeviceGraphicsV1Opcode::SetColorNext => {
                self.set_color(reg0);
                self.next_position();
            }
            DeviceGraphicsV1Opcode::PresentFrame => {
                self.present_frame();
            }
        };

        DeviceReadResult {
            reg0_write_data: reg0,
            self_latency: 0,
        }
    }
}

/// https://thestarman.pcministry.com/RGB/16WinColorT.html
/// u32 in bgra format
const PALETTE_16: [u32; 16] = [
    0x000000FF, //Black
    0x000080FF, //Maroon
    0x008000FF, //Green
    0x008080FF, //Olive
    0x800000FF, //Navy
    0x800080FF, //Purple
    0x808000FF, //Teal
    0xC0C0C0FF, //Silver
    0x808080FF, //Gray
    0x0000FFFF, //Red
    0x00FF00FF, //Lime
    0x00FFFFFF, //Yellow
    0xFF0000FF, //Blue
    0xFF00FFFF, //Fuchsia
    0xFFFF00FF, //Aqua
    0xFFFFFFFF, //White
];
#[repr(u8)]
#[allow(unused)]
pub enum Color {
    Black = 0,
    Maroon,
    Green,
    Olive,
    Navy,
    Purple,
    Teal,
    Silver,
    Gray,
    Red,
    Lime,
    Yellow,
    Blue,
    Fuchsia,
    Aqua,
    White,
}

impl DeviceGraphicsV1 {
    pub fn create(fb_controller: FrameBufferController) -> Self {
        Self {
            fb_controller,
            last_frame_id: 0x1000,
            width: 5,
            height: 5,
            buffer: vec![0; 25],
            palette: PALETTE_16, // copy as default
            cursor_x: 0,
            cursor_y: 0,
        }
    }

    fn resize(&mut self, width: u8, height: u8) {
        self.width = width as usize;
        self.height = height as usize;
        let len = (width * height) as usize;
        self.buffer = vec![0; len];
    }
    fn set_palette(&mut self, dst_index: u8, src_index: u8) {
        self.palette[dst_index as usize] = PALETTE_16[src_index as usize];
    }
    fn set_position(&mut self, x: u8, y: u8) {
        self.cursor_x = x as usize;
        self.cursor_y = y as usize;
        self.cursor_x %= self.width;
        self.cursor_y %= self.height;
    }
    fn next_position(&mut self) {
        self.cursor_x += 1;
        if self.cursor_x >= self.width {
            self.cursor_y += 1;
        }
        self.cursor_x %= self.width;
        self.cursor_y %= self.height;
    }
    fn clear_with_color(&mut self, color_index: u8) {
        self.buffer.fill(self.palette[color_index as usize]);
    }
    fn set_color(&mut self, color_index: u8) {
        self.buffer[self.width * self.cursor_y + self.cursor_x] =
            self.palette[color_index as usize];
    }
    fn present_frame(&mut self) {
        self.fb_controller.send_framebuffer(FrameBuffer {
            id: (self.last_frame_id + 1) % 0x1000,
            w: self.width,
            h: self.height,
            buffer: self.buffer.clone(),
        })
    }
}

#[test]
fn test_frame_sync() {
    use crate::cpu_v1::devices::device_2_gamepad::*;
    use crate::cpu_v1::devices::*;
    use crate::cpu_v1::isa::*;

    test_device(
        &[
            // init devices and modes
            inst_load_imm(DeviceType::Gamepad as u8),
            inst_set_bus_addr0(),
            inst_load_imm(DeviceType::GraphicsV1 as u8),
            inst_set_bus_addr1(),
            inst_load_imm(ButtonQueryMode::Press as u8),
            inst_bus0(DeviceGamepadOpcode::SetButtonQueryMode as u8), // set press mode
            // nops to align game logic
            inst_mov(0, 0), // nop
            inst_mov(0, 0), // nop
            inst_mov(0, 0), // nop
            inst_mov(0, 0), // nop
            inst_mov(0, 0), // nop
            inst_mov(0, 0), // nop
            inst_mov(0, 0), // nop
            inst_mov(0, 0), // nop
            inst_mov(0, 0), // nop
            inst_mov(0, 0), // nop
            // game logic, starting at 0x10 to longjump to
            inst_load_imm(Color::Navy as u8),
            inst_bus1(DeviceGraphicsV1Opcode::Clear as u8), // clear with color
            inst_load_imm(ButtonQueryType::ButtonStart as u8),
            inst_bus0(DeviceGamepadOpcode::QueryButton as u8), // query start button, pressed -> 1, not pressed -> 0
            inst_inc(0), // pressed -> 2 -> Green, not pressed -> 1 -> Maroon
            inst_bus1(DeviceGraphicsV1Opcode::SetColor as u8), // set color
            // game present
            inst_bus0(DeviceGamepadOpcode::NextFrame as u8),
            inst_bus1(DeviceGraphicsV1Opcode::PresentFrame as u8),
            inst_bus1(DeviceGraphicsV1Opcode::WaitNextFrame as u8),
            inst_jmp_long(1), // restart game logic
        ],
        100000000, // just big enough to keep it running
        [0, 0, 0, 0],
    );
}
