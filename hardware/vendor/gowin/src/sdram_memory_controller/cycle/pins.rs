//! Functional SDRAM command/return closure for the fitted native edge calendar.
//! Backing storage is borrowed, never copied or owned by this pin sequencer.
use super::controller::{Output, Sample32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InactiveBank(u8),
    ActiveBankOnRefresh,
    UndrainedRefresh,
    MissingBacking(usize),
}
/// Address/word pipeline equivalent at native capture edges, not an SDF model.
#[derive(Clone, Debug, Default)]
pub struct Pins {
    rows: [u16; 4],
    active: u8,
    reading: bool,
    writing: bool,
    bank: u8,
    column: u8,
    addresses: [Option<u32>; 2],
    pending_value: Option<u32>,
    visible_value: Option<u32>,
}
fn logical(native: u32) -> usize {
    let remaining = native & 0x7ffff;
    ((remaining & 31) | ((native >> 19 & 3) << 5) | ((remaining >> 5) << 7)) as usize
}
fn read(backing: &[u16], native: u32) -> Result<u32, Error> {
    let word = logical(native) * 2;
    let pair = backing
        .get(word..word + 2)
        .ok_or(Error::MissingBacking(word))?;
    Ok(u32::from(pair[0]) | u32::from(pair[1]) << 16)
}
impl Pins {
    /// Resolve the controller's actual pad OE, not its internal stream owner.
    pub fn sample(&self, controller: &Output) -> Sample32 {
        if controller.dq_oe_n == 0 {
            Sample32 {
                value: controller.dq_out,
                known: self
                    .visible_value
                    .map_or(u32::MAX, |v| !(v ^ controller.dq_out)),
            }
        } else {
            Sample32 {
                value: self.visible_value.unwrap_or(0),
                known: if self.visible_value.is_some() {
                    u32::MAX
                } else {
                    0
                },
            }
        }
    }
    /// Sample late SDRAM pins after the associated native falling edge.
    /// The two device address stages precede a return-value stage; native rising
    /// capture observes the first READ value four native edges after command.
    pub fn clock(&mut self, reset: bool, p: &Output, backing: &mut [u16]) -> Result<(), Error> {
        self.clock_observing(reset, p, backing).map(|_| ())
    }
    /// Returns the unmasked bytes of the actual write branch on this edge.
    /// `Some(0)` is a fully masked 32-bit slot, distinct from no write slot.
    pub fn clock_observing(
        &mut self,
        reset: bool,
        p: &Output,
        backing: &mut [u16],
    ) -> Result<Option<u8>, Error> {
        // Like the existing pin fixture, reset suppresses commands but preserves
        // memory/bank state until the controller's real initialization PRE.
        if reset || !p.cke {
            return Ok(None);
        }
        match p.command {
            3 => {
                self.active |= 1 << p.bank;
                self.rows[p.bank as usize] = p.address & 0x7ff;
            }
            2 => {
                if p.address & 0x400 != 0 || p.bank == self.bank {
                    self.reading = false;
                    self.writing = false;
                }
                if p.address & 0x400 != 0 {
                    self.active = 0;
                } else {
                    self.active &= !(1 << p.bank);
                }
            }
            1 => {
                if self.active != 0 {
                    return Err(Error::ActiveBankOnRefresh);
                }
                if self.reading || self.writing || self.addresses.iter().any(Option::is_some) {
                    return Err(Error::UndrainedRefresh);
                }
            }
            4 | 5 => {
                if self.active & (1 << p.bank) == 0 {
                    return Err(Error::InactiveBank(p.bank));
                }
                self.bank = p.bank;
                self.column = p.address as u8;
                self.reading = p.command == 5;
                self.writing = p.command == 4;
            }
            6 => {
                self.reading = false;
                self.writing = false;
            }
            _ => {}
        }
        let location = u32::from(self.bank) << 19
            | u32::from(self.rows[self.bank as usize]) << 8
            | u32::from(self.column);
        let mut written = None;
        if self.writing {
            let old = read(backing, location)?;
            let mut mask = 0u32;
            for byte in 0..4 {
                if p.dqm & (1 << byte) == 0 {
                    mask |= 255u32 << (byte * 8);
                }
            }
            let value = (old & !mask) | (p.dq_out & mask);
            let word = logical(location) * 2;
            backing[word] = value as u16;
            backing[word + 1] = (value >> 16) as u16;
            written = Some((!p.dqm & 15).count_ones() as u8);
            self.column = self.column.wrapping_add(1);
        }
        let push = if self.reading {
            self.column = self.column.wrapping_add(1);
            Some(location)
        } else {
            None
        };
        let emitted = self.addresses[1].map(|a| read(backing, a)).transpose()?;
        self.visible_value = self.pending_value;
        self.pending_value = emitted;
        self.addresses[1] = self.addresses[0];
        self.addresses[0] = push;
        Ok(written)
    }
}
