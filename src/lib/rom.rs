use crate::{mux16_w, Wires};

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
    pub fn apply(self, in_addr: Wires<4>) -> Wires<8> {
        let data_wires: [Wires<8>; 16] = [
            Wires::<8>::parse_u8(self.data[0]),
            Wires::<8>::parse_u8(self.data[1]),
            Wires::<8>::parse_u8(self.data[2]),
            Wires::<8>::parse_u8(self.data[3]),
            Wires::<8>::parse_u8(self.data[4]),
            Wires::<8>::parse_u8(self.data[5]),
            Wires::<8>::parse_u8(self.data[6]),
            Wires::<8>::parse_u8(self.data[7]),
            Wires::<8>::parse_u8(self.data[8]),
            Wires::<8>::parse_u8(self.data[9]),
            Wires::<8>::parse_u8(self.data[10]),
            Wires::<8>::parse_u8(self.data[11]),
            Wires::<8>::parse_u8(self.data[12]),
            Wires::<8>::parse_u8(self.data[13]),
            Wires::<8>::parse_u8(self.data[14]),
            Wires::<8>::parse_u8(self.data[15]),
        ];

        mux16_w(data_wires, in_addr)
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
    let data = rom.apply(addr);

    for i in 0..16 {
        addr.set_u8(i);
        simulate();
        assert_eq!(16 - i, data.get_u8());
    }
}
