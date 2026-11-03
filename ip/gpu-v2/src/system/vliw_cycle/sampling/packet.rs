//! Two enabled-edge Group35 packet formation from the held Work member.
use super::{
    super::{require, vertex::Bits, ContractError},
    word::Word,
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub valid: bool,
    pub member: Word,
    pub tap: Bits,
    pub texture_base: u16,
    pub mip: bool,
    pub framebuffer: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub ready: bool,
    pub valid: bool,
    pub packet: Bits,
}
#[derive(Clone, Debug, Default)]
pub struct Packet {
    pub capture: Bits,
    pub level_base: Bits,
    pub word: Bits,
    pub valid: u8,
}
impl Packet {
    pub fn outputs(&self, i: Input) -> Output {
        Output {
            ready: i.ce,
            valid: self.valid & 2 != 0,
            packet: self.word,
        }
    }
    fn result(&self, framebuffer: bool) -> (Bits, u16, u16) {
        if !self.capture.is_known(41) || !self.level_base.is_known(15) {
            return (Bits::default(), 0, 0);
        }
        let c = self.capture.value;
        let tap = (c >> 39 & 3) as u8;
        let x = tap & 1 != 0;
        let y = tap & 2 != 0;
        let level = (c >> 27 & 15) as u8;
        let local_x = c >> 16 & 15;
        let local_y = c >> 20 & 7;
        let sx = level <= 4 || local_x != 15;
        let sy = level <= 3 || local_y != 7;
        let xmask = if level <= 4 {
            0
        } else {
            63u64 >> 10u8.wrapping_sub(level)
        };
        let ymask = if level <= 3 {
            0
        } else {
            127u64 >> 10u8.wrapping_sub(level)
        };
        let tx = ((c >> 3 & 63) + u64::from(x && !sx)) & xmask;
        let ty = ((c >> 9 & 127) + u64::from(y && !sy)) & ymask;
        let pitch = if framebuffer {
            4
        } else {
            level.saturating_sub(4)
        };
        let macro_tile = ((tx << pitch) & 4095) | (ty >> 1 & 63);
        let half = (self.level_base.value + macro_tile * 2 + (ty & 1)) & 32767;
        let mut mask = 0u64;
        for t in 0..4 {
            if (sx || (t & 1 != 0) == x) && (sy || (t & 2 != 0) == y) {
                mask |= 1 << t;
            }
        }
        let first = c >> 37 & 1 != 0 && tap == 0;
        let last = c >> 38 & 1 != 0 && (c & 7) >> tap == 0;
        let word = half
            | (local_x << 15)
            | (local_y << 19)
            | (u64::from(first) << 22)
            | (u64::from(last) << 23)
            | (c >> 31 & 15) << 24
            | (c >> 35 & 3) << 28
            | u64::from(level & 1) << 30
            | mask << 31;
        (Bits::new(word, 35), tx as u16, ty as u16)
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let (word, tx, ty) = self.result(i.framebuffer);
        if !i.reset && i.ce {
            require(
                !(i.valid && i.texture_base & 1 != 0),
                "physical Packet",
                "Texture16x8 base must be256B aligned",
            )?;
            require(
                !(i.valid && i.member.slice(36, 4).value & (1 << i.tap.value) == 0),
                "physical Packet",
                "GPU contract: physical packet tap",
            )?;
            require(
                !(self.valid & 1 != 0 && i.framebuffer && (tx >= 25 || ty >= 30)),
                "physical Packet",
                "RTT400 Group bounds",
            )?;
        }
        if i.reset {
            self.valid = 0;
            return Ok(());
        }
        if !i.ce {
            return Ok(());
        }
        if self.valid & 1 != 0 {
            self.word = word;
        }
        if i.valid {
            self.capture = Bits {
                value: ((i.tap.value & 3) << 39)
                    | ((i.member.value >> 37) as u64 & ((1 << 39) - 1)),
                known: ((i.tap.known & 3) << 39)
                    | ((i.member.known >> 37) as u64 & ((1 << 39) - 1)),
            };
            let level = i.member.slice(64, 4).value as usize;
            let prefix = if i.mip {
                [0u16, 2, 4, 6, 8, 10, 18, 50, 178, 690, 2738]
                    .get(level)
                    .copied()
                    .unwrap_or(0)
            } else {
                0
            };
            self.level_base = Bits::new(u64::from(i.texture_base >> 1) + u64::from(prefix), 15);
        }
        self.valid = ((self.valid << 1) | u8::from(i.valid)) & 3;
        Ok(())
    }
}
