//! Four 1024x16 true dual-port BSRAMs, addressed as 1024 64-bit words.
//! Port A belongs to the microcore; port B belongs to DMA. Both ports are
//! synchronous. A mixed-port access to the same word while either writes is
//! an invalid hardware condition, not a convenient read-before-write mode.

use crate::timing::RamRead;

pub const SCRATCH_BYTES: usize = 8192;
pub const SCRATCH_WORDS: usize = SCRATCH_BYTES / 8;

#[derive(Clone, Debug)]
pub struct Scratchpad {
    banks: [[u16; SCRATCH_WORDS]; 4],
    core_output: RamRead,
    dma_output: RamRead,
}

impl Scratchpad {
    pub fn new(fill: u16) -> Self {
        Self {
            banks: [[fill; SCRATCH_WORDS]; 4],
            core_output: RamRead::Data(0),
            dma_output: RamRead::Data(0),
        }
    }

    pub fn core_output(&self) -> RamRead {
        self.core_output
    }

    pub fn dma_output(&self) -> RamRead {
        self.dma_output
    }

    pub fn inspect_word(&self, address: usize) -> u64 {
        assert!(address < SCRATCH_WORDS);
        (0..4).fold(0_u64, |word, bank| {
            word | (u64::from(self.banks[bank][address]) << (bank * 16))
        })
    }

    /// Addresses are 64-bit word indices. A and B each support read or write.
    /// A same-address read/write across ports makes that read invalid.
    pub fn tick(
        &mut self,
        core_read: Option<usize>,
        core_write: Option<(usize, u64)>,
        dma_read: Option<usize>,
        dma_write: Option<(usize, u64)>,
    ) {
        assert!(!(core_read.is_some() && core_write.is_some()));
        assert!(!(dma_read.is_some() && dma_write.is_some()));
        for address in [
            core_read,
            dma_read,
            core_write.map(|v| v.0),
            dma_write.map(|v| v.0),
        ]
        .into_iter()
        .flatten()
        {
            assert!(address < SCRATCH_WORDS);
        }
        assert!(
            !matches!((core_write, dma_write), (Some((a, _)), Some((b, _))) if a == b),
            "same-address dual write"
        );
        if let Some(address) = core_read {
            self.core_output = if dma_write.is_some_and(|(other, _)| other == address) {
                RamRead::Collision
            } else {
                RamRead::Data(self.inspect_word(address))
            };
        }
        if let Some(address) = dma_read {
            self.dma_output = if core_write.is_some_and(|(other, _)| other == address) {
                RamRead::Collision
            } else {
                RamRead::Data(self.inspect_word(address))
            };
        }
        for (address, value) in [core_write, dma_write].into_iter().flatten() {
            for bank in 0..4 {
                self.banks[bank][address] = (value >> (bank * 16)) as u16;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_banks_preserve_lanes_and_hold_outputs() {
        let mut ram = Scratchpad::new(0xa55a);
        ram.tick(None, None, None, Some((4, 0x0123_4567_89ab_cdef)));
        ram.tick(Some(4), None, None, None);
        assert_eq!(ram.core_output(), RamRead::Data(0x0123_4567_89ab_cdef));
        ram.tick(None, None, None, Some((5, 0)));
        assert_eq!(ram.core_output(), RamRead::Data(0x0123_4567_89ab_cdef));
        assert_eq!(ram.inspect_word(6), 0xa55a_a55a_a55a_a55a);
    }

    #[test]
    fn dual_port_collision_is_not_defined_data() {
        let mut ram = Scratchpad::new(0);
        ram.tick(Some(1), None, None, Some((1, 123)));
        assert_eq!(ram.core_output(), RamRead::Collision);
        ram.tick(Some(1), None, None, None);
        assert_eq!(ram.core_output(), RamRead::Data(123));
    }
}
