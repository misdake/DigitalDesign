//! Clocked primitive boundaries used by the production four-phase interpolator.
use super::super::vertex::Bits;
pub fn signed(v: u64, n: u32) -> i64 {
    ((v << (64 - n)) as i64) >> (64 - n)
}
#[derive(Clone, Debug)]
pub struct Pair {
    pub w: [u32; 2],
    pub d: [Bits; 2],
    pub base: Bits,
    pub result: Bits,
}
impl Default for Pair {
    fn default() -> Self {
        Self {
            w: [0; 2],
            d: [Bits::new(0, 18); 2],
            base: Bits::new(0, 54),
            result: Bits::new(0, 54),
        }
    }
}
impl Pair {
    pub fn clock(&mut self, reset: bool, ce: bool, w: [u32; 2], p: Bits) {
        if reset {
            *self = Self::default();
            return;
        }
        if !ce {
            return;
        }
        self.result = if self.d.iter().all(|v| v.is_known(18)) && self.base.is_known(54) {
            Bits::new(
                (i128::from(self.w[0]) * i128::from(signed(self.d[0].value, 18))
                    + i128::from(self.w[1]) * i128::from(signed(self.d[1].value, 18))
                    + i128::from(signed(self.base.value, 54))) as u64,
                54,
            )
        } else {
            Bits::default()
        };
        self.w = w;
        self.d = [
            p.slice(0, 17).extend(17, 18),
            p.slice(17, 17).extend(17, 18),
        ];
        self.base = Bits::concat(p.slice(34, 17).extend(17, 38), Bits::new(0x8000, 16), 16);
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Multiply {
    pub a: u32,
    pub b: u32,
    pub result: u64,
}
impl Multiply {
    pub fn clock(&mut self, reset: bool, ce: bool, a: u32, b: u32) {
        if reset {
            *self = Self::default();
        } else if ce {
            self.result = u64::from(self.a) * u64::from(self.b);
            self.a = a & 0x3ffff;
            self.b = b & 0x3ffff;
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Window {
    pub mant: u32,
    pub d_exponent: u8,
    pub lod_abs: u64,
    pub abs_valid: bool,
    pub lod_valid: bool,
    pub lod_zero: bool,
    pub lod_exponent: u8,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowInput {
    pub reset: bool,
    pub ce: bool,
    pub phase: u8,
    pub d_valid: bool,
    pub h_valid: bool,
    pub d: u32,
    pub h: u32,
    pub lod_valid: bool,
    pub lod_value: Bits,
}
impl Window {
    pub fn pending(&self) -> bool {
        self.abs_valid || self.lod_valid
    }
    pub fn clock(&mut self, i: WindowInput) {
        if i.reset {
            *self = Self::default();
            return;
        }
        if !i.ce {
            return;
        }
        let value = match i.phase {
            0 => u64::from(i.d),
            2 => self.lod_abs,
            _ => u64::from(i.h),
        };
        let automatic = i.phase & 1 == 0;
        // This is the exact leading-one window, including the all-zero case.
        // H uses the saved D exponent, rather than normalizing each H itself.
        let leading = if value == 0 {
            0
        } else {
            63 - value.leading_zeros() as u8
        };
        let shift = if automatic {
            leading
        } else {
            self.d_exponent + 1
        };
        let mant = ((u128::from(value) << 17) >> shift) as u32 & 0x3ffff;
        let take_d = i.phase == 0 && i.d_valid;
        let take_h = i.phase & 1 != 0 && i.h_valid;
        let take_lod = i.phase == 2 && self.abs_valid;
        self.abs_valid = i.lod_valid;
        if i.lod_valid {
            self.lod_abs = signed(i.lod_value.value, 36).unsigned_abs() & 0xfffffffff;
        }
        self.lod_valid = take_lod;
        if take_d || take_h || take_lod {
            self.mant = mant;
        }
        if take_d {
            self.d_exponent = leading & 31;
        }
        if take_lod {
            self.lod_exponent = leading;
            self.lod_zero = value == 0;
        }
    }
}
