//! Selected fine-only coordinate storage; full output views are combinational compatibility wiring.
use super::*;
pub const SHARED_NUMERIC_BITS: usize = 68;

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
pub struct CompactCoordinateEmu {
    v_input: u16,
    distance: u8,
    nearest: bool,
    halve: bool,
    mask: u16,
    result: u128,
    phase: u8,
    valid: u8,
}
impl CompactCoordinateEmu {
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
    /// A host snapshot of the physical68 bits, never a retained shadow bank.
    pub fn bank(&self) -> [u64; 3] {
        let config = u64::from(self.distance)
            | u64::from(self.nearest) << 4
            | u64::from(self.halve) << 5
            | u64::from(self.mask) << 6;
        [
            u64::from(self.v_input) | config << 16 | (self.result as u64) << 32,
            (self.result >> 32) as u64 & 15,
            (self.result >> 96) as u64,
        ]
    }
    pub fn output(&self) -> Result<Option<Output>, String> {
        Ok((self.valid & 4 != 0).then(|| {
            super::super::compact::expand_result(self.result as u64, (self.mask + 1).ilog2() as u8)
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
        let f = axis * 8;
        let b = 16 + axis * 10;
        self.result = (self.result & !(255u128 << f) & !(1023u128 << b))
            | u128::from(row.fractions[0]) << f
            | u128::from(row.coordinates[0][0]) << b;
    }
}
