//! Production sixteen-slot result ownership and synchronous export calendar.
use super::{check, Bits, Error};
#[derive(Clone, Debug)]
pub struct Export {
    pub produced: u8,
    pub consumed: u8,
    pub import_state: u8,
    pub result_owner: bool,
    pub response_index: u8,
    pub fetch_index: u8,
    pub valid: bool,
    pub index: u8,
    pub data: Bits,
    pub low_q: Bits,
    pub high_q: Bits,
    descriptors: [Bits; 16],
    high: [Bits; 512],
}
impl Default for Export {
    fn default() -> Self {
        Self {
            produced: 0,
            consumed: 0,
            import_state: 0,
            result_owner: false,
            response_index: 0,
            fetch_index: 0,
            valid: false,
            index: 0,
            data: Bits::new(0, 54),
            low_q: Bits::default(),
            high_q: Bits::default(),
            descriptors: [Bits::default(); 16],
            high: [Bits::default(); 512],
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct ExportComb {
    pub packet_valid: bool,
    pub packet_take: bool,
    pub word_take: bool,
    pub release: bool,
    pub room: bool,
    pub retire: bool,
    pub read: bool,
    pub address: u16,
}
#[derive(Clone, Copy)]
pub(super) struct ExportWrite {
    pub publish: bool,
    pub descriptor: Bits,
    pub coefficient: bool,
    pub address: u16,
    pub data: Bits,
}
impl Export {
    pub fn count(&self) -> u8 {
        self.produced.wrapping_sub(self.consumed) & 31
    }
    pub fn empty(&self) -> bool {
        self.produced == self.consumed
    }
    pub fn full(&self) -> bool {
        self.produced ^ self.consumed == 16
    }
    pub fn head(&self) -> Bits {
        self.descriptors[(self.consumed & 15) as usize]
    }
    pub fn high(&self, address: u16) -> Bits {
        self.high[(address & 511) as usize]
    }
    pub(super) fn comb(
        &self,
        reset: bool,
        ce: bool,
        packet_ready: bool,
        word_ready: bool,
    ) -> ExportComb {
        let packet_valid = !self.empty() && self.import_state == 0 && !reset;
        let packet_take = ce && packet_valid && packet_ready;
        let word_take = ce && self.valid && !reset && word_ready;
        let room = !self.valid || word_ready;
        let retire = !self.result_owner || room;
        let read = ce && !reset && (self.import_state == 1 || packet_take) && retire;
        let head = self.head().value;
        let which = match self.fetch_index >> 1 {
            5 => 0,
            6 => 1,
            _ => 2,
        };
        let address = if self.fetch_index < 10 {
            u16::from(self.fetch_index & 15) * 16 + u16::from(self.consumed & 15)
        } else {
            256 | (u16::from(self.fetch_index & 1) * 128)
                | ((((head >> 32) & 1) as u16) * 64)
                | ((head >> (which * 6)) as u16 & 63)
        };
        ExportComb {
            packet_valid,
            packet_take,
            word_take,
            release: word_take && self.index == 15,
            room,
            retire,
            read,
            address,
        }
    }
    pub(super) fn clock(
        &mut self,
        reset: bool,
        ce: bool,
        x: ExportComb,
        w: ExportWrite,
        low: &[Bits; 512],
        colors: &[Bits; 2],
    ) -> Result<(), Error> {
        if reset {
            self.produced = 0;
            self.consumed = 0;
            self.import_state = 0;
            self.result_owner = false;
            self.response_index = 0;
            self.fetch_index = 0;
            self.valid = false;
            self.index = 0;
            self.data = Bits::new(0, 54);
            self.low_q = Bits::new(0, 36);
            self.high_q = Bits::new(0, 18);
            return Ok(());
        }
        if !ce {
            return Ok(());
        }
        check(self.count() <= 16, "T result queue overflow")?;
        check(!w.publish || !self.full(), "T publish without owned slot")?;
        check(
            !w.coefficient || w.address < 256,
            "T coefficient overlaps V attributes",
        )?;
        if x.word_take {
            self.data.get(54, "accepted T16 word")?;
        }
        let mut n = self.clone();
        if w.publish {
            w.descriptor.get(33, "published T descriptor")?;
            n.descriptors[(self.produced & 15) as usize] = w.descriptor;
            n.produced = self.produced.wrapping_add(1) & 31;
        }
        if x.packet_take {
            n.import_state = 1;
        }
        if x.word_take {
            n.valid = false;
            if self.index == 15 {
                n.import_state = 0;
                n.consumed = self.consumed.wrapping_add(1) & 31;
                n.fetch_index = 0;
            }
        }
        if x.retire {
            n.result_owner = false;
        }
        if self.result_owner && x.room {
            n.data = if self.response_index < 10 {
                Bits::concat(self.high_q, self.low_q, 36)
            } else if self.response_index == 10 {
                Bits::concat(
                    colors[((self.head().value >> 32) & 1) as usize].slice(0, 18),
                    self.low_q,
                    36,
                )
            } else if self.response_index == 11 {
                Bits::concat(
                    Bits::concat(
                        Bits::new(0, 4),
                        colors[((self.head().value >> 32) & 1) as usize].slice(18, 14),
                        14,
                    ),
                    self.low_q,
                    36,
                )
            } else {
                Bits::concat(Bits::new(0, 18), self.low_q, 36)
            };
            n.index = self.response_index;
            n.valid = true;
        }
        if x.read {
            self.head().get(33, "T export descriptor")?;
            n.low_q = low[x.address as usize];
            if self.fetch_index < 10 {
                n.high_q = self.high[x.address as usize];
            }
            n.response_index = self.fetch_index;
            n.result_owner = true;
            if self.fetch_index == 15 {
                n.import_state = 2;
            } else {
                n.fetch_index = self.fetch_index + 1;
            }
        }
        if w.coefficient {
            n.high[w.address as usize] = w.data.slice(36, 18);
        }
        check(
            !(x.read && w.coefficient && x.address == w.address),
            "T result cross-port collision",
        )?;
        *self = n;
        Ok(())
    }
}
