//! Actual packet arithmetic. Complete94-bit external capture uses emit0 only
//! at E0; surviving E0 state93 + E1 state76 + seven72-bit banks =673 bits.
use super::transport::Member;

pub(super) const DATA_BITS: usize = 93 + 76 + 7 * 72;
pub(super) const CONTROL_BITS: usize = 9 + 1;
#[cfg(test)]
pub(super) const BANK_BITS: [usize; 9] = [93, 76, 72, 72, 72, 72, 72, 72, 72];

#[derive(Clone, Copy, Debug)]
pub(super) struct Input {
    pub member: Member,
    pub tap: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Selected {
    header: u32,       //28
    weights: [u16; 4], //36
    key: u8,           //6
    masks: [bool; 4],  //4
    first: bool,
    last: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Pipeline {
    input: Option<u128>,        //93, never a retained Member92+tap2
    selected: Option<Selected>, //76
    words: [Option<u128>; 7],   //E2..E8
    fault: bool,
}
impl Pipeline {
    pub(super) fn inflight(&self) -> usize {
        usize::from(self.input.is_some())
            + usize::from(self.selected.is_some())
            + self.words.iter().filter(|v| v.is_some()).count()
    }
    pub(super) fn output(&self) -> Option<i128> {
        self.words[6].map(|v| v as i128)
    }
    pub(super) fn tick(&mut self, ce: bool, input: Option<Input>) -> Result<Option<i128>, String> {
        if self.fault {
            return Err("packet terminal fault".into());
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
    fn advance(&mut self, input: Option<Input>) -> Result<Option<i128>, String> {
        let input = input
            .map(|v| {
                if v.tap > 3 || v.member.emit() >> v.tap & 1 == 0 {
                    return Err(String::from("packet tap is not emitted"));
                }
                // emit0 (member bit36) has its last read above. Remove that bit
                // before retaining E0; tap2 is stored at bits91..92.
                let m = v.member.bits();
                Ok((m & ((1_u128 << 36) - 1)) | ((m >> 37) << 36) | (u128::from(v.tap) << 91))
            })
            .transpose()?;
        let old_output = self.output();
        let selected = self.input.map(|word| {
            fn field(word: u128, bit: u32, width: u32) -> u128 {
                (word >> bit) & ((1_u128 << width) - 1)
            }
            let tap = field(word, 91, 2) as u8;
            let x = tap & 1;
            let y = tap >> 1;
            // All member fields above removed emit0 are shifted down one bit.
            let same = [field(word, 73, 1) != 0, field(word, 74, 1) != 0];
            let tile_x = field(word, 39 + 7 * u32::from(x), 7);
            let tile_y = field(word, 53 + 7 * u32::from(y), 7);
            let header = field(word, 75, 4)
                | field(word, 79, 4) << 4
                | tile_x << 8
                | tile_y << 15
                | field(word, 67, 3) << 22
                | field(word, 70, 3) << 25;
            let higher = (1..4).any(|i| i > tap && field(word, 35 + u32::from(i), 1) != 0);
            Selected {
                header: header as u32,
                weights: std::array::from_fn(|i| field(word, (9 * i) as u32, 9) as u16),
                key: (field(word, 83, 4) * 4 + field(word, 87, 2)) as u8,
                masks: std::array::from_fn(|i| {
                    ((i & 1) as u8 == x || same[0]) && ((i >> 1) as u8 == y || same[1])
                }),
                first: field(word, 89, 1) != 0 && tap == 0,
                last: field(word, 90, 1) != 0 && !higher,
            }
        });
        let packed = self.selected.map(|s| {
            let mut word = u128::from(s.header);
            for (i, (weight, mask)) in s.weights.into_iter().zip(s.masks).enumerate() {
                word |= u128::from(if mask { weight } else { 0 }) << (28 + 9 * i);
            }
            word | u128::from(s.first) << 64
                | u128::from(s.last) << 65
                | u128::from(s.key / 4) << 66
                | u128::from(s.key % 4) << 70
        });
        for i in (1..7).rev() {
            self.words[i] = self.words[i - 1];
        }
        self.words[0] = packed;
        self.selected = selected;
        self.input = input;
        Ok(old_output)
    }
    #[cfg(test)]
    pub(super) fn banks(&self) -> [Option<u128>; 9] {
        let selected = self.selected.map(|s| {
            let mut word = u128::from(s.header);
            for (i, w) in s.weights.into_iter().enumerate() {
                word |= u128::from(w) << (28 + 9 * i);
            }
            word |= u128::from(s.key) << 64;
            for (i, v) in s.masks.into_iter().enumerate() {
                word |= u128::from(v) << (70 + i);
            }
            word | u128::from(s.first) << 74 | u128::from(s.last) << 75
        });
        [
            self.input,
            selected,
            self.words[0],
            self.words[1],
            self.words[2],
            self.words[3],
            self.words[4],
            self.words[5],
            self.words[6],
        ]
    }
}
