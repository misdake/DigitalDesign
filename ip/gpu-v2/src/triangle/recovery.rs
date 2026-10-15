//! Explicit attribute-code recovery leaf. No fields/inverse/raster or UV transport/saturation.
use super::{basecolor_threefield::PreparedBasis, basecolor_uv12::SplitWeight};
use crate::material_color::ColorSource as BaseColorSource;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    pub uv_bits: u8,
    pub max_weight_exp: u8,
    pub exact_uv: bool,
    pub token_bits: u8,
}
impl Default for Params {
    fn default() -> Self {
        Self {
            uv_bits: 12,
            max_weight_exp: 3,
            exact_uv: true,
            token_bits: 7,
        }
    }
}
impl Params {
    pub fn width(self) -> u8 {
        // |a0 + d1*b1 + d2*b2| <= 4095 + 8192*2^E < 2^(14+E).
        // One sign bit plus this integer domain and UV_BITS fractional bits.
        15 + self.uv_bits + self.max_weight_exp
    }
    pub fn exponent_bits(self) -> u8 {
        (8 - self.max_weight_exp.leading_zeros() as u8).max(1)
    }
    pub fn token_mask(self) -> u16 {
        ((1u32 << self.token_bits) - 1) as u16
    }
    pub fn validate(self) {
        assert!((9..=12).contains(&self.uv_bits) && self.max_weight_exp <= 3);
        assert!((1..=16).contains(&self.token_bits));
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Delta {
    pub magnitude: u16,
    pub negative: bool,
    pub exponent: u8,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Descriptor {
    pub texture: bool,
    pub base: [i16; 6],
    pub delta: [[Delta; 6]; 2],
    pub active: [bool; 6],
}
impl Descriptor {
    pub fn valid(self, p: Params) -> bool {
        for g in 0..6 {
            if g < 3 {
                if !(-2048..=2047).contains(&self.base[g]) {
                    return false;
                }
            } else if !(0..=if self.texture { 4095 } else { 255 }).contains(&self.base[g]) {
                return false;
            }
            if self.texture && g == 5 && (self.base[g] != 0 || self.active[g]) {
                return false;
            }
            if !self.active[g] {
                continue;
            }
            for d in self.delta.map(|v| v[g]) {
                if g >= 3 && self.texture && p.exact_uv {
                    if d.magnitude > 4095 || d.exponent != 0 {
                        return false;
                    }
                } else if g >= 3 && !self.texture {
                    if d.magnitude > 255 || d.exponent != 0 {
                        return false;
                    }
                } else if d.magnitude > 511
                    || d.exponent > 4
                    || (u32::from(d.magnitude) << d.exponent) > 4096
                {
                    return false;
                }
            }
        }
        true
    }
}
/// Physical-lane preparation is once per source; normal never moves in the leaf.
pub fn prepare(b: &PreparedBasis, p: Params) -> Descriptor {
    p.validate();
    let texture = b.mode == BaseColorSource::Texture;
    let mut d = Descriptor {
        texture,
        ..Default::default()
    };
    for g in 0..6 {
        if texture && g == 5 {
            continue;
        }
        let k = if g < 3 {
            g + if texture { 2 } else { 3 }
        } else {
            g - 3
        };
        d.base[g] = b.source[0][k] as i16;
        d.active[g] = !b.constant[k];
        for j in 0..2 {
            d.delta[j][g] = if g >= 3 && (!texture || p.exact_uv) {
                Delta {
                    magnitude: b.delta[j][k].unsigned_abs() as u16,
                    negative: b.delta[j][k] < 0,
                    exponent: 0,
                }
            } else {
                let n = b.narrow_delta[j][k];
                Delta {
                    magnitude: n.magnitude,
                    negative: n.negative,
                    exponent: n.exponent,
                }
            };
        }
    }
    assert!(d.valid(p));
    d
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Weight {
    pub normal: u16,
    pub uv_high: u16,
    pub uv_low: u8,
    pub negative: bool,
    pub exponent: u8,
    pub normal_endpoint: bool,
    pub uv_endpoint: bool,
}
impl Weight {
    fn unsupported(self, texture: bool, p: Params) -> bool {
        self.exponent > p.max_weight_exp
            || self.normal > 511
            || self.normal_endpoint && self.normal != 0
            || texture
                && (self.uv_high > 511
                    || self.uv_low >= 1 << (p.uv_bits - 9)
                    || self.uv_endpoint && (self.uv_high != 0 || self.uv_low != 0))
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pixel {
    pub beta: [Weight; 2],
    pub token: u16,
    pub fallback: bool,
}
/// Original helper remains owned upstream. Large exponents explicitly request a wider path.
pub fn packet(beta: [f64; 3], token: u16, p: Params) -> Pixel {
    p.validate();
    assert_eq!(token & !p.token_mask(), 0, "token must fit physical port");
    let mut out = Pixel {
        token,
        ..Default::default()
    };
    for j in 0..2 {
        let normal = SplitWeight::quantize(beta[j + 1], 9).expect("original finite beta");
        let uv = SplitWeight::quantize(beta[j + 1], p.uv_bits).expect("original finite beta");
        assert_eq!(normal.exponent, uv.exponent);
        if normal.exponent > 15 {
            out.fallback = true;
            continue;
        }
        out.beta[j] = Weight {
            normal: normal.high9,
            uv_high: uv.high9,
            uv_low: uv.low3,
            negative: normal.negative,
            exponent: normal.exponent as u8,
            normal_endpoint: normal.endpoint,
            uv_endpoint: uv.endpoint,
        };
    }
    out
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs {
    pub reset: bool,
    pub ce: bool,
    pub config: Option<Descriptor>,
    pub pixel: Option<Pixel>,
    pub result_ready: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outputs {
    pub config_ready: bool,
    pub config_error: bool,
    pub pixel_ready: bool,
    pub result_valid: bool,
    pub token: u16,
    pub fallback: bool,
    pub data: [i64; 6],
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    pub phase: bool,
    pub loaded: bool,
    pub config_error: bool,
    pub descriptor: Descriptor,
    pub held: Weight,
    pub ret_valid: bool,
    pub ret_last: bool,
    pub ret_token: u16,
    pub ret_fallback: bool,
    pub ret_normal_endpoint: bool,
    pub ret_uv_endpoint: bool,
    pub ret_sign: [bool; 6],
    pub ret_shift: [u8; 6],
    pub products: [u32; 6],
    pub correction: [u32; 6],
    pub accumulator: [i64; 6],
    pub result_valid: bool,
    pub result_token: u16,
    pub result_fallback: bool,
    pub result: [i64; 6],
}
#[derive(Clone, Debug)]
pub struct Emu {
    pub params: Params,
    pub regs: Registers,
}
impl Emu {
    pub fn new(params: Params) -> Self {
        params.validate();
        Self {
            params,
            regs: Registers::default(),
        }
    }
    pub fn outputs(&self, i: &Inputs) -> Outputs {
        let r = &self.regs;
        let drained = !r.phase && !r.ret_valid && !r.result_valid;
        Outputs {
            config_ready: drained,
            config_error: r.config_error,
            pixel_ready: r.loaded
                && !r.phase
                && (!r.result_valid || i.result_ready)
                && i.config.is_none(),
            result_valid: r.result_valid,
            token: r.result_token,
            fallback: r.result_fallback,
            data: r.result,
        }
    }
    pub fn tick(&mut self, i: Inputs) -> Outputs {
        let o = self.outputs(&i);
        if i.reset {
            self.regs = Registers::default();
            return o;
        }
        if !i.ce {
            return o;
        }
        if let Some(d) = i.config.filter(|_| o.config_ready) {
            self.regs.config_error = !d.valid(self.params);
            self.regs.loaded = d.valid(self.params);
            if self.regs.loaded {
                self.regs.descriptor = d;
            }
            return o;
        }
        let r = &mut self.regs;
        let p = self.params;
        if r.result_valid && !i.result_ready {
            return o;
        }
        if r.result_valid && i.result_ready {
            r.result_valid = false;
        }
        if r.ret_valid {
            let d = r.descriptor;
            let mut magnitudes = [0u64; 6];
            if !r.ret_fallback {
                for (g, magnitude) in magnitudes.iter_mut().enumerate() {
                    if !d.active[g] {
                        continue;
                    }
                    let uv = d.texture && (g == 3 || g == 4);
                    let endpoint = if uv {
                        r.ret_uv_endpoint
                    } else {
                        r.ret_normal_endpoint
                    };
                    let product = if uv {
                        if endpoint {
                            (r.products[g] + r.correction[g]) << p.uv_bits
                        } else {
                            (r.products[g] << (p.uv_bits - 9)) + r.correction[g]
                        }
                    } else if endpoint {
                        r.products[g] << 9
                    } else {
                        r.products[g]
                    };
                    *magnitude = u64::from(product) << r.ret_shift[g];
                }
            }
            let accumulated = std::array::from_fn(|g| {
                let seed = if r.ret_last {
                    r.accumulator[g]
                } else {
                    i64::from(d.base[g]) << p.uv_bits
                };
                accumulate(
                    seed,
                    magnitudes[g],
                    d.active[g] && !r.ret_fallback && r.ret_sign[g],
                    p.width(),
                )
            });
            if r.ret_last {
                r.result = if r.ret_fallback { [0; 6] } else { accumulated };
                r.result_valid = true;
                r.result_token = r.ret_token;
                r.result_fallback = r.ret_fallback;
            } else {
                r.accumulator = accumulated;
            }
        }
        let issue = if r.phase {
            Some((r.held, r.ret_token, r.ret_fallback, true))
        } else {
            i.pixel.filter(|_| o.pixel_ready).map(|x| {
                let f = x.fallback
                    || x.beta
                        .iter()
                        .any(|w| w.unsupported(r.descriptor.texture, p));
                assert_eq!(x.token & !p.token_mask(), 0, "token must fit physical port");
                r.held = x.beta[1];
                // Unsupported exponent information is already retained in ret_fallback.
                // The owned compute word stores only the low supported exponent bits.
                r.held.exponent &= (1 << p.exponent_bits()) - 1;
                (x.beta[0], x.token, f, false)
            })
        };
        r.ret_valid = issue.is_some();
        if let Some((w, token, fallback, last)) = issue {
            let d = r.descriptor;
            let j = usize::from(last);
            r.phase = !last;
            r.ret_last = last;
            r.ret_token = token;
            r.ret_fallback = fallback;
            r.ret_normal_endpoint = w.normal_endpoint;
            r.ret_uv_endpoint = w.uv_endpoint;
            for g in 0..6 {
                let delta = d.delta[j][g];
                let uv = d.texture && (g == 3 || g == 4);
                let endpoint = if uv { w.uv_endpoint } else { w.normal_endpoint };
                r.ret_sign[g] = delta.negative ^ w.negative;
                r.ret_shift[g] = if fallback || !d.active[g] {
                    0
                } else {
                    p.uv_bits - if uv { p.uv_bits } else { 9 } + delta.exponent + w.exponent
                };
                r.correction[g] = 0;
                if !fallback && d.active[g] {
                    let low = delta.magnitude & 511;
                    let high = (delta.magnitude >> 9) as u8;
                    let operand = if endpoint {
                        1
                    } else if uv {
                        w.uv_high
                    } else {
                        w.normal
                    };
                    r.products[g] = u32::from(low) * u32::from(operand);
                    if uv {
                        r.correction[g] = if endpoint {
                            u32::from(high) << 9
                        } else {
                            (fabric(w.uv_high, high) << p.uv_bits)
                                + fabric(low, w.uv_low)
                                + (fabric(u16::from(high), w.uv_low) << 9)
                        };
                    }
                }
            }
        }
        let limit = 1i64 << (p.width() - 1);
        assert!(r
            .accumulator
            .iter()
            .chain(&r.result)
            .all(|v| (-limit..limit).contains(v)));
        o
    }
}
/// Match the actual shared seed/magnitude XOR/carry-in arithmetic at the physical width.
fn accumulate(seed: i64, magnitude: u64, subtract: bool, width: u8) -> i64 {
    let mask = (1u64 << width) - 1;
    let operand = magnitude ^ if subtract { mask } else { 0 };
    let carry_sum = (((seed as u64 & mask) << 1) | u64::from(subtract)) + ((operand << 1) | 1);
    let raw = (carry_sum >> 1) & mask;
    if raw & (1 << (width - 1)) == 0 {
        raw as i64
    } else {
        raw as i64 - (1i64 << width)
    }
}
fn fabric(a: u16, b: u8) -> u32 {
    let mut v = 0;
    for bit in 0..3 {
        if b & (1 << bit) != 0 {
            v += u32::from(a) << bit;
        }
    }
    v
}
pub const MODULE: &str = "gpu_v2_threefield_recovery";
pub fn portable_rtl(p: Params) -> String {
    p.validate();
    include_str!("rtl/recovery.v")
        .replace("__UV_BITS__", &p.uv_bits.to_string())
        .replace("__MAX_EXP__", &p.max_weight_exp.to_string())
        .replace("__EXACT_UV__", if p.exact_uv { "1" } else { "0" })
        .replace("__TOKEN_BITS__", &p.token_bits.to_string())
}
pub fn native_rtl(p: Params) -> String {
    format!(
        "`define GPU_RECOVERY_NATIVE\n{}\n`undef GPU_RECOVERY_NATIVE\n",
        portable_rtl(p)
    )
}
