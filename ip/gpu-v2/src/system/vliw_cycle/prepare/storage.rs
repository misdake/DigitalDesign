//! Private PA/PB/PC and the registered A/B, live-C MULTALU36X18 boundary.
use super::super::vertex::Bits;

#[derive(Clone, Copy, Debug, Default)]
pub struct Ports {
    pub reset: bool,
    pub ce: bool,
    pub a_write: bool,
    pub a_write_address: u8,
    pub a_data: Bits,
    pub a_read: bool,
    pub a_address: u8,
    pub b_write: bool,
    pub b_write_address: u8,
    pub b_data: Bits,
    pub b_read: bool,
    pub b_address: u8,
    pub c_write: bool,
    pub c_write_address: u8,
    pub c_data: Bits,
    pub c_read: bool,
    pub c_address: u8,
}
#[derive(Clone, Debug)]
pub struct Storage {
    pub a: [Bits; 128],
    pub b: [Bits; 16],
    pub c: [Bits; 16],
    pub aq: Bits,
    pub b_address: u8,
    pub c_address: u8,
}
/// The same small fixed-factor decoder as iz_operand_table, not a ROM copy.
fn factor(address: usize) -> u64 {
    match address {
        32..=49 => 131072 >> (address - 32),
        64..=79 => {
            let b = address % 8;
            let step = 32 << if b > 4 { 0 } else { b };
            if address >= 72 {
                0u64.wrapping_sub(step) & 0x3ffff
            } else {
                step
            }
        }
        80 => 30,
        81 => 0x3ffff,
        96..=111 => {
            let step = 128 >> (address % 8);
            if address >= 104 {
                0u64.wrapping_sub(step) & 0x3ffff
            } else {
                step
            }
        }
        _ => 0,
    }
}
impl Default for Storage {
    fn default() -> Self {
        let mut a = [Bits::default(); 128];
        for (row, cell) in a.iter_mut().enumerate().skip(32) {
            *cell = Bits::new(factor(row), 18);
        }
        let mut c = [Bits::default(); 16];
        c[4] = Bits::new(0, 48);
        c[6] = Bits::new(0, 48);
        c[7] = Bits::new(131071, 48);
        Self {
            a,
            b: [Bits::default(); 16],
            c,
            aq: Bits::new(0, 18),
            b_address: 0,
            c_address: 0,
        }
    }
}
impl Storage {
    pub fn bq(&self) -> Bits {
        self.b[usize::from(self.b_address)]
    }
    pub fn cq(&self) -> Bits {
        self.c[usize::from(self.c_address)].extend(48, 54)
    }
    pub fn clock(&mut self, p: Ports) {
        if p.reset {
            self.aq = Bits::new(0, 18);
            self.b_address = 0;
            self.c_address = 0;
            return;
        }
        if !p.ce {
            return;
        }
        // PA pays a synchronous old-cell read. PB/PC pay address registers;
        // their asynchronous cells are observed through the held address.
        if p.a_read {
            self.aq = self.a[usize::from(p.a_address & 127)];
        }
        if p.a_write {
            self.a[usize::from(p.a_write_address & 15)] =
                Bits::concat(Bits::new(0, 1), p.a_data.slice(0, 17), 17);
        }
        if p.b_read {
            self.b_address = p.b_address & 15;
        }
        if p.c_read {
            self.c_address = p.c_address & 15;
        }
        if p.b_write {
            self.b[usize::from(p.b_write_address & 15)] = p.b_data.slice(0, 36);
        }
        if p.c_write {
            self.c[usize::from(p.c_write_address & 15)] = p.c_data.slice(0, 48);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Dsp {
    pub a: Bits,
    pub signed: bool,
    pub b: Bits,
    pub result: Bits,
}
impl Default for Dsp {
    fn default() -> Self {
        Self {
            a: Bits::new(0, 18),
            signed: false,
            b: Bits::new(0, 36),
            result: Bits::new(0, 54),
        }
    }
}
fn signed(value: u64, width: u32) -> i64 {
    ((value << (64 - width)) as i64) >> (64 - width)
}
impl Dsp {
    pub fn clock(&mut self, reset: bool, ce: bool, a: Bits, asign: bool, b: Bits, c: Bits) {
        if reset {
            *self = Self::default();
            return;
        }
        if !ce {
            return;
        }
        // C is live on the result edge, not the previous A/B capture edge.
        self.result = if self.a.is_known(18) && self.b.is_known(36) && c.is_known(54) {
            let av = if self.signed {
                signed(self.a.value, 18)
            } else {
                self.a.value as i64
            };
            Bits::new(
                (i128::from(av) * i128::from(signed(self.b.value, 36))
                    + i128::from(signed(c.value, 54))) as u64,
                54,
            )
        } else {
            Bits::default()
        };
        self.a = a;
        self.signed = asign;
        self.b = b;
    }
}
