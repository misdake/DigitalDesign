//! Independent host reference: ideal equations and configurable quantized stages.
//! It never calls audited arithmetic. Stage integers are the counted-model goldens.
use super::super::{format, ports::*};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rounding {
    #[default]
    NearestEven,
    Floor,
    /// Equivalent to flooring unsigned magnitude and then restoring its sign.
    TowardZero,
    HalfUp,
}

/// Stage-isolated precision experiments. Generated ROM endpoints stay RNE.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoundingPolicy {
    pub normalization: Rounding,
    pub projection: Rounding,
    pub half: Rounding,
    pub dot: Rounding,
    pub rsqrt: Rounding,
    pub power: Rounding,
    pub output: Rounding,
}
impl RoundingPolicy {
    pub fn floor_all() -> Self {
        Self {
            normalization: Rounding::Floor,
            projection: Rounding::Floor,
            half: Rounding::Floor,
            dot: Rounding::Floor,
            rsqrt: Rounding::Floor,
            power: Rounding::Floor,
            output: Rounding::Floor,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub quantization: super::super::LightingQuantization,
    pub block_prescale: bool,
    pub direction_fraction: u32,
    pub reciprocal_fraction: u32,
    /// Extra interpolation/product fractional bits; ROM endpoints retain their format.
    /// This is an oracle experiment, not a change to the counted contract.
    pub reciprocal_work_extra: u32,
    pub dot_fraction: u32,
    pub power_fraction: u32,
    pub intensity_fraction: u32,
    pub approximate_square: bool,
    pub approximate_rsqrt: bool,
    pub approximate_power: bool,
    pub rounding: RoundingPolicy,
    /// Explicit oracle-only approximation experiment; never implicit sharing.
    pub half_ndc_override: Option<[i32; 2]>,
    pub normal_override: Option<[i16; 3]>,
    /// Independent scalar-dot normalization experiment (fixed Q14/Q15 ports).
    pub scalar_norm: bool,
    pub weighted_view: bool,
    /// System candidate: scalar N normalization and raw-dot sign gate.
    pub scalar_normal: bool,
    pub exact_normal_gate: bool,
    /// Use exact H squares while preserving N, V and half-vector generation.
    pub direct_square: bool,
    pub direct_all_squares: bool,
    pub square9: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            quantization: Default::default(),
            block_prescale: false,
            direction_fraction: format::Direction::FORMAT.fraction,
            reciprocal_fraction: format::Reciprocal::FORMAT.fraction,
            reciprocal_work_extra: 0,
            dot_fraction: format::Specular::FORMAT.fraction,
            power_fraction: format::Specular::FORMAT.fraction,
            intensity_fraction: format::Intensity::FORMAT.fraction,
            approximate_square: true,
            approximate_rsqrt: true,
            approximate_power: true,
            rounding: RoundingPolicy::default(),
            half_ndc_override: None,
            normal_override: None,
            scalar_norm: false,
            weighted_view: false,
            scalar_normal: false,
            exact_normal_gate: false,
            direct_square: false,
            direct_all_squares: false,
            square9: false,
        }
    }
}

