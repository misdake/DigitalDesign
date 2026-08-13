//! Pure address/lane mapping between G16 words and Tang Nano 20K SDRAM beats.

use super::{Word, CACHE_LINE_WORDS};

pub const SDRAM_DATA_BITS: usize = 32;
pub const SDRAM_BURST_BEATS: usize = 8;
pub const SDRAM_BANK_BITS: usize = 2;
pub const SDRAM_ROW_BITS: usize = 11;
pub const SDRAM_COLUMN_BITS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControllerAddress {
    pub bank: u8,
    pub row: u16,
    pub column: u8,
}

impl ControllerAddress {
    pub const fn packed(self) -> u32 {
        ((self.bank as u32) << (SDRAM_ROW_BITS + SDRAM_COLUMN_BITS))
            | ((self.row as u32) << SDRAM_COLUMN_BITS)
            | self.column as u32
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaskedWriteBeat {
    pub address: ControllerAddress,
    pub data: u32,
    /// Controller HS/Gowin byte mask: one disables the corresponding byte.
    pub mask: u8,
}

/// Maps the initial 128-KiB CPU window to the bottom of the fitted SDRAM.
pub const fn word_address(address: Word) -> ControllerAddress {
    let beat = (address as u32) >> 1;
    ControllerAddress {
        bank: ((beat >> (SDRAM_ROW_BITS + SDRAM_COLUMN_BITS)) & 0x3) as u8,
        row: ((beat >> SDRAM_COLUMN_BITS) & 0x7ff) as u16,
        column: (beat & 0xff) as u8,
    }
}

pub const fn line_address(address: Word) -> ControllerAddress {
    word_address(address & !((CACHE_LINE_WORDS as Word) - 1))
}

pub const fn masked_word_write(address: Word, value: Word) -> MaskedWriteBeat {
    if address & 1 == 0 {
        MaskedWriteBeat {
            address: word_address(address),
            data: value as u32,
            mask: 0b1100,
        }
    } else {
        MaskedWriteBeat {
            address: word_address(address),
            data: (value as u32) << 16,
            mask: 0b0011,
        }
    }
}

pub fn unpack_line(beats: [u32; SDRAM_BURST_BEATS]) -> [Word; CACHE_LINE_WORDS] {
    let mut words = [0; CACHE_LINE_WORDS];
    for (index, beat) in beats.into_iter().enumerate() {
        words[index * 2] = beat as Word;
        words[index * 2 + 1] = (beat >> 16) as Word;
    }
    words
}

pub fn pack_line(words: [Word; CACHE_LINE_WORDS]) -> [u32; SDRAM_BURST_BEATS] {
    std::array::from_fn(|index| {
        u32::from(words[index * 2]) | (u32::from(words[index * 2 + 1]) << 16)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_line_is_exactly_one_controller_burst() {
        assert_eq!(CACHE_LINE_WORDS, SDRAM_BURST_BEATS * 2);
        assert_eq!(line_address(0x123f).column, 0x18);
        assert_eq!(line_address(0x123f), line_address(0x1230));
    }

    #[test]
    fn halfword_stores_select_the_little_endian_byte_lanes() {
        assert_eq!(
            masked_word_write(0x20, 0xabcd),
            MaskedWriteBeat {
                address: word_address(0x20),
                data: 0x0000_abcd,
                mask: 0b1100,
            }
        );
        assert_eq!(
            masked_word_write(0x21, 0x1234),
            MaskedWriteBeat {
                address: word_address(0x20),
                data: 0x1234_0000,
                mask: 0b0011,
            }
        );
    }

    #[test]
    fn line_pack_round_trips_all_word_lanes() {
        let words = std::array::from_fn(|index| 0x1000 + index as u16);
        assert_eq!(unpack_line(pack_line(words)), words);
    }
}
