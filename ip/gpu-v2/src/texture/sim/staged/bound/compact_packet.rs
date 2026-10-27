//! Actual Compact77 E0 -> canonical Group72 E1, preserving stage ages and CE.
use super::compact_transport::Member;
#[derive(Clone, Copy, Debug)]
pub(super) struct Input {
    pub member: Member,
    pub tap: u8,
}
pub(super) struct Pipeline {
    short: bool,
    input: Option<u128>,
    words: [Option<i128>; 8],
    fault: bool,
}
impl Pipeline {
    pub(super) fn with_short_alignment(short: bool) -> Self {
        Self {
            short,
            input: None,
            words: [None; 8],
            fault: false,
        }
    }
    pub(super) fn output(&self) -> Option<i128> {
        self.words[if self.short { 0 } else { 7 }]
    }
    pub(super) fn inflight(&self) -> usize {
        usize::from(self.input.is_some()) + self.words.iter().filter(|v| v.is_some()).count()
    }
    pub(super) fn owner_read(&self) -> bool {
        self.short && self.input.is_some()
    }
    pub(super) fn tick(&mut self, ce: bool, input: Option<Input>) -> Result<Option<i128>, String> {
        if self.fault {
            return Err("compact packet terminal".into());
        }
        if !ce {
            return Ok(None);
        }
        let input = input
            .map(|i| {
                if i.tap > 3 || i.member.emit() >> i.tap & 1 == 0 {
                    return Err("compact packet tap".into());
                }
                let m = i.member.bits();
                Ok((m & ((1u128 << 36) - 1)) | ((m >> 37) << 36) | (u128::from(i.tap) << 75))
            })
            .transpose();
        let input = match input {
            Ok(v) => v,
            Err(e) => {
                self.fault = true;
                return Err(e);
            }
        };
        let old = self.output();
        let packed = self.input.map(|w| {
            let f = |b: u32, n: u32| (w >> b) & ((1u128 << n) - 1);
            let tap = f(75, 2);
            let x = tap & 1;
            let y = tap >> 1;
            let level = f(63, 4);
            let same = [level <= 3 || f(53, 3) != 7, level <= 3 || f(56, 3) != 7];
            let mask = if level <= 3 {
                0
            } else {
                (1u128 << (level - 3)) - 1
            };
            let tx = (f(39, 7) + u128::from(x != 0 && !same[0])) & mask;
            let ty = (f(46, 7) + u128::from(y != 0 && !same[1])) & mask;
            let mut word = f(59, 4)
                | (level << 4)
                | (tx << 8)
                | (ty << 15)
                | (f(53, 3) << 22)
                | (f(56, 3) << 25);
            for i in 0..4 {
                let selected = (i & 1 == x || same[0]) && (i >> 1 == y || same[1]);
                word |= (u128::from(selected) * f((9 * i) as u32, 9)) << (28 + 9 * i);
            }
            let higher = (1..4).any(|i| i > tap && f((35 + i) as u32, 1) != 0);
            word |= u128::from(f(73, 1) != 0 && tap == 0) << 64;
            word |= u128::from(f(74, 1) != 0 && !higher) << 65;
            word |= f(67, 4) << 66;
            word |= f(71, 2) << 70;
            word as i128
        });
        if !self.short {
            for i in (1..8).rev() {
                self.words[i] = self.words[i - 1];
            }
        }
        self.words[0] = packed;
        self.input = input;
        Ok(old)
    }
    #[cfg(test)]
    pub(super) fn banks(&self) -> [Option<u128>; 9] {
        let mut r = [None; 9];
        r[0] = self.input;
        if self.short {
            r[2] = self.words[0].map(|v| v as u128);
        } else {
            for i in 0..8 {
                r[i + 1] = self.words[i].map(|v| v as u128);
            }
        }
        r
    }
}
