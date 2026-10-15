//! Independent three-field numeric experiment. Geometry widths/inverse are not frozen RTL.
//! Original unsnapped XYW/source is shared by fans. Only the selected albedo is retained.
use super::{
    ports::{FieldOrigin, Interpolation},
    sim::oracle::Report,
};
use crate::material_color::{Albedo, ColorSource as BaseColorSource};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Exact,
    Unorm9,
    Q9,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Weight9 {
    /// Nine unsigned magnitude bits; sign is separate, never clamped.
    pub magnitude: u16,
    pub negative: bool,
    /// Study covers nonnegative0..1024: eleven unsigned metadata bits (host i16).
    pub exponent: i16,
    /// Q9 code512 is an exact endpoint bypass; not a hidden tenth multiply bit.
    pub endpoint: bool,
}
impl Weight9 {
    pub fn quantize(value: f64, profile: Profile) -> Result<Self, String> {
        if !value.is_finite() || profile == Profile::Exact {
            return Err("finite narrow weight profile required".into());
        }
        let mut normalized = value.abs();
        let mut exponent = 0;
        while normalized > 1.0 {
            normalized *= 0.5;
            exponent += 1;
            assert!(exponent <= 1024);
        }
        let denominator = if profile == Profile::Unorm9 {
            511.0
        } else {
            512.0
        };
        let code = (normalized * denominator).round_ties_even() as u16;
        let endpoint = code == 512;
        Ok(Self {
            magnitude: if endpoint { 0 } else { code },
            negative: value.is_sign_negative(),
            exponent,
            endpoint,
        })
    }
    pub fn value(self, profile: Profile) -> f64 {
        let mut v = if self.endpoint {
            1.0
        } else {
            f64::from(self.magnitude)
                / if profile == Profile::Unorm9 {
                    511.0
                } else {
                    512.0
                }
        };
        for _ in 0..self.exponent {
            v *= 2.0;
        }
        if self.negative {
            -v
        } else {
            v
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Delta9 {
    pub magnitude: u16,
    pub negative: bool,
    pub exponent: u8,
}
impl Delta9 {
    pub fn encode(value: i32) -> Self {
        let absolute = value.unsigned_abs();
        let mut exponent = 0;
        loop {
            let divisor = 1u32 << exponent;
            let q = absolute / divisor;
            let r = absolute % divisor;
            let rounded = q + u32::from(2 * r > divisor || 2 * r == divisor && q & 1 != 0);
            if rounded <= 511 {
                return Self {
                    magnitude: rounded as u16,
                    negative: value < 0,
                    exponent,
                };
            }
            exponent += 1;
            assert!(exponent <= 4, "actual UV12/S12 delta domain");
        }
    }
    pub fn value(self) -> i32 {
        let v = i32::from(self.magnitude) << self.exponent;
        if self.negative {
            -v
        } else {
            v
        }
    }
}
#[derive(Clone, Debug)]
pub struct PreparedBasis {
    pub mode: BaseColorSource,
    pub channels: usize,
    pub fields: [[i128; 3]; 3],
    pub coefficients: [[f64; 3]; 3],
    pub anchor: [f64; 2],
    pub radius: f64,
    pub depth_k: f64,
    pub attribute_fraction: u8,
    /// Exact source codes: Color RGB8/normalS12; Texture UV12/normalS12.
    pub source: [[i32; 6]; 3],
    pub delta: [[i32; 6]; 2],
    pub narrow_delta: [[Delta9; 6]; 2],
    pub constant: [bool; 6],
}
fn cross(a: [i128; 3], b: [i128; 3]) -> [i128; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
impl PreparedBasis {
    /// Actual exact-integer cofactor preparation followed by an unfrozen f64 cache.
    /// Clipping/projection/coverage are supplied by the unchanged oracle report.
    pub fn prepare(
        report: &Report,
        albedo: [Albedo; 3],
        mode: BaseColorSource,
    ) -> Result<Self, String> {
        if report.config.interpolation != Interpolation::Basis || report.config.field_bits.is_some()
        {
            return Err("study requires original unquantized three-field Basis".into());
        }
        let original = report.source.as_ref().ok_or("no surviving source")?;
        let p = original.raw_clip.map(|v| {
            [
                i128::from(v[0]) + i128::from(v[3]),
                i128::from(v[3]) - i128::from(v[1]),
                2 * i128::from(v[3]),
            ]
        });
        let d1 = std::array::from_fn(|i| p[1][i] - p[0][i]);
        let d2 = std::array::from_fn(|i| p[2][i] - p[0][i]);
        let cof = [cross(d1, d2), cross(p[2], p[0]), cross(p[0], p[1])];
        let width = i128::from(report.config.width);
        let height = i128::from(report.config.height);
        let fields = cof.map(|f| [f[0] * height, f[1] * width, f[2] * width * height]);
        let determinant = (0..3).map(|i| p[0][i] * cof[0][i]).sum::<i128>() * width * height;
        if determinant == 0 {
            return Err("singular original source".into());
        }
        let (anchor2, radius) = if report.config.field_origin == FieldOrigin::Local {
            let x0 = report
                .triangles
                .iter()
                .map(|t| t.bbox[0])
                .min()
                .ok_or("no bbox")?;
            let y0 = report.triangles.iter().map(|t| t.bbox[1]).min().unwrap();
            let x1 = report.triangles.iter().map(|t| t.bbox[2]).max().unwrap();
            let y1 = report.triangles.iter().map(|t| t.bbox[3]).max().unwrap();
            let span = u32::from((x1 - x0 + 1).max(y1 - y0 + 1));
            (
                [
                    i128::from(x0) + i128::from(x1) + 1,
                    i128::from(y0) + i128::from(y1) + 1,
                ],
                span.div_ceil(2).next_power_of_two() as f64,
            )
        } else {
            ([0, 0], 1.0)
        };
        let coefficients = fields.map(|f| {
            [
                2.0 * radius * f[0] as f64,
                2.0 * radius * f[1] as f64,
                (f[0] * anchor2[0] + f[1] * anchor2[1] + 2 * f[2]) as f64,
            ]
        });
        let channels = if mode == BaseColorSource::VertexColor {
            6
        } else {
            5
        };
        let mut source = [[0; 6]; 3];
        for i in 0..3 {
            let offset = match (mode, albedo[i]) {
                (BaseColorSource::VertexColor, Albedo::Color(rgb)) => {
                    for (k, v) in rgb.into_iter().enumerate() {
                        source[i][k] = i32::from(v);
                    }
                    3
                }
                (BaseColorSource::Texture, Albedo::Texture(uv)) => {
                    if uv.iter().any(|&u| u > 4095) {
                        return Err("UV12 source domain".into());
                    }
                    source[i][0] = i32::from(uv[0]);
                    source[i][1] = i32::from(uv[1]);
                    2
                }
                _ => return Err("draw/source albedo mismatch".into()),
            };
            for k in 0..3 {
                let code = (original.attributes[i][5 + k] * 1024.0).round_ties_even();
                if !(-2048.0..=2047.0).contains(&code)
                    || code != original.attributes[i][5 + k] * 1024.0
                {
                    return Err("exact S12F10 source normal required".into());
                }
                source[i][offset + k] = code as i32;
            }
        }
        let delta = std::array::from_fn::<_, 2, _>(|j| {
            std::array::from_fn(|k| source[j + 1][k] - source[0][k])
        });
        let narrow_delta = delta.map(|d| d.map(Delta9::encode));
        let constant =
            std::array::from_fn(|k| source[0][k] == source[1][k] && source[0][k] == source[2][k]);
        Ok(Self {
            mode,
            channels,
            fields,
            coefficients,
            anchor: anchor2.map(|v| v as f64 * 0.5),
            radius,
            depth_k: determinant as f64 / original.raw_scale,
            attribute_fraction: report.config.attribute_fraction,
            source,
            delta,
            narrow_delta,
            constant,
        })
    }
    pub fn weights(&self, point: [f64; 2]) -> Result<([f64; 3], f64), String> {
        if point.iter().any(|p| !p.is_finite() || p.abs() > 4096.0) {
            return Err("original helper position bound".into());
        }
        let local = [
            (point[0] - self.anchor[0]) / self.radius,
            (point[1] - self.anchor[1]) / self.radius,
            1.0,
        ];
        let values = self
            .coefficients
            .map(|f| (0..3).map(|i| f[i] * local[i]).sum::<f64>());
        let inv = 1.0 / values[0];
        let beta1 = values[1] * inv;
        let beta2 = values[2] * inv;
        let w = self.depth_k * inv;
        if !w.is_finite() || w <= 0.0 || !beta1.is_finite() || !beta2.is_finite() {
            return Err("original nonpositive/nonfinite W or weights".into());
        }
        Ok(([1.0 - beta1 - beta2, beta1, beta2], w))
    }
    pub fn recover(
        &self,
        point: [f64; 2],
        profile: Profile,
        narrow_deltas: bool,
    ) -> Result<Recovered, String> {
        let (beta, w) = self.weights(point)?;
        // Validate the original selected attribute domain before approximation. A narrow
        // weight range never changes the helper's original valid/invalid classification.
        let exact = std::array::from_fn::<_, 6, _>(|k| {
            f64::from(self.source[0][k])
                + f64::from(self.delta[0][k]) * beta[1]
                + f64::from(self.delta[1][k]) * beta[2]
        });
        let normal_offset = if self.mode == BaseColorSource::Texture {
            2
        } else {
            3
        };
        if exact[..self.channels].iter().any(|v| !v.is_finite()) {
            return Err("original nonfinite attribute".into());
        }
        if exact[normal_offset..normal_offset + 3]
            .iter()
            .any(|v| !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&v.round_ties_even()))
        {
            return Err("original normal output overflow".into());
        }
        if self.mode == BaseColorSource::Texture
            && exact[..2].iter().any(|v| {
                (v / 4095.0 * 2.0f64.powi(i32::from(self.attribute_fraction))).abs()
                    > i64::MAX as f64 - 4096.0
            })
        {
            return Err("original UV output overflow".into());
        }
        let mut represented = beta;
        let mut weights = [Weight9::default(); 2];
        if profile != Profile::Exact {
            for i in 0..2 {
                weights[i] = Weight9::quantize(beta[i + 1], profile)?;
                represented[i + 1] = weights[i].value(profile);
            }
            represented[0] = 1.0 - represented[1] - represented[2];
        }
        let mut raw = [0.0; 6];
        let mut small_products = 0;
        for (k, out) in raw.iter_mut().enumerate().take(self.channels) {
            *out = f64::from(self.source[0][k]);
            if self.constant[k] {
                continue;
            }
            for j in 0..2 {
                let delta = if narrow_deltas {
                    self.narrow_delta[j][k].value()
                } else {
                    self.delta[j][k]
                };
                *out += f64::from(delta) * represented[j + 1];
                if profile != Profile::Exact
                    && narrow_deltas
                    && self.narrow_delta[j][k].magnitude != 0
                    && !weights[j].endpoint
                {
                    small_products += 1;
                }
            }
        }
        Ok(Recovered {
            beta,
            represented,
            weights,
            raw,
            w,
            small_products,
        })
    }
    /// Real unsigned9 products, separate signs/scale and fabric i128 assembly to raw-code F16.
    /// Unsupported assembly range is a research limitation, never invalid-UV policy.
    pub fn recover_integer_q16(
        &self,
        weights: [Weight9; 2],
        profile: Profile,
    ) -> Result<[i128; 6], String> {
        if profile == Profile::Exact {
            return Err("narrow profile required".into());
        }
        let denominator = if profile == Profile::Unorm9 {
            511i128
        } else {
            512
        };
        let mut raw = [0i128; 6];
        for (k, out) in raw.iter_mut().enumerate().take(self.channels) {
            *out = i128::from(self.source[0][k]) << 16;
            if self.constant[k] {
                continue;
            }
            let mut numerator = 0i128;
            for (j, weight) in weights.into_iter().enumerate() {
                let delta = self.narrow_delta[j][k];
                let shift = i32::from(delta.exponent) + i32::from(weight.exponent) + 16;
                if !(0..=100).contains(&shift) {
                    return Err(
                        "integer assembly research range; retain valid helper identity".into(),
                    );
                }
                let product = if weight.endpoint {
                    i128::from(delta.magnitude) * denominator
                } else {
                    i128::from(delta.magnitude) * i128::from(weight.magnitude)
                };
                let term = product
                    .checked_shl(shift as u32)
                    .ok_or("integer assembly overflow")?;
                if weight.negative ^ delta.negative {
                    numerator -= term;
                } else {
                    numerator += term;
                }
            }
            let q = numerator.div_euclid(denominator);
            let r = numerator.rem_euclid(denominator);
            *out += q + i128::from(2 * r > denominator || 2 * r == denominator && q & 1 != 0);
        }
        Ok(raw)
    }
}
#[derive(Clone, Debug)]
pub struct Recovered {
    pub beta: [f64; 3],
    pub represented: [f64; 3],
    pub weights: [Weight9; 2],
    pub raw: [f64; 6],
    pub w: f64,
    pub small_products: usize,
}
impl Recovered {
    pub fn normal(&self, mode: BaseColorSource) -> [f64; 3] {
        let start = if mode == BaseColorSource::Texture {
            2
        } else {
            3
        };
        std::array::from_fn(|i| self.raw[start + i] / 1024.0)
    }
    pub fn uv(&self) -> [f64; 2] {
        [self.raw[0] / 4095.0, self.raw[1] / 4095.0]
    }
    pub fn rgb(&self) -> [f64; 3] {
        std::array::from_fn(|i| self.raw[i] / 255.0)
    }
}