impl Config {
    /// Independent numerical golden for the exact selected counted contract.
    pub fn from_counted(kernel: super::counted::Config) -> Self {
        let compensated =
            kernel.quantization == super::super::LightingQuantization::CompensatedFloor;
        Self {
            quantization: kernel.quantization,
            block_prescale: kernel.block_prescale,
            scalar_norm: kernel.scalar_norm,
            weighted_view: kernel.weighted_view,
            scalar_normal: kernel.scalar_normal,
            exact_normal_gate: kernel.exact_normal_gate,
            direct_square: kernel.direct_square,
            direct_all_squares: kernel.direct_all_squares,
            square9: kernel.square9,
            rounding: if compensated {
                RoundingPolicy {
                    dot: Rounding::HalfUp,
                    output: Rounding::NearestEven,
                    ..RoundingPolicy::floor_all()
                }
            } else {
                RoundingPolicy {
                    power: if kernel.power_floor {
                        Rounding::Floor
                    } else {
                        Rounding::NearestEven
                    },
                    ..Default::default()
                }
            },
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
pub struct Golden {
    trace: bool,
    pub stages: Vec<(String, i128)>,
    pub g: i128,
    pub h: i128,
    pub intensity_fraction: u32,
}
impl Golden {
    fn stage(&mut self, name: impl Into<String>, value: i128) -> i128 {
        if self.trace {
            self.stages.push((name.into(), value));
        }
        value
    }
    pub fn intensities(&self) -> [f64; 2] {
        let scale = (1_u64 << self.intensity_fraction) as f64;
        [self.g as f64 / scale, self.h as f64 / scale]
    }
}

/// Independent division/remainder rounding, including negative ties.
fn rne(n: i128, shift: u32) -> i128 {
    if shift == 0 {
        return n;
    }
    let d = 1_i128 << shift;
    let q = n.div_euclid(d);
    let r = n.rem_euclid(d);
    q + i128::from(r * 2 > d || r * 2 == d && q % 2 != 0)
}
fn rounded(n: i128, shift: u32, rounding: Rounding) -> i128 {
    match rounding {
        Rounding::NearestEven => rne(n, shift),
        Rounding::Floor => n >> shift,
        Rounding::TowardZero => n / (1_i128 << shift),
        Rounding::HalfUp => {
            if shift == 0 {
                n
            } else {
                (n >> shift) + ((n >> (shift - 1)) & 1)
            }
        }
    }
}
fn quantize(v: f64, f: u32) -> i128 {
    (v * (1_u64 << f) as f64).round_ties_even() as i128
}
fn rescale(raw: i128, source: u32, target: u32) -> i128 {
    rescale_with(raw, source, target, Rounding::NearestEven)
}
fn rescale_with(raw: i128, source: u32, target: u32, rounding: Rounding) -> i128 {
    if source > target {
        rounded(raw, source - target, rounding)
    } else {
        raw << (target - source)
    }
}

fn normalize(
    raw: [i128; 3],
    threshold: i128,
    bounded: bool,
    c: Config,
    prefix: &str,
    g: &mut Golden,
    threshold_length: Option<i128>,
) -> [i128; 3] {
    let f = c.direction_fraction;
    let m = raw.iter().map(|v| v.abs()).max().unwrap();
    let zero = threshold_length.map_or(m < threshold, |length| m * 512 < length);
    let raw = if zero { [1 << (f - 1), 0, 0] } else { raw };
    let m = raw.iter().map(|v| v.abs()).max().unwrap();
    let highest = 127 - m.leading_zeros() as i32;
    let mut shift = if bounded { 0 } else { f as i32 - 1 - highest };
    // Block prescaling keeps its explicit boundary at max=32767.
    // Floor removes RNE overflow, but the selected circuit still uses this bin.
    if c.block_prescale && prefix == "n" && !bounded && m == 32767 {
        shift = -2;
    }
    if !bounded
        && shift < 0
        && raw
            .iter()
            .any(|&a| rounded(a, (-shift) as u32, c.rounding.normalization).abs() >= 1 << f)
    {
        shift -= 1;
    }
    if g.trace {
        g.stage(format!("{prefix}.shift"), i128::from(shift));
    }
    let v = raw.map(|a| {
        if shift >= 0 {
            a << shift
        } else {
            rounded(a, (-shift) as u32, c.rounding.normalization)
        }
    });
    let low = f - 7;
    let q = v
        .iter()
        .map(|a| {
            let u = a.abs();
            if c.square9 && prefix != "v" {
                let magnitude_q8 = rne(u, 6);
                (magnitude_q8 * magnitude_q8) << 12
            } else if c.approximate_square
                && !c.direct_all_squares
                && !(c.direct_square && prefix == "h")
            {
                let high = u >> low;
                let tail = u % (1 << low);
                ((high * high) << (2 * low)) + (((2 * high + 1) * tail) << low)
            } else {
                u * u
            }
        })
        .sum::<i128>();
    if g.trace {
        g.stage(format!("{prefix}.q"), q);
    }
    let highest = 127 - q.leading_zeros() as i32;
    let exponent = highest - 2 * f as i32;
    if c.weighted_view && prefix == "v" {
        let mantissa = q >> (highest - 14);
        let segment = (mantissa - 16384) / 256;
        let fraction = mantissa % 256;
        let parity = exponent.rem_euclid(2);
        let endpoint =
            |i: i128| quantize(((1.0 + i as f64 / 64.0) * 2_f64.powi(parity)).sqrt(), 15);
        let base = endpoint(segment);
        let root = base
            + rounded(
                (endpoint(segment + 1) - base) * fraction,
                8,
                c.rounding.rsqrt,
            );
        let length = if exponent < 0 {
            rounded(root, 1, c.rounding.normalization)
        } else {
            root
        };
        g.stage("v.length", length);
        return v;
    }
    let r = if c.approximate_rsqrt {
        // Extract 6-bit segment plus 8-bit fraction from the normalized q.
        let mantissa = if highest >= 14 {
            q >> (highest - 14)
        } else {
            q << (14 - highest)
        };
        let tail = mantissa - 16384;
        let segment = tail / 256;
        let fraction = tail % 256;
        let parity = exponent.rem_euclid(2);
        let endpoint = |i: i128| {
            quantize(
                1.0 / ((1.0 + i as f64 / 64.0) * 2_f64.powi(parity)).sqrt(),
                c.reciprocal_fraction,
            )
        };
        let base = endpoint(segment);
        let delta = base - endpoint(segment + 1);
        let extra = c.reciprocal_work_extra;
        let r0 = (base << extra) - rounded(delta * fraction, 8 - extra, c.rounding.rsqrt);
        let restore = -exponent.div_euclid(2);
        if restore >= 0 {
            r0 << restore
        } else {
            rounded(r0, (-restore) as u32, c.rounding.normalization)
        }
    } else {
        quantize(
            1.0 / (q as f64 / 2_f64.powi((2 * f) as i32)).sqrt(),
            c.reciprocal_fraction + c.reciprocal_work_extra,
        )
    };
    if g.trace {
        g.stage(format!("{prefix}.r"), r);
    }
    if c.scalar_norm && (prefix == "h" || c.scalar_normal && prefix == "n") {
        return v;
    }
    let limit = 1_i128 << f;
    let unit = v.map(|a| {
        if zero {
            0
        } else {
            rounded(
                a * r,
                c.reciprocal_fraction + c.reciprocal_work_extra,
                c.rounding.normalization,
            )
            .clamp(-limit, limit)
        }
    });
    for (i, value) in unit.iter().enumerate() {
        if g.trace {
            g.stage(format!("{prefix}.{i}"), *value);
        }
    }
    unit
}

/// Quantized contract of the generated table, independently addressed by segments.
pub fn power_table(x: u32, code: u8) -> Result<u32, InputError> {
    power_table_with(x, code, Rounding::NearestEven)
}
pub fn power_table_with(x: u32, code: u8, rounding: Rounding) -> Result<u32, InputError> {
    if code > 16 || x > 32768 {
        return Err(InputError::Shininess);
    }
    if x == 32768 {
        return Ok(32768);
    }
    let (_, b, wide, fine, offset) = format::POWER_PARAMS[usize::from(code)];
    let (index, tail, shift) = if x < b {
        (offset + (x >> wide), x % (1 << wide), wide)
    } else {
        (
            offset + (b >> wide) + ((x - b) >> fine),
            (x - b) % (1 << fine),
            fine,
        )
    };
    let entry = format::POWER_RAW[index as usize];
    Ok((entry & 65535) as u32
        + rounded(i128::from(entry >> 16) * i128::from(tail), shift, rounding) as u32)
}

pub fn evaluate(
    pixel: PixelInput,
    material: Material,
    light: Light,
    projection: Projection,
    c: Config,
) -> Result<Golden, InputError> {
    evaluate_inner(pixel, material, light, projection, c, true)
}

/// Fast functional output using the very same oracle arithmetic. The original
/// kernel requires no stage records; scalar review variants retain their trace
/// because it is currently part of reciprocal recovery, rather than duplicating
/// the numerical expression in a browser shader.
pub fn evaluate_output(
    pixel: PixelInput,
    material: Material,
    light: Light,
    projection: Projection,
    c: Config,
) -> Result<LightingOutput, InputError> {
    let g = evaluate_inner(pixel, material, light, projection, c, c.scalar_norm)?;
    if g.intensity_fraction != 8 {
        return Err(InputError::Configuration);
    }
    Ok(LightingOutput {
        g: g.g as u16,
        h: g.h as u16,
    })
}

fn evaluate_inner(
    pixel: PixelInput,
    material: Material,
    light: Light,
    projection: Projection,
    c: Config,
    trace: bool,
) -> Result<Golden, InputError> {
    validate(pixel, material, light, projection)?;
    if c.quantization == super::super::LightingQuantization::CompensatedFloor
        && (c.direction_fraction != 14
            || c.reciprocal_fraction != 15
            || c.dot_fraction != 15
            || c.power_fraction != 15
            || c.intensity_fraction != 8
            || c.reciprocal_work_extra != 0
            || !c.approximate_square
            || !c.approximate_rsqrt
            || !c.approximate_power
            || !c.scalar_norm
            || c.scalar_normal
            || c.direct_square
            || c.direct_all_squares
            || c.square9
            || c.half_ndc_override.is_some()
            || c.normal_override.is_some()
            || c.rounding
                != (RoundingPolicy {
                    dot: Rounding::HalfUp,
                    output: Rounding::NearestEven,
                    ..RoundingPolicy::floor_all()
                }))
    {
        return Err(InputError::Configuration);
    }
    if !(10..=20).contains(&c.direction_fraction)
        || !(10..=24).contains(&c.reciprocal_fraction)
        || c.reciprocal_work_extra > 8
        || c.half_ndc_override
            .is_some_and(|p| p.iter().any(|&x| !(-16384..=16384).contains(&x)))
        || !(8..=24).contains(&c.dot_fraction)
        || !(8..=24).contains(&c.power_fraction)
        || !(4..=16).contains(&c.intensity_fraction)
    {
        return Err(InputError::Configuration);
    }
    if c.weighted_view
        && (!c.scalar_norm
            || c.scalar_normal
            || c.block_prescale
            || c.direct_square
            || c.direct_all_squares
            || c.square9
            || c.direction_fraction != 14
            || c.reciprocal_fraction != 15
            || c.reciprocal_work_extra != 0
            || !c.approximate_square
            || !c.approximate_rsqrt)
    {
        return Err(InputError::Configuration);
    }
    if c.scalar_normal && !c.scalar_norm {
        return Err(InputError::Configuration);
    }
    if c.scalar_norm
        && (c.direction_fraction != 14
            || c.reciprocal_fraction != 15
            || c.reciprocal_work_extra != 0
            || (c.rounding.normalization != Rounding::NearestEven
                && c.quantization != super::super::LightingQuantization::CompensatedFloor))
    {
        return Err(InputError::Configuration);
    }
    let mut g = Golden {
        trace,
        stages: Vec::new(),
        g: 0,
        h: 0,
        intensity_fraction: c.intensity_fraction,
    };
    let f = c.direction_fraction;
    let fi = c.intensity_fraction;
    let ia = rescale(i128::from(light.ambient), 8, fi);
    let id = rescale(i128::from(light.directional), 8, fi);
    if material.unlit {
        g.g = 1 << fi;
    } else if id == 0 {
        g.g = ia;
    } else {
        let nraw = c
            .normal_override
            .unwrap_or(pixel.normal)
            .map(|v| rescale(i128::from(v), 14, f));
        let n = normalize(nraw, rescale(4, 14, f).max(1), false, c, "n", &mut g, None);
        let l = light.direction.map(|v| rescale(i128::from(v), 14, f));
        let nl_raw = n.iter().zip(l).map(|(a, b)| a * b).sum::<i128>();
        let normal_zero = nraw.iter().map(|x| x.abs()).max().unwrap() < 4;
        let rn = if c.scalar_normal {
            g.stages.iter().find(|(name, _)| name == "n.r").unwrap().1
        } else {
            0
        };
        let nl = if c.scalar_normal {
            g.stage("nl.raw", nl_raw);
            let input_dot = nraw.iter().zip(l).map(|(a, b)| a * b).sum::<i128>();
            g.stage(
                "nl.gate",
                i128::from(
                    !normal_zero
                        && if c.exact_normal_gate {
                            input_dot > 0
                        } else {
                            nl_raw > 0
                        },
                ),
            );
            if normal_zero {
                0
            } else {
                rne(rne(nl_raw, 12) * rn, 16) << 13
            }
        } else {
            nl_raw
        };
        let positive_nl = if c.scalar_normal {
            !normal_zero
                && if c.exact_normal_gate {
                    nraw.iter().zip(l).map(|(a, b)| a * b).sum::<i128>() > 0
                } else {
                    nl_raw > 0
                }
        } else {
            nl > 0
        };
        g.stage("nl", nl);
        let d = rescale_with(nl.clamp(0, 1 << (2 * f)), 2 * f, fi, c.rounding.dot);
        g.stage("d", d);
        g.g = (ia + rounded(id * d, fi, c.rounding.output)).min((2 << fi) - 1);
        if material.specular_color != [0; 3] {
            let vraw = [
                rescale_with(
                    i128::from(c.half_ndc_override.unwrap_or(pixel.ndc)[0])
                        * i128::from(projection.ray_scale[0]),
                    28,
                    f,
                    c.rounding.projection,
                ),
                rescale_with(
                    i128::from(c.half_ndc_override.unwrap_or(pixel.ndc)[1])
                        * i128::from(projection.ray_scale[1]),
                    28,
                    f,
                    c.rounding.projection,
                ),
                rescale(i128::from(projection.k), 14, f),
            ];
            for (i, value) in vraw.iter().enumerate() {
                if g.trace {
                    g.stage(format!("ray.{i}"), *value);
                }
            }
            let v = normalize(vraw, rescale(4, 14, f).max(1), true, c, "v", &mut g, None);
            let length = if c.weighted_view {
                Some(
                    g.stages
                        .iter()
                        .find(|(name, _)| name == "v.length")
                        .unwrap()
                        .1,
                )
            } else {
                None
            };
            let hraw = std::array::from_fn(|i| {
                if let Some(length) = length {
                    let product = l[i] * length;
                    g.stage(
                        format!("weighted.light.{i}"),
                        if c.quantization == super::super::LightingQuantization::CompensatedFloor {
                            product >> 15
                        } else {
                            product
                        },
                    );
                    rounded((v[i] << 15) + product, 16, c.rounding.half)
                } else {
                    rounded(l[i] + v[i], 1, c.rounding.half)
                }
            });
            let h = normalize(hraw, rescale(64, 14, f), false, c, "h", &mut g, length);
            let nh_raw = n.iter().zip(h).map(|(a, b)| a * b).sum::<i128>();
            let nh = if c.scalar_norm {
                g.stage("nh.raw", nh_raw);
                let rh = g.stages.iter().find(|(name, _)| name == "h.r").unwrap().1;
                let first = if c.scalar_normal {
                    if normal_zero {
                        0
                    } else {
                        rne(rne(nh_raw, 13) * rn, 14)
                    }
                } else {
                    rounded(nh_raw, 12, c.rounding.normalization)
                };
                if nraw.iter().map(|x| x.abs()).max().unwrap() < 4
                    || length.map_or_else(
                        || hraw.iter().map(|x| x.abs()).max().unwrap() < 64,
                        |length| hraw.iter().map(|x| x.abs()).max().unwrap() * 512 < length,
                    )
                {
                    0
                } else {
                    rounded(first * rh, 16, c.rounding.normalization) << 13
                }
            } else {
                nh_raw
            };
            let x = rescale_with(
                nh.clamp(0, 1 << (2 * f)),
                2 * f,
                c.dot_fraction,
                c.rounding.dot,
            );
            g.stage("nh", nh);
            g.stage("x", x);
            let mut p = if c.approximate_power {
                rescale(
                    i128::from(power_table_with(
                        rescale(x, c.dot_fraction, 15) as u32,
                        material.shininess_code,
                        c.rounding.power,
                    )?),
                    15,
                    c.power_fraction,
                )
            } else {
                let s = format::POWER_PARAMS[usize::from(material.shininess_code)].0;
                quantize(
                    (x as f64 / (1_u64 << c.dot_fraction) as f64).powi(s as i32),
                    c.power_fraction,
                )
            };
            let compensated =
                c.quantization == super::super::LightingQuantization::CompensatedFloor;
            if compensated && x != 32768 {
                // Independently account for the ROM encoding; do not consume
                // POWER_MIDPOINT_RAW as a golden for the counted implementation.
                p += 64;
            }
            g.stage(
                if compensated {
                    "power.midpoint_q15"
                } else {
                    "power"
                },
                p,
            );
            let p = if positive_nl {
                rescale_with(
                    p,
                    c.power_fraction,
                    fi,
                    if compensated {
                        Rounding::Floor
                    } else {
                        c.rounding.output
                    },
                )
            } else {
                0
            };
            g.stage("p9", p);
            g.h = rounded(id * p, fi, c.rounding.output);
        }
    }
    g.stage("g", g.g);
    g.stage("h", g.h);
    Ok(g)
}

/// Ideal equations on the same quantized external inputs; no ROM approximations.
pub fn ideal(
    pixel: PixelInput,
    material: Material,
    light: Light,
    projection: Projection,
) -> Result<[f64; 2], InputError> {
    validate(pixel, material, light, projection)?;
    let ia = f64::from(light.ambient) / 256.0;
    let id = f64::from(light.directional) / 256.0;
    if material.unlit {
        return Ok([1.0, 0.0]);
    }
    if id == 0.0 {
        return Ok([ia, 0.0]);
    }
    let unit = |v: [f64; 3], threshold: f64| {
        if v.iter().map(|x| x.abs()).fold(0.0, f64::max) < threshold {
            return [0.0; 3];
        }
        let length = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| x / length)
    };
    let n = unit(pixel.normal.map(|v| f64::from(v) / 16384.0), 4.0 / 16384.0);
    let l = light.direction.map(|v| f64::from(v) / 16384.0);
    let dot = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let nl = dot(n, l);
    if material.specular_color == [0; 3] {
        return Ok([ia + id * nl.clamp(0.0, 1.0), 0.0]);
    }
    let v = unit(
        [
            f64::from(pixel.ndc[0]) / 16384.0 * f64::from(projection.ray_scale[0]) / 16384.0,
            f64::from(pixel.ndc[1]) / 16384.0 * f64::from(projection.ray_scale[1]) / 16384.0,
            f64::from(projection.k) / 16384.0,
        ],
        4.0 / 16384.0,
    );
    let h = unit(std::array::from_fn(|i| (l[i] + v[i]) * 0.5), 64.0 / 16384.0);
    let s = format::POWER_PARAMS[usize::from(material.shininess_code)].0;
    let p = if nl > 0.0 {
        dot(n, h).clamp(0.0, 1.0).powi(s as i32)
    } else {
        0.0
    };
    Ok([ia + id * nl.clamp(0.0, 1.0), id * p])
}
