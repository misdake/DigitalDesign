//! Address units used at the G16 memory-system boundary.

use super::Word;

/// A 16-bit-word address in physical main memory.
///
/// G16 software normally carries only the low 16-bit offset. The selected
/// segment supplies the upper 16 bits without addition or carry propagation.
/// Individual targets validate this architectural address against their
/// fitted memory capacity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PhysicalWordAddress(u32);

impl PhysicalWordAddress {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn from_segment_offset(segment: Word, offset: Word) -> Self {
        Self(((segment as u32) << Word::BITS) | offset as u32)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub const fn byte_address(self) -> u64 {
        (self.0 as u64) << 1
    }

    pub const fn line_base(self, line_words: u32) -> Self {
        assert!(line_words.is_power_of_two());
        Self(self.0 & !(line_words - 1))
    }
}

impl From<Word> for PhysicalWordAddress {
    fn from(value: Word) -> Self {
        Self(u32::from(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_and_offset_are_concatenated_without_carry() {
        let address = PhysicalWordAddress::from_segment_offset(0x003f, 0xffff);
        assert_eq!(address.get(), 0x003f_ffff);
        assert_eq!(address.byte_address(), 0x007f_fffe);
        assert_eq!(address.line_base(16).get(), 0x003f_fff0);
    }
}
