//! Synchronous source/R/G/B read calendar from the production Display pixel cone.
//! Timing and RAM are external owners; every method consumes their old snapshot.

use super::RamWord;

const ZERO: RamWord = RamWord {
    value: 0,
    known: true,
    collision: false,
};

/// One packed nine-bit TMDS precode, plus passive RAM provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Precode {
    pub value: u16,
    pub known: bool,
    pub collision: bool,
}
impl Default for Precode {
    fn default() -> Self {
        Self::from_word(ZERO, 0)
    }
}
impl Precode {
    fn from_word(word: RamWord, shift: u32) -> Self {
        Self {
            value: ((word.value >> shift) & 511) as u16,
            known: word.known,
            collision: word.collision,
        }
    }
}

/// Channel order is red, green, blue. There is one completed pair holding state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColorPrecode {
    pub channels: [Precode; 3],
}

/// Old upstream pixel-domain signals, already synchronized/row-latched by Timing.
/// These are boundary inputs, not a second timing or ownership state machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct PixelReadInput {
    pub pixel_reset: bool,
    pub h_count: u16,
    pub v_count: u16,
    pub scan_enabled: bool,
    pub scan_line_ready: bool,
    pub scan_valid: bool,
    pub scan_linear: bool,
    pub slot: bool,
    pub second: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelReadOutput {
    pub read_address: [u16; 2],
    pub read_address_known: [bool; 2],
    pub read_address_tainted: [bool; 2],
    pub conversion_active: bool,
    pub issue_pair: bool,
    pub emit_pair: bool,
    pub selected: ColorPrecode,
    pub phase: u8,
    pub fetch_pair: u8,
    pub pair_bank: bool,
    pub raw_pair: RamWord,
    pub red: [Precode; 2],
    pub green: [Precode; 2],
    pub completed_pair: [ColorPrecode; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct PixelRead {
    phase: u8,
    fetch_pair: u8,
    pair_bank: bool,
    raw_pair: RamWord,
    red: [Precode; 2],
    green: [Precode; 2],
    completed_pair: [ColorPrecode; 2],
}
impl Default for PixelRead {
    fn default() -> Self {
        Self {
            phase: 0,
            fetch_pair: 0,
            pair_bank: false,
            raw_pair: ZERO,
            red: [Precode::default(); 2],
            green: [Precode::default(); 2],
            completed_pair: [ColorPrecode::default(); 2],
        }
    }
}
impl PixelRead {
    /// Pure combinational ports from old registered RAM Q and old timing.
    pub fn outputs(&self, input: &PixelReadInput, q: [RamWord; 2]) -> PixelReadOutput {
        let c = crate::display::ACTIVE_DISPLAY_CONFIG;
        let base = u16::from(input.slot) * 200 + u16::from(input.second) * 100;
        let source = q[usize::from(self.pair_bank)];
        let mut address = [base + u16::from(self.fetch_pair >> 1); 2];
        let mut address_known = [true; 2];
        let mut address_tainted = [false; 2];
        let index_word = if self.phase == 1 {
            source
        } else {
            self.raw_pair
        };
        if self.phase != 0 {
            address_known = [index_word.known; 2];
            address_tainted = [index_word.collision; 2];
            for (lane, address) in address.iter_mut().enumerate() {
                let pixel = (index_word.value >> (lane * 16)) as u16;
                let raw = match self.phase {
                    1 => (pixel >> 11) & 31,
                    2 => (pixel >> 5) & 63,
                    _ => pixel & 31,
                };
                *address = if self.phase == 2 {
                    448 + raw
                } else if input.scan_linear {
                    448 + ((raw << 1) | (raw >> 4))
                } else {
                    416 + raw
                };
            }
        }
        let h = u32::from(input.h_count);
        let v = u32::from(input.v_count);
        let conversion_active = input.scan_enabled
            && input.scan_line_ready
            && input.scan_valid
            && v >= c.v_active_start
            && v < c.v_active_end
            && h >= c.h_active_start - 4
            && h < c.h_active_end;
        let issue_pair = conversion_active && self.phase == 0 && h < c.h_active_end - 4;
        let emit_pair = conversion_active && self.phase == 0 && h >= c.h_active_start;
        let converted = std::array::from_fn(|lane| ColorPrecode {
            channels: [
                self.red[lane],
                self.green[lane],
                Precode::from_word(q[lane], 0),
            ],
        });
        let current = if emit_pair {
            converted
        } else {
            self.completed_pair
        };
        PixelReadOutput {
            read_address: address,
            read_address_known: address_known,
            read_address_tainted: address_tainted,
            conversion_active,
            issue_pair,
            emit_pair,
            selected: current[usize::from(self.phase & 2 != 0)],
            phase: self.phase,
            fetch_pair: self.fetch_pair,
            pair_bank: self.pair_bank,
            raw_pair: self.raw_pair,
            red: self.red,
            green: self.green,
            completed_pair: self.completed_pair,
        }
    }

    /// Commit one pixel rise. RAM must commit separately from the same old Q.
    pub fn clock(&mut self, input: &PixelReadInput, q: [RamWord; 2]) {
        if input.pixel_reset || input.h_count == 0 {
            // Production reset retains bank/raw/R/G/completed-pair registers.
            self.phase = 0;
            self.fetch_pair = 0;
            return;
        }
        let old = self.outputs(input, q);
        if !old.conversion_active {
            return;
        }
        if old.issue_pair {
            self.pair_bank = self.fetch_pair & 1 != 0;
            self.fetch_pair = self.fetch_pair.wrapping_add(1);
        }
        if u32::from(input.h_count) < crate::display::ACTIVE_DISPLAY_CONFIG.h_active_end - 4 {
            match self.phase {
                1 => self.raw_pair = q[usize::from(self.pair_bank)],
                2 => self.red = q.map(|word| Precode::from_word(word, 0)),
                3 => {
                    self.green = q
                        .map(|word| Precode::from_word(word, if input.scan_linear { 0 } else { 9 }))
                }
                _ => {}
            }
        }
        if old.emit_pair {
            self.completed_pair = std::array::from_fn(|lane| ColorPrecode {
                channels: [
                    self.red[lane],
                    self.green[lane],
                    Precode::from_word(q[lane], 0),
                ],
            });
        }
        self.phase = (self.phase + 1) & 3;
    }
}
