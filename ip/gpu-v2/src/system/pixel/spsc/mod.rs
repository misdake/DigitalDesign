//! Single-producer/single-consumer storage with whole-entry publication.
//!
//! The producer writes fixed-size entries a row at a time. The final row advances
//! insert; no consumer observes the unpublished tail. On the final publication
//! edge an earlier, already written row may be captured into a head, visible
//! next-edge; the final write address itself is never read. Space is returned
//! only when the consumer transfers the final row, and cannot fund a producer
//! write on that same edge. There is no reservation API or per-row ready bitmap.
//!
//! Two completed head words keep successive reads in flight. Asynchronous RAM
//! captures a head on the read edge, visible for transfer on the next edge.
//! Registered RAM additionally owns its paid hardware DO return position.
//! All transfers and registers freeze with CE; reset invalidates pointers
//! and heads without clearing payload RAM. RAM technology is selected by the
//! integrating design; behavioral storage is not a fitted SSRAM/BSRAM claim.

pub mod emu;
pub mod rtl;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadTiming {
    /// SSRAM asynchronous read into a head FF on the request edge.
    Capture,
    /// Synchronous RAM hard DO captured on the request edge, then a head FF.
    Registered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub entries: usize,
    pub rows: usize,
    pub width: u32,
    pub read_timing: ReadTiming,
    pub max_wall: u64,
}

impl Config {
    pub fn validate(self) -> Result<(), String> {
        if !self.entries.is_power_of_two()
            || self.entries > 32
            || !(1..=16).contains(&self.rows)
            || !(1..=64).contains(&self.width)
            || self.max_wall == 0
        {
            return Err(
                "SPSC requires power-of-two 1..32 entries, 1..16 rows, 1..64 bits and a wall bound"
                    .into(),
            );
        }
        Ok(())
    }

    pub fn mask(self) -> u64 {
        u64::MAX >> (64 - self.width)
    }

    /// Logical declaration bits, before mapping RAM and control to a device.
    pub fn payload_bits(self) -> usize {
        self.entries * self.rows * self.width as usize
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub reset: bool,
    pub ce: bool,
    pub input: Option<u64>,
    pub output_ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word {
    pub data: u64,
    pub row: usize,
    pub last: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signals {
    pub input_ready: bool,
    pub output: Option<Word>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub signals: Signals,
    pub accepted: bool,
    pub published: bool,
    pub consumed: bool,
    pub write_address: Option<usize>,
    pub read_address: Option<usize>,
    pub returned: bool,
    pub occupied_entries: usize,
}
