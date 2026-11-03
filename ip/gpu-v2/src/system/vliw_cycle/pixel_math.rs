//! Selected two-credit Final and scalar TDP ROP, including their shared DSP Q.
//! All methods consume old registers. Unreset arithmetic and held RAM loans
//! retain unknown bits; there is no functional pixel calculation scheduled later.
use super::{require, vertex::Bits, ContractError};

fn unary(a: Bits, width: u32, out: u32, f: impl FnOnce(u64) -> u64) -> Bits {
    if a.is_known(width) {
        Bits::new(f(a.value), out)
    } else {
        Bits::default()
    }
}
fn binary(a: Bits, aw: u32, b: Bits, bw: u32, out: u32, f: impl FnOnce(u64, u64) -> u64) -> Bits {
    if a.is_known(aw) && b.is_known(bw) {
        Bits::new(f(a.value, b.value), out)
    } else {
        Bits::default()
    }
}
fn bit_and(a: Bits, b: Bits) -> Bits {
    if a.known & 1 != 0 && a.value & 1 == 0 || b.known & 1 != 0 && b.value & 1 == 0 {
        Bits::new(0, 1)
    } else {
        binary(a, 1, b, 1, 1, |x, y| x & y)
    }
}
pub(super) fn select(bit: Bits, yes: Bits, no: Bits, width: u32) -> Bits {
    if bit.known & 1 != 0 {
        if bit.value & 1 != 0 {
            yes
        } else {
            no
        }
    } else {
        let known = yes.known & no.known & !(yes.value ^ no.value) & ((1u64 << width) - 1);
        Bits {
            value: yes.value & known,
            known,
        }
    }
}
fn channels(v: [Bits; 3], width: u32) -> Bits {
    Bits::concat(v[2], Bits::concat(v[1], v[0], width), width * 2)
}

