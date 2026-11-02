//! Behavioral DisplayLineBuffer mirror, with separate validity/collision diagnostics.

use super::super::line_buffer::precode;
use super::Edges;

/// A registered RAM read. Collision marks cross-port uncertainty or taint
/// propagated through a lookup address; value/known retain behavioral old Q.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RamWord {
    pub value: u32,
    pub known: bool,
    pub collision: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct LineBufferInput {
    pub write_enable: bool,
    /// Nine-bit qword address, truncated as at the hardware port.
    pub write_address: u16,
    pub write_data: u64,
    pub read_address_a: u16,
    pub read_address_b: u16,
    /// Both addresses are concrete by default. False represents an RTL X index.
    pub read_address_known: [bool; 2],
    /// Diagnostic provenance of an address derived from collision-tainted Q.
    pub read_address_tainted: [bool; 2],
}

impl Default for LineBufferInput {
    fn default() -> Self {
        Self {
            write_enable: false,
            write_address: 0,
            write_data: 0,
            read_address_a: 0,
            read_address_b: 0,
            read_address_known: [true; 2],
            read_address_tainted: [false; 2],
        }
    }
}

/// Two 512x32 banks. Data/unused cells start unknown in the RTL model.
/// Only the fitted precode table cells are initialized; read Q starts at zero.
/// There is no RAM reset port: resetting Display does not erase this storage.
#[derive(Clone, Debug)]
pub struct LineBuffer {
    words: [[u32; 512]; 2],
    known: [[bool; 512]; 2],
    q: [RamWord; 2],
}

impl Default for LineBuffer {
    fn default() -> Self {
        let mut ram = Self {
            words: [[0; 512]; 2],
            known: [[false; 512]; 2],
            q: [RamWord {
                value: 0,
                known: true,
                collision: false,
            }; 2],
        };
        for address in 416..512 {
            let index = address - 416;
            let value = if address < 448 {
                precode::tmds_precode(((index << 3) | (index >> 2)) as u8)
            } else {
                precode::table_word(address - 448, &crate::display::LINEAR6_TO_SRGB8)
            };
            for bank in 0..2 {
                ram.words[bank][address] = value;
                ram.known[bank][address] = true;
            }
        }
        ram
    }
}

impl LineBuffer {
    /// Pure registered outputs; repeated observation never advances a port.
    pub fn outputs(&self) -> [RamWord; 2] {
        self.q
    }

    /// Passive diagnostic access, never a scanout bypass.
    pub fn peek(&self, bank: usize, address: u16) -> RamWord {
        let address = usize::from(address & 511);
        RamWord {
            value: self.words[bank][address],
            known: self.known[bank][address],
            collision: false,
        }
    }

    /// Commit this instant once. Pixel reads sample the old array even when a
    /// logic write is coincident. No-edge calls leave Q and storage unchanged.
    pub fn clock(&mut self, edges: Edges, input: &LineBufferInput) {
        let write_address = usize::from(input.write_address & 511);
        if edges.pixel_rise {
            for (bank, address) in [input.read_address_a, input.read_address_b]
                .into_iter()
                .enumerate()
            {
                let address = usize::from(address & 511);
                self.q[bank] = RamWord {
                    value: self.words[bank][address],
                    known: input.read_address_known[bank] && self.known[bank][address],
                    collision: input.read_address_tainted[bank]
                        || (edges.logic_rise
                            && input.write_enable
                            && (!input.read_address_known[bank] || address == write_address)),
                };
            }
        }
        if edges.logic_rise && input.write_enable {
            self.words[0][write_address] = input.write_data as u32;
            self.words[1][write_address] = (input.write_data >> 32) as u32;
            self.known[0][write_address] = true;
            self.known[1][write_address] = true;
        }
    }
}
