//! UV-only binary weight study. Original fields/inverse and helper validity are retained.
//! Q12 is rounded once as a twelve-bit weight, then split hi9/lo3 for real products.
use super::basecolor_threefield::{PreparedBasis, Profile, Recovered};
use crate::material_color::ColorSource as Mode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SplitWeight {
    pub high9: u16,
    pub low3: u8,
    pub low_bits: u8,
    pub negative: bool,
    pub exponent: i16,
    pub endpoint: bool,
}
impl SplitWeight {
    pub fn quantize(v: f64, bits: u8) -> Result<Self, String> {
        if !v.is_finite() || !(9..=12).contains(&bits) {
            return Err("finite Q9..Q12 weight required".into());
        }
        let mut a = v.abs();
        let mut e = 0;
        while a > 1.0 {
            a *= 0.5;
            e += 1;
            assert!(e <= 1024);
        }
        let code = (a * f64::from(1u16 << bits)).round_ties_even() as u16;
        let endpoint = code == 1 << bits;
        let low_bits = bits - 9;
        Ok(Self {
            high9: if endpoint { 0 } else { code >> low_bits },
            low3: if endpoint {
                0
            } else {
                (code & ((1 << low_bits) - 1)) as u8
            },
            low_bits,
            negative: v.is_sign_negative(),
            exponent: e,
            endpoint,
        })
    }
    pub fn value(self) -> f64 {
        let code = (self.high9 << self.low_bits) | u16::from(self.low3);
        let v = if self.endpoint {
            1.0
        } else {
            f64::from(code) / f64::from(1u16 << (9 + self.low_bits))
        };
        let v = v * 2.0f64.powi(i32::from(self.exponent));
        if self.negative {
            -v
        } else {
            v
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    pub unsigned9_products: usize,
    pub fabric9x3_terms: usize,
    pub fabric_adds: usize,
    pub endpoint_bypasses: usize,
}
fn mul9(a: u16, b: u16, work: &mut Work) -> u32 {
    assert!(a <= 511 && b <= 511);
    if a == 0 || b == 0 {
        return 0;
    }
    work.unsigned9_products += 1;
    u32::from(a) * u32::from(b)
}
/// Explicit three-bit shift/add correction; no signed9 operand and no free multiplier.
fn fabric9x3(a: u16, b: u8, work: &mut Work) -> u32 {
    assert!(a <= 511 && b <= 7);
    if a == 0 || b == 0 {
        return 0;
    }
    work.fabric9x3_terms += 1;
    let mut out = 0;
    for bit in 0..3 {
        if b & (1 << bit) != 0 {
            out += u32::from(a) << bit;
            work.fabric_adds += 1;
        }
    }
    out
}
/// Assemble one signed delta*weight into raw-code F16, without intermediate Q9 rounding.
/// Exact delta12 is split low9/high3 as well; it costs extra products/corrections.
pub fn term_q16(
    delta: i32,
    delta_exponent: u8,
    weight: SplitWeight,
    work: &mut Work,
) -> Result<i128, String> {
    let mag = delta.unsigned_abs();
    if mag > 4095
        || weight.high9 > 511
        || weight.low_bits > 3
        || weight.low3 >= 1 << weight.low_bits
        || weight.exponent < 0
    {
        return Err("explicit magnitude/sign/scale operand bounds".into());
    }
    let product = if weight.endpoint {
        work.endpoint_bypasses += usize::from(mag != 0);
        u64::from(mag) << (9 + weight.low_bits)
    } else {
        let dlow = (mag & 511) as u16;
        let dhigh = (mag >> 9) as u8;
        let hi = mul9(dlow, weight.high9, work) + (fabric9x3(weight.high9, dhigh, work) << 9);
        let lo = fabric9x3(dlow, weight.low3, work)
            + (fabric9x3(u16::from(dhigh), weight.low3, work) << 9);
        (u64::from(hi) << weight.low_bits) + u64::from(lo)
    };
    let shift = 16u32 - (9 + u32::from(weight.low_bits))
        + u32::from(delta_exponent)
        + weight.exponent as u32;
    if shift > 100 {
        return Err("assembly research range; helper validity unchanged".into());
    }
    let term = i128::from(product)
        .checked_mul(1i128 << shift)
        .ok_or("assembly overflow")?;
    Ok(if (delta < 0) ^ weight.negative {
        -term
    } else {
        term
    })
}

#[derive(Clone, Debug)]
pub struct MixedRecovered {
    pub beta: [f64; 3],
    pub represented: [f64; 3],
    pub weights: [SplitWeight; 2],
    pub raw: [f64; 6],
    pub w: f64,
    pub integer_q16: Option<[i128; 6]>,
    pub work: Work,
}
impl MixedRecovered {
    /// Actual signed UNORM12 /4095 -> UV F16 transport RNE. Divider hardware is not free.
    pub fn uv_transport_q16(&self) -> Result<[i128; 2], String> {
        let raw = self
            .integer_q16
            .ok_or("exact-f64 weight has no integer transport proof")?;
        Ok(std::array::from_fn(|i| {
            let q = raw[i].div_euclid(4095);
            let r = raw[i].rem_euclid(4095);
            q + i128::from(2 * r > 4095 || 2 * r == 4095 && q & 1 != 0)
        }))
    }
    pub fn normal(&self, mode: Mode) -> [f64; 3] {
        let start = if mode == Mode::Texture { 2 } else { 3 };
        std::array::from_fn(|i| self.raw[start + i] / 1024.0)
    }
    pub fn uv(&self) -> [f64; 2] {
        [self.raw[0] / 4095.0, self.raw[1] / 4095.0]
    }
    pub fn rgb(&self) -> [f64; 3] {
        std::array::from_fn(|i| self.raw[i] / 255.0)
    }
}
/// None means exact weights. Color/normal always use independent Q9 when weights are narrow.
pub fn recover(
    b: &PreparedBasis,
    point: [f64; 2],
    uv_bits: Option<u8>,
    narrow_deltas: bool,
) -> Result<MixedRecovered, String> {
    // Original exact recovery is the validity provider, before all approximations.
    let exact = b.recover(point, Profile::Exact, false)?;
    let Recovered {
        mut raw, w, beta, ..
    } = b.recover(
        point,
        if uv_bits.is_some() {
            Profile::Q9
        } else {
            Profile::Exact
        },
        narrow_deltas,
    )?;
    let mut weights = [SplitWeight::default(); 2];
    let mut represented = exact.beta;
    let mut work = Work::default();
    let mut integer = None;
    if let Some(bits) = uv_bits {
        for i in 0..2 {
            weights[i] =
                SplitWeight::quantize(beta[i + 1], if b.mode == Mode::Texture { bits } else { 9 })?;
            represented[i + 1] = weights[i].value();
        }
        represented[0] = 1.0 - represented[1] - represented[2];
        let mut codes = [0i128; 6];
        for (k, output) in codes.iter_mut().enumerate().take(b.channels) {
            *output = i128::from(b.source[0][k]) << 16;
            if b.constant[k] {
                continue;
            }
            for j in 0..2 {
                let weight = if b.mode == Mode::Texture && k < 2 {
                    weights[j]
                } else {
                    SplitWeight::quantize(beta[j + 1], 9)?
                };
                let (delta, exponent) = if narrow_deltas {
                    let d = b.narrow_delta[j][k];
                    (
                        if d.negative {
                            -i32::from(d.magnitude)
                        } else {
                            i32::from(d.magnitude)
                        },
                        d.exponent,
                    )
                } else {
                    (b.delta[j][k], 0)
                };
                *output = output
                    .checked_add(term_q16(delta, exponent, weight, &mut work)?)
                    .ok_or("assembly accumulator overflow")?;
            }
            if b.mode == Mode::Texture && k < 2 {
                // Report floating reconstruction separately from final F16 transport.
                raw[k] = f64::from(b.source[0][k]);
                for (j, weight) in weights.iter().enumerate() {
                    let d = if narrow_deltas {
                        b.narrow_delta[j][k].value()
                    } else {
                        b.delta[j][k]
                    };
                    raw[k] += f64::from(d) * weight.value();
                }
            }
        }
        integer = Some(codes);
    }
    Ok(MixedRecovered {
        beta,
        represented,
        weights,
        raw,
        w,
        integer_q16: integer,
        work,
    })
}
