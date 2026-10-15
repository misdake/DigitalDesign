//! Explicit mixed-width recovery: load-decoded Delta9 normal, exact UV, independent Q9/Q12 beta.
pub use super::recovery::{
    packet, prepare, Delta, Descriptor, Inputs, Outputs, Params, Pixel, Weight,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoundDelta {
    pub magnitude: u16,
    pub negative: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoundDescriptor {
    pub texture: bool,
    pub base: [i16; 6],
    pub delta: [[BoundDelta; 6]; 2],
    pub active: [bool; 6],
}
impl BoundDescriptor {
    fn decode(d: Descriptor) -> Self {
        Self {
            texture: d.texture,
            base: d.base,
            active: d.active,
            delta: std::array::from_fn(|j| {
                std::array::from_fn(|g| {
                    if !d.active[g] {
                        BoundDelta::default()
                    } else {
                        let delta = d.delta[j][g];
                        BoundDelta {
                            magnitude: if g < 3 {
                                delta.magnitude << delta.exponent
                            } else {
                                delta.magnitude
                            },
                            negative: delta.negative,
                        }
                    }
                })
            }),
        }
    }
}
fn unsupported(w: Weight, texture: bool, p: Params) -> bool {
    w.exponent > p.max_weight_exp
        || w.normal > 511
        || w.normal_endpoint && w.normal != 0
        || texture
            && (w.uv_high > 511
                || w.uv_low >= 1 << (p.uv_bits - 9)
                || w.uv_endpoint && (w.uv_high != 0 || w.uv_low != 0))
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    pub phase: bool,
    pub loaded: bool,
    pub config_error: bool,
    pub descriptor: BoundDescriptor,
    pub held: Weight,
    pub ret_valid: bool,
    pub ret_last: bool,
    pub ret_token: u16,
    pub ret_fallback: bool,
    pub ret_normal_endpoint: bool,
    pub ret_sign: [bool; 6],
    pub ret_exp: u8,
    pub products18: [u64; 5],
    pub products9: [u32; 3],
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
        assert!(params.exact_uv, "mixed binding requires exact UV delta");
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
                self.regs.descriptor = BoundDescriptor::decode(d);
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
                    let product = if g < 3 || uv {
                        r.products18[g]
                    } else if r.ret_normal_endpoint {
                        u64::from(r.products9[g - 3]) << 9
                    } else {
                        u64::from(r.products9[g - 3])
                    };
                    *magnitude = product << (r.ret_exp + if uv { 0 } else { p.uv_bits - 9 });
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
                        .any(|w| unsupported(*w, r.descriptor.texture, p));
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
            r.ret_exp = if fallback { 0 } else { w.exponent };
            for g in 0..6 {
                let delta = d.delta[j][g];
                let uv = d.texture && (g == 3 || g == 4);
                r.ret_sign[g] = delta.negative ^ w.negative;
                if !fallback && d.active[g] {
                    if g < 3 {
                        let operand = if w.normal_endpoint { 512 } else { w.normal };
                        r.products18[g] = u64::from(delta.magnitude) * u64::from(operand);
                    } else if uv {
                        let operand = if w.uv_endpoint {
                            1u16 << p.uv_bits
                        } else {
                            (w.uv_high << (p.uv_bits - 9)) | u16::from(w.uv_low)
                        };
                        r.products18[g] = u64::from(delta.magnitude) * u64::from(operand);
                    } else {
                        // Paid endpoint delta*1, followed by exact fixed <<9 on return.
                        let operand = if w.normal_endpoint { 1 } else { w.normal };
                        r.products9[g - 3] = u32::from(delta.magnitude) * u32::from(operand);
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
pub const MODULE: &str = "gpu_v2_threefield_recovery_mixed";
pub fn portable_rtl(p: Params) -> String {
    p.validate();
    assert!(p.exact_uv);
    include_str!("rtl/recovery_mixed.v")
        .replace("__UV_BITS__", &p.uv_bits.to_string())
        .replace("__MAX_EXP__", &p.max_weight_exp.to_string())
        .replace("__TOKEN_BITS__", &p.token_bits.to_string())
}
pub fn native_rtl(p: Params) -> String {
    format!(
        "`define GPU_RECOVERY_NATIVE\n{}\n`undef GPU_RECOVERY_NATIVE\n",
        portable_rtl(p)
    )
}
