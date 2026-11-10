//! Actual six-DSP, two-edge tap-pair Color and ordered Public16 owner.
//! The input cache DO is borrowed from start through the later ACK. Completed
//! pixels write the parent's final bank directly; there is no local result FIFO.
use super::{
    super::{require, vertex::Bits, ContractError},
    word::Word,
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub proposal_ce: bool,
    pub commit_allow: bool,
    pub check_quad: u8,
    pub publish_valid: bool,
    pub publish_quad: u8,
    pub publish_mask: u8,
    pub valid: bool,
    pub payload: Word,
    pub texels: [Bits; 4],
    pub rgba4444: bool,
    pub out_ready: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub public_free: bool,
    pub publish_ready: bool,
    pub publish_fire: bool,
    pub ready: bool,
    pub input_fire: bool,
    pub valid: bool,
    pub key: Bits,
    pub rgb: Bits,
    pub alpha: Bits,
    pub last: bool,
    pub result_fire: bool,
    pub advance: bool,
    pub live: u16,
    pub started: bool,
    pub accepted: bool,
    pub mul_ce: bool,
    pub accum_ce: bool,
    pub accum_add: bool,
    pub core_valid: bool,
    pub packet_bad: bool,
    pub pair_sum: [Bits; 3],
    pub packed_half: Bits,
    pub packed_low: Bits,
}
#[derive(Clone, Debug, Default)]
pub struct Color {
    hard_dsp: bool,
    pub live: u16,
    pub last_lane: [Bits; 16],
    pub busy: bool,
    pub done: bool,
    pub q_done: bool,
    pub p_key: Bits,
    pub p_last: Bits,
    pub q_key: Bits,
    pub q_last: Bits,
    pub products: [[Bits; 2]; 3],
    pub alpha_pair: Bits,
    pub accumulator: Bits,
}
fn expand(t: Bits, c: usize) -> Bits {
    if c == 1 {
        let v = t.slice(5, 6);
        Bits::concat(v, v.slice(4, 2), 2)
    } else {
        let v = t.slice(if c == 0 { 11 } else { 0 }, 5);
        Bits::concat(v, v.slice(2, 3), 3)
    }
}
impl Color {
    pub fn native() -> Self {
        Self {
            hard_dsp: true,
            // The vendor ALU54D simulation model initializes its output reg.
            // Neither profile resets this accumulator on the input reset.
            accumulator: Bits::new(0, 54),
            ..Default::default()
        }
    }
    pub fn outputs(&self, i: Input) -> Output {
        let attempted = i.proposal_ce && !i.reset;
        let advance = attempted && i.commit_allow;
        let core_ready = i.proposal_ce && self.busy;
        let accepted = i.valid && core_ready;
        let started = i.valid && i.proposal_ce && !self.busy;
        let packet_bad = accepted && i.payload.slice(4, 4).value > 10;
        let core_advance = advance && !packet_bad;
        let core_valid = self.q_done && self.q_last.value & 1 != 0;
        let valid = !i.reset && core_valid;
        let key = self.q_key;
        let quad = (key.value >> 2 & 15) as usize;
        let last = valid && self.live & (1 << quad) != 0 && key.slice(0, 2) == self.last_lane[quad];
        let publish_ready =
            !i.reset && self.live & (1 << (i.publish_quad & 15)) == 0 && i.publish_mask & 15 != 0;
        let pair_sum = self.products.map(|v| {
            if v[0].is_known(17) && v[1].is_known(17) {
                Bits::new(
                    (v[0].value >> 9)
                        + (v[1].value >> 9)
                        + (v[0].value >> 8 & 1)
                        + (v[1].value >> 8 & 1),
                    8,
                )
            } else {
                Bits::default()
            }
        });
        let mut packed_half = Bits::new(0, 54);
        for (c, v) in pair_sum.iter().enumerate() {
            packed_half.set(c as u32 * 12, 8, *v);
        }
        packed_half.set(40, 13, self.alpha_pair);
        let mut packed_low = Bits::new(0, 54);
        packed_low.set(36, 13, self.alpha_pair);
        let alpha = if !i.rgba4444 {
            Bits::new(255, 8)
        } else if self.accumulator.slice(36, 17).is_known(17) {
            Bits::new((self.accumulator.slice(36, 17).value + 255) / 511, 8)
        } else {
            Bits::default()
        };
        Output {
            alpha,
            public_free: !i.reset && self.live & (1 << (i.check_quad & 15)) == 0,
            publish_ready,
            publish_fire: advance && i.publish_valid && publish_ready,
            ready: core_ready && !i.reset,
            input_fire: advance && i.valid && core_ready,
            valid,
            key,
            rgb: Bits::concat(
                self.accumulator.slice(24, 8),
                Bits::concat(
                    self.accumulator.slice(12, 8),
                    self.accumulator.slice(0, 8),
                    8,
                ),
                16,
            ),
            last,
            result_fire: advance && valid && i.out_ready,
            advance,
            live: self.live,
            started,
            accepted,
            mul_ce: core_advance && (accepted || started),
            accum_ce: core_advance && (accepted || self.done),
            accum_add: !(accepted && i.payload.slice(64, 1).value != 0),
            core_valid,
            packet_bad,
            pair_sum,
            packed_half,
            packed_low,
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let o = self.outputs(i);
        let input_quad = (i.payload.value >> 66 & 15) as usize;
        let output_quad = (o.key.value >> 2 & 15) as usize;
        // The selected source-reserved specialization ties publish_bad low.
        // An offered publication without old credit simply does not fire.
        let input_bad = i.valid && i.proposal_ce && self.busy && self.live & (1 << input_quad) == 0;
        let result_bad = o.core_valid && i.out_ready && self.live & (1 << output_quad) == 0;
        let diagnostics = require(
            !(!i.reset && i.proposal_ce && (input_bad || result_bad || o.packet_bad)),
            "Public16 Color",
            "GPU contract: preflight_fault",
        );
        let completion = require(
            !(!i.reset && o.advance && !o.packet_bad && o.core_valid && !i.out_ready),
            "direct Color",
            "closed Color result bank did not accept completion",
        );
        if i.reset {
            self.live = 0;
            self.busy = false;
            self.done = false;
            self.q_done = false;
            return Ok(());
        }
        if o.mul_ce {
            let half = if self.busy { 2 } else { 0 };
            for c in 0..3 {
                for t in 0..2 {
                    let w = i.payload.slice(28 + 9 * (half + t) as u32, 9);
                    let tex = i.texels[half + t];
                    let col = if i.rgba4444 {
                        let n = tex.slice(12 - c as u32 * 4, 4);
                        Bits::concat(n, n, 4)
                    } else {
                        expand(tex, c)
                    };
                    self.products[c][t] = if self.hard_dsp
                        && ((w.is_known(9) && w.value == 0) || (col.is_known(8) && col.value == 0))
                    {
                        Bits::new(0, 17)
                    } else if w.is_known(9) && col.is_known(8) {
                        Bits::new(w.value * col.value, 17)
                    } else {
                        Bits::default()
                    };
                }
            }
        }
        if o.mul_ce {
            let half = if self.busy { 2 } else { 0 };
            let tap = |t: usize| {
                let w = i.payload.slice(28 + 9 * (half + t) as u32, 9);
                let a = i.texels[half + t].slice(0, 4);
                if (w.is_known(9) && w.value == 0) || (a.is_known(4) && a.value == 0) {
                    Bits::new(0, 13)
                } else if w.is_known(9) && a.is_known(4) {
                    Bits::new(w.value * a.value, 13)
                } else {
                    Bits::default()
                }
            };
            let a = tap(0);
            let b = tap(1);
            self.alpha_pair = if a.is_known(13) && b.is_known(13) {
                Bits::new(a.value + b.value, 13)
            } else {
                Bits::default()
            };
        }
        if o.accum_ce {
            self.accumulator = if o.packed_half.is_known(54)
                && o.packed_low.is_known(54)
                && (!o.accum_add || self.accumulator.is_known(54))
            {
                Bits::new(
                    o.packed_half.value
                        + o.packed_low.value
                        + if o.accum_add {
                            self.accumulator.value
                        } else {
                            0
                        },
                    54,
                )
            } else {
                Bits::default()
            };
        }
        if o.advance && !o.packet_bad {
            if self.done {
                self.q_key = self.p_key;
                self.q_last = self.p_last;
            }
            if o.accepted {
                self.p_key = Bits::concat(i.payload.slice(66, 4), i.payload.slice(70, 2), 2);
                self.p_last = i.payload.slice(65, 1);
            }
            self.q_done = self.done;
            self.done = o.accepted;
            self.busy = o.started;
        }
        if o.advance {
            if o.result_fire && o.last {
                self.live &= !(1 << output_quad);
            }
            if o.publish_fire {
                let q = usize::from(i.publish_quad & 15);
                self.live |= 1 << q;
                self.last_lane[q] = Bits::new(
                    u64::from(7 - (i.publish_mask & 15).leading_zeros() as u8),
                    2,
                );
            }
        }
        diagnostics?;
        completion
    }
}
