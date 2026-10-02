use crate::vertex::ports::Transformed;

#[derive(Clone, Debug)]
pub struct Input {
    pub id: u32,
    pub vertices: [Transformed; 3],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// Three homogeneous fields plus base/delta attribute reconstruction.
    Basis,
    /// One denominator and eight precomputed attribute numerator fields.
    Planes,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldOrigin {
    Local,
    /// Precision experiment: coefficients relative to screen origin.
    Global,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverageOrigin {
    Global,
    /// Rebase edges to the first snapped vertex: two edge constants are zero.
    FirstVertex,
}
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub width: u16,
    pub height: u16,
    pub near_raw: i32,
    pub far_raw: i32,
    pub guard_log2: u8,
    pub subpixel_bits: u8,
    pub cull_back: bool,
    pub coverage_origin: CoverageOrigin,
    pub intersection_fraction: Option<u8>,
    /// Shared binary scale; signed coefficient mantissa including sign/guard.
    pub field_bits: Option<u8>,
    pub field_origin: FieldOrigin,
    pub interpolation: Interpolation,
    pub attribute_fraction: u8,
    pub rgb_affine: bool,
    pub max_samples: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            width: 400,
            height: 240,
            near_raw: 8199,
            far_raw: 200 * 65536,
            guard_log2: 3,
            subpixel_bits: 4,
            cull_back: false,
            coverage_origin: CoverageOrigin::FirstVertex,
            intersection_fraction: None,
            field_bits: None,
            field_origin: FieldOrigin::Local,
            interpolation: Interpolation::Basis,
            attribute_fraction: 17,
            rgb_affine: false,
            max_samples: 96000,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.width == 0
            || self.width > 400
            || self.height == 0
            || self.height > 240
            || self.near_raw <= 0
            || self.far_raw <= self.near_raw
            || self.guard_log2 > 3
            || !(1..=8).contains(&self.subpixel_bits)
            || self
                .intersection_fraction
                .is_some_and(|b| !(16..=30).contains(&b))
            || self.field_bits.is_some_and(|b| !(8..=52).contains(&b))
            || !(4..=24).contains(&self.attribute_fraction)
            || self.max_samples == 0
            || self.max_samples > 1_000_000
        {
            return Err("triangle oracle configuration bounds".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuantizedSample {
    /// Unwrapped, signed UV. Extrapolation from coverage snap is retained.
    pub uv: [i64; 2],
    /// Signed Q14; deliberately not normalized or silently clipped to i16.
    pub normal: [i32; 3],
    pub rgb565: u16,
    pub depth: u16,
}
#[derive(Clone, Debug)]
pub struct Sample {
    pub position: [f64; 2],
    pub beta: [f64; 3],
    pub uv: [f64; 2],
    pub rgb: [f64; 3],
    pub normal: [f64; 3],
    pub w: f64,
    pub quantized: QuantizedSample,
}
