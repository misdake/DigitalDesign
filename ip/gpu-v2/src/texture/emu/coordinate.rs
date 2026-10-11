//! Independent generic calendar and selected shared-axis II2 executors.
//! Each owns only physical bits and phase/valid; structural descriptors
//! contain no sampled arithmetic answers. Input-only wires disappear after the
//! acceptance edge. This preserves the frozen coordinate contract (Q8 fraction,
//! signed repeat wrap, nearest/bilinear/trilinear selection) while removing the
//! counted answer dependency.
use super::derivative::{scalar, Calendar, Edge, Registers};

pub const NUMERIC_BITS: usize = 188;
pub const CONTROL_BITS: usize = 5;
pub const SPAN: u8 = 2;
pub const II: u32 = 2;
pub const PERIOD: u32 = 2;
pub mod compact_axis;

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
    fn validate(self) -> Result<(), String> {
        let side = self.side[0];
        if self.uv.iter().any(|v| *v >= 1 << 16)
            || !(2..=1024).contains(&side)
            || !(side as u16).is_power_of_two()
            || self.shift != (side as u16).ilog2() as i32 - 8
            || self.halve != (side > 2)
            || self.side[1] != (side / 2).max(2)
        {
            return Err("coordinate input range".into());
        }
        Ok(())
    }
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
            i.validate()?;
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

/// Physical allocation for the selected shared-axis executor, excluding the
/// unchanged generic calendar above. V/config32 and joint output112 bits.
pub const SHARED_NUMERIC_BITS: usize = 144;

/// Host-only evidence of the axis actually evaluated at this enabled edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisResult {
    pub fractions: [u8; 2],
    pub coordinates: [[u16; 2]; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedEdge {
    pub accepted: bool,
    pub axis: Option<(usize, AxisResult)>,
}

/// Two-edge U then V execution; no answers for V are calculated at admission.
/// Controls and numeric fields correspond to `rtl::coordinate` declarations.
#[derive(Default)]
pub struct SharedCoordinateEmu {
    v_input: u16,
    distance: u8,
    nearest: bool,
    halve: bool,
    mask: u16,
    result: u128,
    phase: u8,
    valid: u8,
}
impl SharedCoordinateEmu {
    pub fn new(calendar: Calendar) -> Result<Self, String> {
        if calendar.numeric_bits() != NUMERIC_BITS
            || calendar.span() != SPAN
            || calendar.ii() != II
            || calendar.period() != PERIOD
        {
            return Err("coordinate allocation mismatch".into());
        }
        let phase = Registers::new(calendar).phase();
        Ok(Self {
            phase,
            ..Self::default()
        })
    }
    pub fn reset(&mut self) {
        self.phase = 0;
        self.valid = 0;
    }
    pub fn phase(&self) -> u8 {
        self.phase
    }
    pub fn idle(&self) -> bool {
        self.valid == 0
    }
    /// A host snapshot of the physical144 bits, never a retained shadow bank.
    pub fn bank(&self) -> [u64; 3] {
        let config = u64::from(self.distance)
            | u64::from(self.nearest) << 4
            | u64::from(self.halve) << 5
            | u64::from(self.mask) << 6;
        [
            u64::from(self.v_input) | config << 16 | (self.result as u64) << 32,
            (self.result >> 32) as u64,
            (self.result >> 96) as u64,
        ]
    }
    pub fn output(&self) -> Result<Option<Output>, String> {
        Ok((self.valid & 4 != 0).then(|| Output {
            fractions: [[0, 8], [16, 24]].map(|shifts| shifts.map(|s| (self.result >> s) as u8)),
            coordinates: [[32, 42, 52, 62], [72, 82, 92, 102]]
                .map(|shifts| shifts.map(|s| ((self.result >> s) & 1023) as u16)),
        }))
    }
    fn axis(uv: u16, distance: u8, nearest: bool, halve: bool, mask: u16) -> AxisResult {
        let fine = ((i32::from(uv) << 2) >> distance) - if nearest { 0 } else { 128 };
        let coarse = if halve { (fine - 128) >> 1 } else { fine };
        let coarse_mask = (mask >> 1) | 1;
        AxisResult {
            fractions: [fine as u8, coarse as u8],
            coordinates: [(fine, mask), (coarse, coarse_mask)].map(|(value, mask)| {
                let index = value >> 8;
                [index, index + 1].map(|v| (v as u16) & mask)
            }),
        }
    }
    pub fn tick(&mut self, ce: bool, input: Option<Input>) -> Result<SharedEdge, String> {
        let mut edge = SharedEdge {
            accepted: false,
            axis: None,
        };
        if !ce {
            return Ok(edge);
        }
        if let Some(i) = input {
            i.validate()?;
            if self.phase != 0 {
                return Err("D/LOD initiation-interval admission".into());
            }
            let distance = (2 - i.shift) as u8;
            let mask = i.side[0] as u16 - 1;
            let axis = Self::axis(i.uv[0] as u16, distance, i.nearest, i.halve, mask);
            self.write_axis(0, axis);
            self.v_input = i.uv[1] as u16;
            self.distance = distance;
            self.nearest = i.nearest;
            self.halve = i.halve;
            self.mask = mask;
            edge.accepted = true;
            edge.axis = Some((0, axis));
        }
        if self.valid & 2 != 0 {
            let v = Self::axis(
                self.v_input,
                self.distance,
                self.nearest,
                self.halve,
                self.mask,
            );
            self.write_axis(1, v);
            edge.axis = Some((1, v));
        }
        self.phase ^= 1;
        self.valid = ((self.valid << 1) | u8::from(edge.accepted) << 1) & 7;
        Ok(edge)
    }
    fn write_axis(&mut self, axis: usize, row: AxisResult) {
        for plane in 0..2 {
            let shift = 16 * plane + 8 * axis;
            self.result =
                (self.result & !(255_u128 << shift)) | u128::from(row.fractions[plane]) << shift;
            for tap in 0..2 {
                let shift = 32 + 40 * plane + 20 * axis + 10 * tap;
                self.result = (self.result & !(1023_u128 << shift))
                    | u128::from(row.coordinates[plane][tap]) << shift;
            }
        }
    }
}
