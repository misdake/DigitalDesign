//! Independent host reference: ideal equations and configurable quantized stages.
//! It never calls audited arithmetic. Stage integers are the counted-model goldens.
use super::super::{format, ports::*};

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub direction_fraction: u32,
    pub reciprocal_fraction: u32,
    pub dot_fraction: u32,
    pub power_fraction: u32,
    pub intensity_fraction: u32,
    pub approximate_square: bool,
    pub approximate_rsqrt: bool,
    pub approximate_power: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            direction_fraction: format::Direction::FORMAT.fraction,
            reciprocal_fraction: format::Reciprocal::FORMAT.fraction,
            dot_fraction: format::Specular::FORMAT.fraction,
            power_fraction: format::Specular::FORMAT.fraction,
            intensity_fraction: format::Intensity::FORMAT.fraction,
            approximate_square: true,
            approximate_rsqrt: true,
            approximate_power: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Golden {
    pub stages: Vec<(String, i128)>,
    pub g: i128,
    pub h: i128,
    pub intensity_fraction: u32,
}
impl Golden {
    fn stage(&mut self, name: impl Into<String>, value: i128) -> i128 {
        self.stages.push((name.into(), value));
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
fn quantize(v: f64, f: u32) -> i128 {
    (v * (1_u64 << f) as f64).round_ties_even() as i128
}
fn rescale(raw: i128, source: u32, target: u32) -> i128 {
    if source > target {
        rne(raw, source - target)
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
) -> [i128; 3] {
    let f = c.direction_fraction;
    let m = raw.iter().map(|v| v.abs()).max().unwrap();
    let zero = m < threshold;
    let raw = if zero { [1 << (f - 1), 0, 0] } else { raw };
    let m = raw.iter().map(|v| v.abs()).max().unwrap();
    let highest = 127 - m.leading_zeros() as i32;
    let mut shift = if bounded { 0 } else { f as i32 - 1 - highest };
    if !bounded && shift < 0 && rne(m, (-shift) as u32) >= 1 << f {
        shift -= 1;
    }
    g.stage(format!("{prefix}.shift"), i128::from(shift));
    let v = raw.map(|a| {
        if shift >= 0 {
            a << shift
        } else {
            rne(a, (-shift) as u32)
        }
    });
    let low = f - 7;
    let q = v
        .iter()
        .map(|a| {
            let u = a.abs();
            if c.approximate_square {
                let high = u >> low;
                let tail = u % (1 << low);
                ((high * high) << (2 * low)) + (((2 * high + 1) * tail) << low)
            } else {
                u * u
            }
        })
        .sum::<i128>();
    g.stage(format!("{prefix}.q"), q);
    let highest = 127 - q.leading_zeros() as i32;
    let exponent = highest - 2 * f as i32;
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
        let r0 = base - rne(delta * fraction, 8);
        let restore = -exponent.div_euclid(2);
        if restore >= 0 {
            r0 << restore
        } else {
            rne(r0, (-restore) as u32)
        }
    } else {
        quantize(
            1.0 / (q as f64 / 2_f64.powi((2 * f) as i32)).sqrt(),
            c.reciprocal_fraction,
        )
    };
    g.stage(format!("{prefix}.r"), r);
    let limit = 1_i128 << f;
    let unit = v.map(|a| {
        if zero {
            0
        } else {
            rne(a * r, c.reciprocal_fraction).clamp(-limit, limit)
        }
    });
    for (i, value) in unit.iter().enumerate() {
        g.stage(format!("{prefix}.{i}"), *value);
    }
    unit
}

/// Quantized contract of the generated table, independently addressed by segments.
pub fn power_table(x: u32, code: u8) -> Result<u32, InputError> {
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
    Ok((entry & 65535) as u32 + rne(i128::from(entry >> 16) * i128::from(tail), shift) as u32)
}

pub fn evaluate(
    pixel: PixelInput,
    material: Material,
    light: Light,
    projection: Projection,
    c: Config,
) -> Result<Golden, InputError> {
    validate(pixel, material, light, projection)?;
    if !(10..=20).contains(&c.direction_fraction)
        || !(10..=24).contains(&c.reciprocal_fraction)
        || !(8..=24).contains(&c.dot_fraction)
        || !(8..=24).contains(&c.power_fraction)
        || !(4..=16).contains(&c.intensity_fraction)
    {
        return Err(InputError::Configuration);
    }
    let mut g = Golden {
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
        let nraw = pixel.normal.map(|v| rescale(i128::from(v), 14, f));
        let n = normalize(nraw, rescale(4, 14, f).max(1), false, c, "n", &mut g);
        let l = light.direction.map(|v| rescale(i128::from(v), 14, f));
        let nl = n.iter().zip(l).map(|(a, b)| a * b).sum::<i128>();
        g.stage("nl", nl);
        let d = rescale(nl.clamp(0, 1 << (2 * f)), 2 * f, fi);
        g.stage("d", d);
        g.g = (ia + rne(id * d, fi)).min((2 << fi) - 1);
        if material.specular_color != [0; 3] {
            let vraw = [
                rescale(
                    i128::from(pixel.ndc[0]) * i128::from(projection.ray_scale[0]),
                    30,
                    f,
                ),
                rescale(
                    i128::from(pixel.ndc[1]) * i128::from(projection.ray_scale[1]),
                    30,
                    f,
                ),
                rescale(i128::from(projection.k), 14, f),
            ];
            for (i, value) in vraw.iter().enumerate() {
                g.stage(format!("ray.{i}"), *value);
            }
            let v = normalize(vraw, rescale(4, 14, f).max(1), true, c, "v", &mut g);
            let hraw = std::array::from_fn(|i| rne(l[i] + v[i], 1));
            let h = normalize(hraw, rescale(64, 14, f), false, c, "h", &mut g);
            let nh = n.iter().zip(h).map(|(a, b)| a * b).sum::<i128>();
            let x = rescale(nh.clamp(0, 1 << (2 * f)), 2 * f, c.dot_fraction);
            g.stage("nh", nh);
            g.stage("x", x);
            let p = if c.approximate_power {
                rescale(
                    i128::from(power_table(
                        rescale(x, c.dot_fraction, 15) as u32,
                        material.shininess_code,
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
            g.stage("power", p);
            let p = if nl > 0 {
                rescale(p, c.power_fraction, fi)
            } else {
                0
            };
            g.stage("p9", p);
            g.h = rne(id * p, fi);
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
            f64::from(pixel.ndc[0]) / 65536.0 * f64::from(projection.ray_scale[0]) / 16384.0,
            f64::from(pixel.ndc[1]) / 65536.0 * f64::from(projection.ray_scale[1]) / 16384.0,
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