/// The same three physical registered products serve both consumers. A ROP
/// write changes the spec projection too; there are no independent saved Qs.
#[derive(Clone, Debug, Default)]
pub struct SharedBank {
    pub products: [Bits; 3],
}
#[derive(Clone, Copy, Debug, Default)]
pub struct SharedInput {
    pub rop_ce: bool,
    pub rop_a: Bits,
    pub rop_b: Bits,
    pub spec_ce: bool,
    pub spec_a: Bits,
    pub spec_b: Bits,
}
impl SharedBank {
    pub fn rop_product(&self) -> Bits {
        channels(self.products, 18)
    }
    pub fn spec_product(&self) -> Bits {
        channels(self.products.map(|p| p.slice(0, 17)), 17)
    }
    /// The primitive has no reset assignment. The parent disables both offers
    /// during reset; if both are offered, the production mux selects ROP.
    pub fn clock(&mut self, i: SharedInput) {
        if !i.rop_ce && !i.spec_ce {
            return;
        }
        for c in 0..3 {
            self.products[c] = if i.rop_ce {
                binary(i.rop_a.slice(c as u32 * 9, 9), 9, i.rop_b, 8, 18, |a, b| {
                    let signed = ((a as i64) << 55) >> 55;
                    (signed * b as i64) as u64
                })
            } else {
                binary(
                    i.spec_a,
                    9,
                    i.spec_b.slice(c as u32 * 8, 8),
                    8,
                    18,
                    |a, b| a * b,
                )
            };
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FinalInput {
    pub reset: bool,
    pub ce: bool,
    pub proposal_ce: bool,
    pub commit_allow: bool,
    pub abort: bool,
    pub shade_phase: bool,
    pub in_valid: bool,
    pub tint: Bits,
    pub texture: Bits,
    pub specular: Bits,
    pub g: Bits,
    pub h: Bits,
    pub shared_spec_product: Bits,
    pub out_ready: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct FinalOutput {
    pub fault: bool,
    pub proposal_ready: bool,
    pub in_ready: bool,
    pub advance: bool,
    pub in_accept: bool,
    pub out_valid: bool,
    pub out_fire: bool,
    pub rgb: Bits,
    pub credits: u8,
    pub queued: u8,
    pub shared_spec_ce: bool,
    pub shared_spec_a: Bits,
    pub shared_spec_b: Bits,
}
#[derive(Clone, Debug, Default)]
pub struct FinalState {
    pub stopped: bool,
    pub q_count: u8,
    pub q_head: bool,
    pub q_tail: bool,
    pub arithmetic_live: bool,
    pub results: [Bits; 2],
    pub products: [Bits; 3],
    pub spec_terms: [Bits; 3],
    pub expected_reservation: Option<u8>,
}
#[derive(Clone, Debug, Default)]
pub struct Final {
    pub state: FinalState,
}
impl Final {
    pub fn outputs(&self, i: FinalInput) -> FinalOutput {
        let s = &self.state;
        let fault = s.stopped || i.abort;
        let base = s.arithmetic_live && !i.shade_phase;
        let free = s.q_count == 0 || !s.arithmetic_live && s.q_count == 1;
        let proposal_ready = !i.shade_phase && !i.reset && !fault && free;
        let running = i.ce && !i.reset && !fault;
        let advance = running && i.commit_allow;
        let in_accept = advance && i.in_valid && proposal_ready;
        let out_valid = !i.reset && !fault && (s.q_count != 0 || base);
        let colors = std::array::from_fn(|c| {
            binary(s.products[c], 17, s.spec_terms[c], 8, 8, |p, sh| {
                ((p >> 8) + sh).min(255)
            })
        });
        FinalOutput {
            fault,
            proposal_ready,
            in_ready: running && proposal_ready,
            advance,
            in_accept,
            out_valid,
            out_fire: advance && i.out_ready && out_valid,
            rgb: if s.q_count != 0 {
                s.results[usize::from(s.q_head)]
            } else {
                channels(colors, 8)
            },
            credits: s.q_count + u8::from(s.arithmetic_live),
            queued: s.q_count + u8::from(base),
            shared_spec_ce: advance && in_accept,
            shared_spec_a: i.h,
            shared_spec_b: i.specular,
        }
    }
    pub fn clock(&mut self, i: FinalInput) -> Result<(), ContractError> {
        let o = self.outputs(i);
        let s = &self.state;
        let base = s.arithmetic_live && !i.shade_phase;
        let spec = s.arithmetic_live && i.shade_phase;
        let push = o.advance && base && !(s.q_count == 0 && o.out_fire);
        let pop = o.out_fire && s.q_count != 0;
        if !i.reset {
            let check = |ok, rule| require(ok, "final", rule);
            check(
                s.expected_reservation.is_none_or(|v| v == o.credits),
                "Final reservation conservation",
            )?;
            check(o.credits <= 2, "Final old reservation exceeds two slots")?;
            check(
                !(base && s.q_count == 2),
                "Final BASE has no reserved result slot",
            )?;
            check(!(push && s.q_count == 2), "Final result queue overflow")?;
            check(!(pop && s.q_count == 0), "Final result queue underflow")?;
            let credit_bad = s.q_count == 3 || s.arithmetic_live && s.q_count == 2;
            let h_bad = o.proposal_ready && i.in_valid && i.h.is_known(9) && i.h.value & 511 > 256;
            check(
                !(i.proposal_ce && !o.fault && (credit_bad || h_bad)),
                "GPU contract: preflight_bad",
            )?;
        }
        let mut n = s.clone();
        if i.reset {
            n.stopped = false;
            n.arithmetic_live = false;
            n.q_count = 0;
            n.q_head = false;
            n.q_tail = false;
            n.expected_reservation = None;
        } else {
            n.stopped |= i.abort;
            n.expected_reservation = Some(o.credits + u8::from(o.in_accept) - u8::from(o.out_fire));
            if o.advance {
                if !i.shade_phase {
                    n.arithmetic_live = o.in_accept;
                }
                if push {
                    n.results[usize::from(s.q_tail)] = channels(
                        std::array::from_fn(|c| {
                            binary(s.products[c], 17, s.spec_terms[c], 8, 8, |p, sh| {
                                ((p >> 8) + sh).min(255)
                            })
                        }),
                        8,
                    );
                    n.q_tail = !s.q_tail;
                }
                if pop {
                    n.q_head = !s.q_head;
                }
                n.q_count = (s.q_count + u8::from(push)) - u8::from(pop);
                if spec {
                    n.spec_terms =
                        std::array::from_fn(|c| i.shared_spec_product.slice(c as u32 * 17 + 8, 8));
                }
            }
        }
        if o.advance && (o.in_accept || spec) {
            for c in 0..3 {
                n.products[c] = if spec {
                    let folded = unary(s.products[c], 17, 8, |p| {
                        let plus128 = (p + 128) & 65535;
                        ((plus128 + (plus128 >> 8)) & 65535) >> 8
                    });
                    binary(i.g, 9, folded, 8, 17, |g, tex| g * tex)
                } else {
                    binary(
                        i.tint.slice(c as u32 * 8, 8),
                        8,
                        i.texture.slice(c as u32 * 8, 8),
                        8,
                        17,
                        |a, b| a * b,
                    )
                };
            }
        }
        self.state = n;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RopInput {
    pub reset: bool,
    pub ce: bool,
    pub shade_phase: bool,
    pub valid: bool,
    pub blend: bool,
    pub depth_func: u8,
    pub depth_write: bool,
    pub covered: Bits,
    pub old_color: Bits,
    pub old_depth: Bits,
    pub rgb: Bits,
    pub alpha: Bits,
    pub depth: Bits,
    pub shared_rop_product: Bits,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct RopOutput {
    pub ready: bool,
    pub valid: bool,
    pub color: Bits,
    pub depth: Bits,
    pub color_written: Bits,
    pub depth_written: Bits,
    pub shared_mul_pending: bool,
    pub shared_rop_ce: bool,
    pub shared_rop_a: Bits,
    pub shared_rop_b: Bits,
}
#[derive(Clone, Debug, Default)]
pub struct RopState {
    pub v1: bool,
    pub v2: bool,
    pub v3: bool,
    pub output_valid: bool,
    pub sd1: Bits,
    pub sd3: Bits,
    pub oc1: Bits,
    pub source1: [Bits; 3],
    pub alpha1: Bits,
    pub covered1: Bits,
    pub dw1: Bits,
    pub blend1: Bits,
    pub func1: Bits,
    pub gate2: Bits,
    pub dw2: Bits,
    pub destination2: [Bits; 3],
    pub alpha2: Bits,
    pub delta2: [Bits; 3],
    pub color: Bits,
    pub color_written: Bits,
    pub depth_written: Bits,
}
#[derive(Clone, Debug, Default)]
pub struct Rop {
    pub state: RopState,
}
fn depth_pass(function: Bits, source: Bits, old: Bits) -> Bits {
    // An unknown selector reaches the source function's ordinary case default.
    if !function.is_known(3) {
        return Bits::new(1, 1);
    }
    let f = function.value as u8;
    if f & 7 == 0 {
        return Bits::new(0, 1);
    }
    if f & 7 == 7 {
        return Bits::new(1, 1);
    }
    binary(source, 16, old, 16, 1, |s, o| {
        u64::from(match f & 7 {
            1 => s < o,
            2 => s == o,
            3 => s <= o,
            4 => s > o,
            5 => s != o,
            _ => s >= o,
        })
    })
}
fn expand(color: Bits) -> [Bits; 3] {
    [
        unary(color.slice(11, 5), 5, 8, |v| (v << 3) | (v >> 2)),
        unary(color.slice(5, 6), 6, 8, |v| (v << 2) | (v >> 4)),
        unary(color.slice(0, 5), 5, 8, |v| (v << 3) | (v >> 2)),
    ]
}
fn q5(v: u64) -> u64 {
    let h = v >> 3;
    let l = v & 7;
    let down = l == 0 && h >= 19 || l == 1 && h >= 23 || l == 2 && h >= 28;
    let up = l == 5 && h <= 3 || l == 6 && h <= 8 || l == 7 && h <= 12;
    h + u64::from(up) - u64::from(down)
}
fn q6(v: u64) -> u64 {
    let h = v >> 2;
    let l = v & 3;
    h + u64::from(l == 3 && h <= 20) - u64::from(l == 0 && h >= 43)
}
impl Rop {
    pub fn outputs(&self, i: RopInput) -> RopOutput {
        let s = &self.state;
        RopOutput {
            ready: i.ce,
            valid: s.output_valid,
            color: s.color,
            depth: s.sd3,
            color_written: s.color_written,
            depth_written: s.depth_written,
            shared_mul_pending: s.v2,
            shared_rop_ce: i.ce && s.v2,
            shared_rop_a: channels(s.delta2, 9),
            shared_rop_b: s.alpha2,
        }
    }
    pub fn clock(&mut self, i: RopInput) {
        let s = &self.state;
        let mut n = s.clone();
        if i.reset {
            n.v1 = false;
            n.v2 = false;
            n.v3 = false;
            n.output_valid = false;
            n.color = Bits::new(0, 16);
            n.color_written = Bits::new(0, 1);
            n.depth_written = Bits::new(0, 1);
        } else if i.ce {
            n.v1 = i.valid;
            if i.valid {
                n.sd1 = i.depth;
                n.oc1 = i.old_color;
            }
            n.source1 = std::array::from_fn(|c| i.rgb.slice(c as u32 * 8, 8));
            n.alpha1 = i.alpha;
            n.covered1 = i.covered;
            n.dw1 = Bits::new(u64::from(i.depth_write), 1);
            n.blend1 = Bits::new(u64::from(i.blend), 1);
            n.func1 = Bits::new(i.depth_func.into(), 3);
            n.v2 = s.v1;
            let dest = expand(s.oc1);
            if !i.shade_phase {
                n.gate2 = bit_and(s.covered1, depth_pass(s.func1, s.sd1, i.old_depth));
                n.dw2 = s.dw1;
                n.destination2 = dest;
            }
            n.alpha2 = Bits::concat(
                Bits::new(0, 1),
                select(
                    s.blend1,
                    unary(s.alpha1, 8, 7, |a| (a + 2) >> 2),
                    Bits::new(64, 7),
                    7,
                ),
                7,
            );
            n.delta2 =
                std::array::from_fn(|c| binary(s.source1[c], 8, dest[c], 8, 9, u64::wrapping_sub));
            n.v3 = s.v2;
            if i.shade_phase {
                n.sd3 = s.sd1;
            }
            n.output_valid = s.v3;
            n.color_written = s.gate2;
            n.depth_written = bit_and(s.gate2, s.dw2);
            let rgb: [Bits; 3] = std::array::from_fn(|c| {
                binary(
                    s.destination2[c],
                    8,
                    i.shared_rop_product.slice(c as u32 * 18, 14),
                    14,
                    8,
                    |d, delta| (((d << 6) + delta + 32) & 16383) >> 6,
                )
            });
            n.color = Bits::concat(
                unary(rgb[0], 8, 5, q5),
                Bits::concat(unary(rgb[1], 8, 6, q6), unary(rgb[2], 8, 5, q5), 5),
                11,
            );
        }
        self.state = n;
    }
}
