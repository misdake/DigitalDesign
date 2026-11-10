//! Command coefficient: three registered products, E3/E4 immutable weight writes,
//! two eight-bit mask rows and a held synchronous metadata Q. No weight shadow.
use super::super::{require, vertex::Bits, ContractError};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub proposal_ce: bool,
    pub commit_allow: bool,
    pub valid: bool,
    pub owner: Bits,
    pub result: Bits,
    pub available: u8,
    pub clamp: bool,
    pub out_ready: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub ready: bool,
    pub proposal_ready: bool,
    pub accepted: bool,
    pub valid: bool,
    pub fine: Bits,
    pub coarse: Bits,
    pub meta: Bits,
    pub reserved: u8,
    pub advance: bool,
    pub consumed: bool,
    pub publication: bool,
    pub weight_write: bool,
    pub weight_address: Bits,
    pub weight_value: Bits,
    pub metadata_read: bool,
    pub metadata_address: u8,
    pub metadata_write: bool,
}
#[derive(Clone, Debug)]
pub struct Coefficient {
    hard_dsp: bool,
    pub input_stage: Bits,
    pub coarse_clamped: u8,
    pub horizontal: Bits,
    pub row_totals: Bits,
    pub fine_mask: Bits,
    pub products: [Bits; 3],
    pub keys: [Bits; 2],
    pub metadata: [Bits; 16],
    pub meta_q: Bits,
    pub ready_masks: [Bits; 2],
    pub phase: bool,
    pub valid: u8,
    pub cwr: u8,
    pub mrd: u8,
    pub cohorts: u8,
    pub rrd: bool,
    pub rwr: bool,
    pub queued: u8,
}
impl Default for Coefficient {
    fn default() -> Self {
        Self {
            hard_dsp: false,
            input_stage: Bits::new(0, 37),
            coarse_clamped: 0,
            horizontal: Bits::new(0, 17),
            row_totals: Bits::new(0, 18),
            fine_mask: Bits::new(0, 4),
            products: [Bits::default(); 3],
            keys: [Bits::default(); 2],
            metadata: [Bits::default(); 16],
            meta_q: Bits::new(0, 36),
            ready_masks: [Bits::default(); 2],
            phase: false,
            valid: 0,
            cwr: 0,
            mrd: 0,
            cohorts: 0,
            rrd: false,
            rwr: false,
            queued: 0,
        }
    }
}
fn mul(a: Bits, b: Bits, hard_dsp: bool) -> Bits {
    // The vendor MULT9X9 model explicitly short-circuits a known zero
    // operand before multiplication. Portable Verilog `x * 0` stays X.
    if hard_dsp && ((a.is_known(9) && a.value == 0) || (b.is_known(8) && b.value == 0)) {
        Bits::new(0, 17)
    } else if a.is_known(9) && b.is_known(8) {
        Bits::new(a.value * b.value, 17)
    } else {
        Bits::default()
    }
}
fn sub(a: Bits, b: Bits) -> Bits {
    if a.is_known(9) && b.is_known(9) {
        Bits::new(a.value.wrapping_sub(b.value), 9)
    } else {
        Bits::default()
    }
}
fn reduction(a: Bits) -> Bits {
    if a.value & a.known != 0 {
        Bits::new(1, 1)
    } else if a.is_known(9) {
        Bits::new(0, 1)
    } else {
        Bits::default()
    }
}
fn flags(mask: Bits) -> Bits {
    let mut v = Bits::default();
    for c in 0..4 {
        v.set(c * 9, 9, Bits::concat(Bits::new(0, 8), mask.slice(c, 1), 1));
    }
    v
}
fn bad_input(i: Input) -> bool {
    let v = i.owner.value;
    let p0 = v & 511;
    let p1 = v >> 9 & 511;
    p0 + p1 != 511
        || (v >> 37 & 1 != 0) != (p1 == 0)
        || (v >> 18 & 1 != 0 && p0 != 511)
        || (v >> 19 & 15) > 10
        || (v >> 23 & 15) > 10
}
impl Coefficient {
    pub fn native() -> Self {
        Self {
            hard_dsp: true,
            ..Default::default()
        }
    }
    fn arithmetic(&self) -> ([Bits; 3], Bits, Bits, Bits) {
        let r = self.input_stage;
        let fine_u = r.slice(18, 8);
        let fine_v = r.slice(26, 8);
        let coarse = |f: Bits, parity: Bits, axis: u32| {
            if self.coarse_clamped & (1 << axis) != 0 {
                Bits::new(0, 8)
            } else if r.slice(36, 1).value == 0 {
                f
            } else if f.is_known(8) && parity.is_known(1) {
                let half = u64::from(f.value & 128 == 0);
                Bits::new(
                    ((parity.value ^ half) << 7) | (half << 6) | (f.value >> 1 & 63),
                    8,
                )
            } else {
                Bits::default()
            }
        };
        let cu = coarse(fine_u, r.slice(34, 1), 0);
        let cv = coarse(fine_v, r.slice(35, 1), 1);
        let parent = if self.phase {
            self.horizontal.slice(0, 9)
        } else {
            r.slice(0, 9)
        };
        let u = if self.phase {
            self.horizontal.slice(9, 8)
        } else {
            fine_u
        };
        let bottom = self.products[0].slice(8, 9);
        let top = sub(parent, bottom);
        let operands = [
            if self.phase {
                (r.slice(0, 9), fine_v)
            } else {
                (r.slice(9, 9), cv)
            },
            (bottom, u),
            (top, u),
        ];
        let mut corners = Bits::default();
        corners.set(
            0,
            9,
            sub(self.row_totals.slice(9, 9), self.products[2].slice(8, 9)),
        );
        corners.set(9, 9, self.products[2].slice(8, 9));
        corners.set(
            18,
            9,
            sub(self.row_totals.slice(0, 9), self.products[1].slice(8, 9)),
        );
        corners.set(27, 9, self.products[1].slice(8, 9));
        let mut mask = Bits::default();
        for c in 0..4 {
            mask.set(c, 1, reduction(corners.slice(c * 9, 9)));
        }
        (
            operands.map(|(a, b)| mul(a, b, self.hard_dsp)),
            Bits::concat(cu, r.slice(9, 9), 9),
            Bits::concat(top, bottom, 9),
            Bits::concat(mask, corners, 36),
        )
    }
    pub fn outputs(&self, i: Input) -> Output {
        let owner = i.owner.value;
        let cost = if i.valid {
            u8::from(owner & 511 != 0) + u8::from(owner >> 9 & 511 != 0)
        } else {
            0
        };
        let ce = i.proposal_ce && i.commit_allow;
        let advance = ce && self.queued < 2;
        let proposal_ready =
            !i.reset && self.queued < 2 && self.cohorts < 3 && !self.phase && i.available >= cost;
        let ready = advance && self.cohorts < 3 && !self.phase && i.available >= cost;
        let accepted = i.valid && ready;
        let input_fault = accepted && (bad_input(i) || i.available > 16);
        let consumed = ce && i.out_ready && self.queued != 0;
        let publication = advance && self.valid & 16 != 0;
        let v3 = self.valid & 4 != 0;
        let v4 = self.valid & 8 != 0;
        let returning = self.arithmetic().3;
        let key = self.keys[1];
        let suppress = key.slice(7, 1);
        let coarse = v4 && suppress.known & 1 != 0 && suppress.value == 0;
        let address = Bits::concat(
            key.slice(0, 6),
            Bits {
                value: key.value >> 6 & 1 ^ u64::from(coarse),
                known: key.known >> 6 & 1,
            },
            1,
        );
        let mask = self.ready_masks[usize::from(self.rrd)];
        Output {
            ready,
            proposal_ready,
            accepted,
            valid: self.queued != 0,
            fine: flags(mask.slice(0, 4)),
            coarse: flags(mask.slice(4, 4)),
            meta: self.meta_q,
            reserved: if accepted { cost } else { 0 },
            advance,
            consumed,
            publication,
            weight_write: !i.reset && advance && (v3 || coarse) && !input_fault,
            weight_address: address,
            weight_value: returning.slice(0, 36),
            metadata_read: !i.reset
                && !input_fault
                && (publication || (consumed && self.queued & 2 != 0)),
            metadata_address: self.mrd.wrapping_add(u8::from(consumed)) & 15,
            metadata_write: !i.reset && accepted && !input_fault,
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let o = self.outputs(i);
        let owner = i.owner.value;
        let n = (owner >> 19 & 15) as u8;
        let nearest = owner >> 18 & 1 != 0;
        let p0 = owner & 511;
        let p1 = owner >> 9 & 511;
        let x = i.result.value >> 16 & 1023;
        let y = i.result.value >> 26 & 1023;
        let fu = i.result.value & 255;
        let fv = i.result.value >> 8 & 255;
        if !i.reset {
            require(
                !(i.proposal_ce
                    && i.valid
                    && o.proposal_ready
                    && (bad_input(i) || i.available > 16)),
                "command coefficient",
                "GPU contract: preflight_bad",
            )?;
            require(
                !(o.metadata_write && o.metadata_read && self.cwr == o.metadata_address),
                "command coefficient",
                "GPU contract: coefficient metadata cross-port collision",
            )?;
            require(
                !o.accepted || (i.owner.is_known(38) && i.result.is_known(36)),
                "command coefficient",
                "accepted source must be known",
            )?;
        }
        let (products, horizontal, rows, returning) = self.arithmetic();
        let meta_return = self.metadata[usize::from(o.metadata_address)];
        if o.metadata_write {
            let meta = (owner >> 31 & 63) << 30
                | (owner >> 27 & 15) << 26
                | u64::from(n) << 22
                | u64::from(y & 1 == 0 && fv & 128 == 0 && !(i.clamp && y == 0)) << 21
                | u64::from(x & 1 == 0 && fu & 128 == 0 && !(i.clamp && x == 0)) << 20
                | y << 10
                | x;
            self.metadata[usize::from(self.cwr)] = Bits::new(meta, 36);
        }
        if i.reset {
            self.input_stage = Bits::new(0, 37);
            self.coarse_clamped = 0;
            self.horizontal = Bits::new(0, 17);
            self.row_totals = Bits::new(0, 18);
            self.fine_mask = Bits::new(0, 4);
            self.phase = false;
            self.valid = 0;
            self.mrd = 0;
            self.cwr = 0;
            self.cohorts = 0;
            self.rrd = false;
            self.rwr = false;
            self.queued = 0;
            self.meta_q = Bits::new(0, 36);
            return Ok(());
        }
        if o.metadata_read {
            self.meta_q = meta_return;
        }
        self.queued = self
            .queued
            .wrapping_add(u8::from(o.publication))
            .wrapping_sub(u8::from(o.consumed))
            & 3;
        self.cohorts = self
            .cohorts
            .wrapping_add(u8::from(o.accepted))
            .wrapping_sub(u8::from(o.publication))
            & 3;
        if o.consumed {
            self.rrd = !self.rrd;
            self.mrd = self.mrd.wrapping_add(1) & 15;
        }
        if o.advance {
            let v2 = self.valid & 2 != 0;
            let v3 = self.valid & 4 != 0;
            let v4 = self.valid & 8 != 0;
            if v4 {
                self.ready_masks[usize::from(self.rwr)] =
                    Bits::concat(returning.slice(36, 4), self.fine_mask, 4);
            }
            if o.publication {
                self.rwr = !self.rwr;
            }
            if v2 {
                self.horizontal = horizontal;
                self.keys[1] = self.keys[0];
            }
            if v2 || v3 {
                self.row_totals = rows;
            }
            if v3 {
                self.fine_mask = returning.slice(36, 4);
            }
            if o.accepted {
                let clipped = |base: u64, frac: u64| {
                    let mask = (1u64 << n.max(1)) - 1;
                    i.clamp
                        && !nearest
                        && (n <= 1
                            || base == 0 && frac < 128
                            || base == mask
                            || base == (mask ^ 1) && frac >= 128)
                };
                self.coarse_clamped = u8::from(clipped(x, fu)) | (u8::from(clipped(y, fv)) << 1);
                self.input_stage = Bits::new(
                    p0 | (p1 << 9)
                        | ((if nearest { 0 } else { fu }) << 18)
                        | ((if nearest { 0 } else { fv }) << 26)
                        | ((x & 1) << 34)
                        | ((y & 1) << 35)
                        | (u64::from(n > 1 && !nearest) << 36),
                    37,
                );
                self.keys[0] = Bits::new(
                    (u64::from(n == 0) << 7) | (u64::from(n & 1) << 6) | (owner >> 31 & 63),
                    8,
                );
                self.cwr = self.cwr.wrapping_add(1) & 15;
            }
            self.products = products;
            self.valid = ((self.valid << 1) | u8::from(o.accepted)) & 31;
            self.phase = !self.phase;
        }
        Ok(())
    }
}
