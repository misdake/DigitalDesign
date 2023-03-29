use crate::{mux16_w, mux256_w, Wires};

pub struct Rom16x8 {
    data: [u8; 16],
}
impl Rom16x8 {
    pub fn create() -> Self {
        Self { data: [0; 16] }
    }
    pub fn set(&mut self, addr: u8, value: u8) {
        self.data[addr as usize] = value;
    }
    pub fn apply(self, in_addr: Wires<4>) -> ([Wires<8>; 16], Wires<8>) {
        let data_wires = self.data.map(|v| Wires::<8>::parse_u8(v));
        let output = mux16_w(data_wires.as_slice(), in_addr);
        (data_wires, output)
    }
}

pub struct Rom256x8 {
    data: [u8; 256],
}
impl Rom256x8 {
    pub fn create() -> Self {
        Self { data: [0; 256] }
    }
    pub fn set(&mut self, addr: u8, value: u8) {
        self.data[addr as usize] = value;
    }
    pub fn apply(self, in_addr: Wires<8>) -> ([Wires<8>; 256], Wires<8>) {
        let data_wires = self.data.map(|v| Wires::<8>::parse_u8(v));
        let output = mux256_w(data_wires.as_slice(), in_addr);
        (data_wires, output)
    }
}

#[test]
fn test_rom16x8() {
    use crate::*;
    clear_all();

    let mut rom = Rom16x8::create();
    for i in 0..16 {
        rom.set(i, 16 - i);
    }

    let addr = input_w::<4>();
    let (_, data) = rom.apply(addr);

    for i in 0..16 {
        addr.set_u8(i);
        simulate();
        assert_eq!(16 - i, data.get_u8());
    }
}
#[test]
fn test_rom256x8() {
    use crate::*;
    clear_all();

    let mut rom = Rom256x8::create();
    for i in 0..=255 {
        rom.set(i, 255 - i);
    }

    let addr = input_w::<8>();
    let (_, data) = rom.apply(addr);

    for i in 0..=255 {
        addr.set_u8(i);
        simulate();
        assert_eq!(255 - i, data.get_u8());
    }
}
