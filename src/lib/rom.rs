use crate::Wires;

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
        let data_wires: Vec<_> = self.data.iter().map(|v| Wires::<8>::parse_u8(*v)).collect();
        crate::mux16_w_v2(data_wires.as_slice(), in_addr)
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
