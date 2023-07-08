use crate::cpu_v1::devices::device_2_and_3_util::{FrameBuffer, FrameBufferController};
use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceType};
use std::time::Duration;

pub struct DeviceGraphicsV1 {
    fb_controller: FrameBufferController,
    last_frame_id: usize,

    width: usize,
    height: usize,
    buffer: Vec<u32>,
    palette: &'static [u32; 16],
    cursor_x: usize,
    cursor_y: usize,
}

#[repr(u8)]
#[allow(unused)]
pub enum DeviceGraphicsV1Opcode {
    WaitNextFrame = 0,
    Resize,       // width = regA, height = regB
    SetCursor,    // x = regA, y = regB
    SetColor,     // buffer[x, y] = palette[regA]
    SetColorNext, // set color, then next position
    PresentFrame, // send buffer to window
}

impl Device for DeviceGraphicsV1 {
    fn device_type(&self) -> DeviceType {
        DeviceType::GraphicsV1
    }
    fn exec(&mut self, opcode4: u8, reg0: u8, reg1: u8) -> DeviceReadResult {
        let opcode: DeviceGraphicsV1Opcode = unsafe { std::mem::transmute(opcode4) };
        let r = match opcode {
            DeviceGraphicsV1Opcode::WaitNextFrame => {
                while self.last_frame_id == self.fb_controller.get_presented_frame_id() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                self.last_frame_id = self.fb_controller.get_presented_frame_id();
                None
            }
            DeviceGraphicsV1Opcode::Resize => {
                self.resize(reg0, reg1);
                None
            }
            DeviceGraphicsV1Opcode::SetCursor => {
                self.set_position(reg0, reg1);
                None
            }
            DeviceGraphicsV1Opcode::SetColor => {
                self.set_color(reg0);
                None
            }
            DeviceGraphicsV1Opcode::SetColorNext => {
                self.set_color(reg0);
                self.next_position();
                None
            }
            DeviceGraphicsV1Opcode::PresentFrame => {
                self.present_frame();
                None
            }
        };

        DeviceReadResult {
            reg0_write_data: r.unwrap_or(reg0),
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
pub enum Color {
    Black,
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
            palette: &PALETTE_16,
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
