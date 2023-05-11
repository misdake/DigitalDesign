use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceType};

#[derive(Default)]
pub struct DeviceGraphicsV1 {
    //TODO receiver for frame id
    //TODO sender for present frame
    frame_id: usize,
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
    GetFrameId = 0,
    Resize,       // width = regA, height = regB
    SetCursor,    // x = regA, y = regB
    NextPosition, // x++, if overflow y++
    SetColor,     // buffer[x, y] = palette[regA]
    SetColorNext, // set color, then next position
    PresentFrame, // send buffer to window
}

impl Device for DeviceGraphicsV1 {
    fn device_type(&self) -> DeviceType {
        DeviceType::GraphicsV1
    }
    fn exec(&mut self, _opcode4: u8, _reg0: u8, _reg1: u8) -> DeviceReadResult {
        todo!()
    }
}
