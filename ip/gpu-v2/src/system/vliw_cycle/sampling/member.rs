//! Two real Member registers; adjacent taps partition 16x8 physical lines.
use super::{
    super::{require, vertex::Bits, ContractError},
    word::Word,
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub valid: bool,
    pub flags: Bits,
    pub meta: Bits,
    pub coarse: bool,
    pub last_fine: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub ready: bool,
    pub valid: bool,
    pub member: Word,
}
#[derive(Clone, Debug, Default)]
pub struct Member {
    pub common: Word,
    pub member: Word,
    pub valid: u8,
}
impl Member {
    pub fn outputs(&self, i: Input) -> Output {
        Output {
            ready: i.ce,
            valid: self.valid & 2 != 0,
            member: self.member,
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        if i.reset {
            self.valid = 0;
            return Ok(());
        }
        if !i.ce {
            return Ok(());
        }
        let meta = i.meta.value;
        let level0 = meta >> 22 & 15;
        let level = if i.coarse {
            level0.saturating_sub(1)
        } else {
            level0
        };
        if i.valid {
            require(
                level <= 10 && i.flags.value & ((1 << 36) - 1) != 0,
                "texture membership",
                "GPU contract: texture membership",
            )?;
            require(
                i.flags.is_known(36) && i.meta.is_known(36),
                "texture membership",
                "accepted source must be known",
            )?;
        }
        let old = self.common;
        if self.valid & 1 != 0 {
            let v = old.value;
            let sx = (v >> 60 & 15) <= 4 || (v >> 49 & 15) != 15;
            let sy = (v >> 60 & 15) <= 3 || (v >> 53 & 7) != 7;
            let n = std::array::from_fn::<_, 4, _>(|c| v >> (9 * c) & 511 != 0);
            let emit = u8::from(n[0])
                | (u8::from(n[1] && !(n[0] && sx)) << 1)
                | (u8::from(n[2] && !((n[0] && sy) || (n[1] && sx && sy))) << 2)
                | (u8::from(n[3] && !((n[0] && sx && sy) || (n[1] && sy) || (n[2] && sx))) << 3);
            let mut member = Word::default();
            member.set(40, 36, old.slice(36, 36));
            member.set(36, 4, Bits::new(emit.into(), 4));
            member.set(0, 36, old.slice(0, 36));
            self.member = member;
        }
        if i.valid {
            let mask = if level <= 1 { 1 } else { (1u64 << level) - 1 };
            let x = if i.coarse && level0 > 1 {
                ((meta & 1023) >> 1).wrapping_sub(meta >> 20 & 1) & mask
            } else {
                meta & 1023
            };
            let y = if i.coarse && level0 > 1 {
                ((meta >> 10 & 1023) >> 1).wrapping_sub(meta >> 21 & 1) & mask
            } else {
                meta >> 10 & 1023
            };
            let value = u128::from(i.flags.value)
                | (u128::from(x >> 4 & 63) << 36)
                | (u128::from(y >> 3 & 127) << 42)
                | (u128::from(x & 15) << 49)
                | (u128::from(y & 7) << 53)
                | (u128::from(meta >> 26 & 15) << 56)
                | (u128::from(level) << 60)
                | (u128::from(meta >> 32 & 15) << 64)
                | (u128::from(meta >> 30 & 3) << 68)
                | (u128::from(!i.coarse) << 70)
                | (u128::from(i.last_fine || i.coarse) << 71);
            self.common = Word::new(value, 72);
        }
        self.valid = ((self.valid << 1) | u8::from(i.valid)) & 127;
        Ok(())
    }
}
