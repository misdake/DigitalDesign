//! Actual coordinate registers on the certified II2 calendar. The shared
//! executor owns only physical bits and phase/valid; structural descriptors
//! contain no sampled arithmetic answers. Input-only wires disappear after the
//! acceptance edge. This preserves the frozen coordinate contract (Q8 fraction,
//! signed repeat wrap, nearest/bilinear/trilinear selection) while removing the
//! counted answer dependency.
use super::derivative::{scalar, Calendar, Edge, Registers};

pub const NUMERIC_BITS: usize = 716;
pub const CONTROL_BITS: usize = 18;
pub const SPAN: u8 = 9;
pub const II: u32 = 2;
pub const PERIOD: u32 = 8;

/// Retained coordinate outputs: fine/coarse Q8 fractions per axis and the four
/// wrapped texel taps per plane (axis-major: t{w}.{axis}.{tap}).
pub const OUTPUTS: &[&str] = &[
    "f0.0", "f0.1", "f1.0", "f1.1", "t0.0.0", "t0.0.1", "t0.1.0", "t0.1.1", "t1.0.0", "t1.0.1",
    "t1.1.0", "t1.1.1",
];

/// Scalar operands captured at admission. UV is already the derivative stage's
/// wrapped Q16 output; the LOD context supplies the shift/nearest/halve flags
/// and the per-plane physical side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub uv: [u32; 2],
    /// Exactly log2(side[0])-8; physical mip sides are powers of two, 2..1024.
    pub shift: i32,
    pub nearest: bool,
    pub halve: bool,
    pub side: [i16; 2],
}
impl Input {
    fn rows(self) -> Vec<(&'static str, usize, i128)> {
        vec![
            ("wrapped_uv", 0, i128::from(self.uv[0])),
            ("wrapped_uv", 1, i128::from(self.uv[1])),
            ("coordinate_shift", 0, i128::from(self.shift)),
            ("flags", 0, i128::from(self.nearest)),
            ("flags", 1, i128::from(self.halve)),
            ("side", 0, i128::from(self.side[0])),
            ("side", 1, i128::from(self.side[1])),
        ]
    }
}

/// Captured coordinate result in coefficient-operand order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    /// `[which][axis]` fine/coarse Q8 fraction.
    pub fractions: [[u8; 2]; 2],
    /// `[which][i]` wrapped tap, `i = 2*axis + tap`.
    pub coordinates: [[u16; 4]; 2],
}

pub struct CoordinateEmu {
    registers: Registers,
}
impl CoordinateEmu {
    pub fn new(calendar: Calendar) -> Result<Self, String> {
        if calendar.numeric_bits() != NUMERIC_BITS
            || calendar.span() != SPAN
            || calendar.ii() != II
            || calendar.period() != PERIOD
        {
            return Err("coordinate allocation mismatch".into());
        }
        Ok(Self {
            registers: Registers::new(calendar),
        })
    }
    pub fn output(&self) -> Result<Option<Output>, String> {
        self.registers
            .output()?
            .map(|v| {
                let raw = |name: &str| scalar(&v, name);
                Ok(Output {
                    fractions: std::array::from_fn(|w| {
                        std::array::from_fn(|a| raw(&format!("f{w}.{a}")) as u8)
                    }),
                    coordinates: std::array::from_fn(|w| {
                        std::array::from_fn(|i| raw(&format!("t{w}.{}.{}", i / 2, i % 2)) as u16)
                    }),
                })
            })
            .transpose()
    }
    pub fn tick(&mut self, ce: bool, input: Option<Input>) -> Result<Edge, String> {
        if let Some(i) = input.filter(|_| ce) {
            let side = i.side[0];
            if i.uv.iter().any(|v| *v >= 1 << 16)
                || !(2..=1024).contains(&side)
                || !(side as u16).is_power_of_two()
                || i.shift != (side as u16).ilog2() as i32 - 8
                || i.halve != (side > 2)
                || i.side[1] != (side / 2).max(2)
            {
                return Err("coordinate input range".into());
            }
        }
        let rows = input.map(Input::rows);
        self.registers.tick(ce, rows.as_deref())
    }
    pub fn idle(&self) -> bool {
        self.registers.idle()
    }
    pub fn phase(&self) -> u8 {
        self.registers.phase()
    }
    pub fn bank(&self) -> &[u64] {
        self.registers.bank()
    }
}
