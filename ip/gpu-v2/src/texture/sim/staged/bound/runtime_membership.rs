//! Actual scalar membership registers, independent of closed counted frames.
//! E0..E6 retain648 data bits; old E6 writes Work on E7. No output FIFO.
use super::transport::{Member, MemberFields};

pub(super) const DATA_BITS: usize = 92 + 90 + 92 + 98 + 3 * 92;
pub(super) const CONTROL_BITS: usize = 7 + 1;
#[cfg(test)]
pub(super) const BANK_BITS: [usize; 7] = [92, 90, 92, 98, 92, 92, 92];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Input {
    pub weights: [u16; 4],
    /// x0,x1,y0,y1, all unsigned10.
    pub coordinates: [u16; 4],
    pub slot: u8,
    pub level: u8,
    pub key: u8,
    pub fine: bool,
    pub last_fine: bool,
}
impl Input {
    fn validate(self) -> Result<Self, String> {
        if self.weights.iter().any(|&w| w > 511)
            || self.coordinates.iter().any(|&v| v > 1023)
            || self.slot > 15
            || self.level > 15
            || self.key > 63
        {
            return Err("membership input width".into());
        }
        if self.weights == [0; 4] {
            return Err("membership inactive plane".into());
        }
        Ok(self)
    }
}
// Common86: weights36 + tiles28 + local6 + identity14 + flags2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Common {
    weights: [u16; 4],
    tiles: [u8; 4],
    local: [u8; 2],
    slot: u8,
    level: u8,
    key: u8,
    fine: bool,
    final_plane: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slice {
    common: Common,
    nonzero: [bool; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Equal {
    slice: Slice,
    same: [bool; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pairs {
    equal: Equal,
    /// 01,02,03,12,13,23, each retained conservatively as its own bit.
    pairs: [bool; 6],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Pipeline {
    short_alignment: bool,
    input: Option<Input>,
    slice: Option<Slice>,
    equal: Option<Equal>,
    pairs: Option<Pairs>,
    emit: Option<Member>,
    align: Option<Member>,
    output: Option<Member>,
    fault: bool,
}
impl Pipeline {
    pub(super) fn with_short_alignment(short_alignment: bool) -> Self {
        Self {
            short_alignment,
            ..Self::default()
        }
    }
    pub(super) fn inflight(&self) -> usize {
        [
            self.input.is_some(),
            self.slice.is_some(),
            self.equal.is_some(),
            self.pairs.is_some(),
            self.emit.is_some(),
            self.align.is_some(),
            self.output.is_some(),
        ]
        .into_iter()
        .filter(|v| *v)
        .count()
    }
    #[cfg(test)]
    pub(super) fn output(&self) -> Option<Member> {
        self.output
    }
    pub(super) fn tick(
        &mut self,
        ce: bool,
        input: Option<Input>,
    ) -> Result<Option<Member>, String> {
        if self.fault {
            return Err("membership terminal fault".into());
        }
        if !ce {
            return Ok(None);
        }
        let result = self.advance(input);
        if result.is_err() {
            self.fault = true;
        }
        result
    }
    fn advance(&mut self, input: Option<Input>) -> Result<Option<Member>, String> {
        let input = input.map(Input::validate).transpose()?;
        let old_output = if self.short_alignment {
            self.emit
        } else {
            self.output
        };
        // All right-hand sides read old registers; commits are simultaneous.
        let emit = self
            .pairs
            .map(|p| {
                let mut flags = [false; 4];
                let nz = p.equal.slice.nonzero;
                let eq = p.pairs;
                flags[0] = nz[0];
                flags[1] = nz[1] && !(nz[0] && eq[0]);
                flags[2] = nz[2] && !((nz[0] && eq[1]) || (nz[1] && eq[3]));
                flags[3] = nz[3] && !((nz[0] && eq[2]) || (nz[1] && eq[4]) || (nz[2] && eq[5]));
                let c = p.equal.slice.common;
                Member::from_fields(MemberFields {
                    weights: c.weights,
                    emit: flags,
                    tiles: c.tiles,
                    local: c.local,
                    same: p.equal.same,
                    slot: c.slot,
                    level: c.level,
                    key: c.key,
                    fine: c.fine,
                    final_plane: c.final_plane,
                })
            })
            .transpose()?;
        let pairs = self.equal.map(|equal| {
            let [x, y] = equal.same;
            Pairs {
                equal,
                pairs: [x, y, x && y, x && y, y, x],
            }
        });
        let equal = self.slice.map(|slice| Equal {
            same: [
                slice.common.tiles[0] == slice.common.tiles[1],
                slice.common.tiles[2] == slice.common.tiles[3],
            ],
            slice,
        });
        let slice = self.input.map(|v| Slice {
            common: Common {
                weights: v.weights,
                tiles: v.coordinates.map(|c| (c >> 3) as u8),
                local: [(v.coordinates[0] & 7) as u8, (v.coordinates[2] & 7) as u8],
                slot: v.slot,
                level: v.level,
                key: v.key,
                fine: v.fine,
                final_plane: !v.fine || v.last_fine,
            },
            nonzero: v.weights.map(|w| w != 0),
        });
        self.output = if self.short_alignment {
            None
        } else {
            self.align
        };
        self.align = if self.short_alignment {
            None
        } else {
            self.emit
        };
        self.emit = emit;
        self.pairs = pairs;
        self.equal = equal;
        self.slice = slice;
        self.input = input;
        Ok(old_output)
    }
    #[cfg(test)]
    pub(super) fn banks(&self) -> [Option<u128>; 7] {
        // Diagnostic encoding of actual registers, never another retained bank.
        fn common(c: Common) -> u128 {
            let mut word = 0;
            let mut bit = 0;
            for (v, bits) in c
                .weights
                .map(|v| (u128::from(v), 9))
                .into_iter()
                .chain(c.tiles.map(|v| (u128::from(v), 7)))
                .chain(c.local.map(|v| (u128::from(v), 3)))
                .chain([
                    (c.slot.into(), 4),
                    (c.level.into(), 4),
                    (c.key.into(), 6),
                    (c.fine.into(), 1),
                    (c.final_plane.into(), 1),
                ])
            {
                word |= v << bit;
                bit += bits;
            }
            assert_eq!(bit, 86);
            word
        }
        fn slice(s: Slice) -> u128 {
            common(s.common)
                | s.nonzero
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| u128::from(v) << (86 + i))
                    .sum::<u128>()
        }
        fn equal(e: Equal) -> u128 {
            slice(e.slice) | u128::from(e.same[0]) << 90 | u128::from(e.same[1]) << 91
        }
        [
            self.input.map(|v| {
                let mut word = 0;
                for (i, w) in v.weights.into_iter().enumerate() {
                    word |= u128::from(w) << (i * 9);
                }
                for (i, c) in v.coordinates.into_iter().enumerate() {
                    word |= u128::from(c) << (36 + i * 10);
                }
                word | u128::from(v.slot) << 76
                    | u128::from(v.level) << 80
                    | u128::from(v.key) << 84
                    | u128::from(v.fine) << 90
                    | u128::from(v.last_fine) << 91
            }),
            self.slice.map(slice),
            self.equal.map(equal),
            self.pairs.map(|p| {
                equal(p.equal)
                    | p.pairs
                        .into_iter()
                        .enumerate()
                        .map(|(i, v)| u128::from(v) << (92 + i))
                        .sum::<u128>()
            }),
            self.emit.map(Member::bits),
            self.align.map(Member::bits),
            self.output.map(Member::bits),
        ]
    }
}
