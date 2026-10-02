//! Functional SDRAM device at the pin boundary, independent of host Service.
use super::native::{mapped, Controller};
use crate::sdram_memory_controller::ports::OracleImage;
pub struct Pins {
    base: u64,
    bytes: Vec<u8>,
    rows: [Option<u16>; 4],
    reading: bool,
    writing: bool,
    bank: usize,
    column: u8,
    queue: Option<u32>,
    pub refreshes: u64,
}
impl Pins {
    pub fn new(image: OracleImage) -> Self {
        Self {
            base: image.base(),
            bytes: image.bytes().to_vec(),
            rows: [None; 4],
            reading: false,
            writing: false,
            bank: 0,
            column: 0,
            queue: None,
            refreshes: 0,
        }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn contains(&self, address: u64, bytes: usize) -> bool {
        address.checked_sub(self.base).is_some_and(|o| {
            o.checked_add(bytes as u64)
                .is_some_and(|e| e <= self.bytes.len() as u64)
        })
    }
    fn logical(native: u32) -> u64 {
        (((native >> 19) << 5) | (native & 31) | ((native & 0x7ffff) >> 5 << 7)) as u64 * 4
    }
    fn read(&self, native: u32) -> u32 {
        let address = Self::logical(native);
        if !self.contains(address, 4) {
            return 0xbad0bad0;
        }
        let o = (address - self.base) as usize;
        u32::from_le_bytes(self.bytes[o..o + 4].try_into().unwrap())
    }
    /// One SDRAM rising edge. Return data scheduled after tAC; the engine
    /// delays its visibility until the next capture phase, rather than feeding
    /// an oracle value directly into the controller's output registers.
    pub fn edge(&mut self, c: &Controller, reset: bool) -> Result<Option<u32>, String> {
        if reset || !c.cke {
            return Ok(None);
        }
        match c.command {
            3 => {
                if self.rows[c.pin_bank].is_some() {
                    return Err("ACT of open pin bank".into());
                }
                self.rows[c.pin_bank] = Some(c.pin_address);
            }
            2 => {
                for b in 0..4 {
                    if c.pin_address & 1024 != 0 || b == c.pin_bank {
                        self.rows[b] = None;
                    }
                }
                if c.pin_address & 1024 != 0 || self.bank == c.pin_bank {
                    self.reading = false;
                    self.writing = false;
                }
            }
            1 => {
                if self.rows.iter().any(Option::is_some)
                    || self.reading
                    || self.writing
                    || self.queue.is_some()
                {
                    return Err("REF before pin drain".into());
                }
                self.refreshes += 1;
            }
            4 | 5 => {
                if self.rows[c.pin_bank].is_none() {
                    return Err("column without ACT".into());
                }
                self.bank = c.pin_bank;
                self.column = c.pin_address as u8;
                self.reading = c.command == 5;
                self.writing = c.command == 4;
            }
            6 => {
                self.reading = false;
                self.writing = false;
            }
            _ => {}
        }
        let location = (self.bank as u32) << 19
            | (u32::from(self.rows[self.bank].unwrap_or(0)) << 8)
            | u32::from(self.column);
        if self.writing {
            if !c.drive {
                return Err("un-driven write DQ".into());
            }
            let address = Self::logical(location);
            if !self.contains(address, 4) {
                return Err("write outside pin image".into());
            }
            let o = (address - self.base) as usize;
            for (b, value) in c.dq.to_le_bytes().iter().enumerate() {
                if c.dqm & (1 << b) == 0 {
                    self.bytes[o + b] = *value;
                }
            }
            self.column = self.column.wrapping_add(1);
        }
        let returned = self.queue.map(|a| self.read(a));
        self.queue = if self.reading {
            self.column = self.column.wrapping_add(1);
            Some(location)
        } else {
            None
        };
        Ok(returned)
    }
    pub fn native_word(address: u64) -> u32 {
        mapped((address / 4) as u32)
    }
}
