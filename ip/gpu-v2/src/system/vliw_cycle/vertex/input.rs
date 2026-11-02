use super::{check, mask, Error};

/// Known bits are explicit. Unwritten storage is not manufactured as zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bits {
    pub value: u64,
    pub known: u64,
}
impl Bits {
    pub fn new(value: u64, width: u32) -> Self {
        Self {
            value: value & mask(width),
            known: mask(width),
        }
    }
    pub fn slice(self, lo: u32, width: u32) -> Self {
        Self {
            value: (self.value >> lo) & mask(width),
            known: (self.known >> lo) & mask(width),
        }
    }
    pub fn concat(hi: Self, lo: Self, lo_width: u32) -> Self {
        Self {
            value: (hi.value << lo_width) | lo.value,
            known: (hi.known << lo_width) | lo.known,
        }
    }
    pub fn extend(self, from: u32, to: u32) -> Self {
        let high = mask(to) ^ mask(from);
        let sign = 1u64 << (from - 1);
        Self {
            value: self.value | if self.value & sign != 0 { high } else { 0 },
            known: self.known | if self.known & sign != 0 { high } else { 0 },
        }
    }
    pub fn set(&mut self, lo: u32, width: u32, v: Self) {
        let m = mask(width) << lo;
        self.value = (self.value & !m) | ((v.value << lo) & m);
        self.known = (self.known & !m) | ((v.known << lo) & m);
    }
    pub fn is_known(self, width: u32) -> bool {
        self.known & mask(width) == mask(width)
    }
    pub fn get(self, width: u32, site: &'static str) -> Result<u64, Error> {
        if self.is_known(width) {
            Ok(self.value & mask(width))
        } else {
            Err(Error::Unknown(site))
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RamPorts {
    pub reset: bool,
    pub dma_write: bool,
    pub dma_address: u16,
    pub dma_data: u64,
    pub left_read: bool,
    pub right_read: bool,
    pub left_address: u16,
    pub right_address: u16,
    pub work_write: bool,
    pub work_address: u16,
    pub work_data: Bits,
}
#[derive(Clone, Debug)]
pub struct InputRam {
    left: [Bits; 512],
    right: [Bits; 512],
    work: [Bits; 16],
    owner: [Option<u8>; 16],
    lq: Bits,
    source_rq: Bits,
    read_work: bool,
    work_address: u8,
}
impl Default for InputRam {
    fn default() -> Self {
        Self {
            left: [Bits::default(); 512],
            right: [Bits::default(); 512],
            work: [Bits::default(); 16],
            owner: [None; 16],
            lq: Bits::default(),
            source_rq: Bits::default(),
            read_work: false,
            work_address: 0,
        }
    }
}
impl InputRam {
    pub fn left_q(&self) -> Bits {
        self.lq
    }
    pub fn right_q(&self) -> Bits {
        if self.read_work {
            self.work[self.work_address as usize]
        } else {
            self.source_rq
        }
    }
    pub fn source_q(&self) -> Bits {
        Bits::concat(self.source_rq.slice(0, 32), self.lq.slice(0, 32), 32)
    }
    pub fn peek(&self, row: u16) -> [Bits; 2] {
        [
            self.left[(row & 511) as usize],
            self.right[(row & 511) as usize],
        ]
    }
    pub fn work_peek(&self, row: u8) -> Bits {
        self.work[(row & 15) as usize]
    }
    pub fn clock(&mut self, p: RamPorts) -> Result<(), Error> {
        if p.reset {
            self.lq = Bits::new(0, 36);
            self.source_rq = Bits::new(0, 36);
            self.read_work = false;
            self.work_address = 0;
            self.owner = [None; 16];
            return Ok(());
        }
        let work_read = p.right_read && (p.right_address & 1023) >> 6 == 6;
        let source_read = p.right_read && !work_read;
        let wa = (p.dma_address & 511) as usize;
        let la = (p.left_address & 511) as usize;
        let ra = (p.right_address & 511) as usize;
        check(
            !p.dma_write || !(p.left_read && wa == la || source_read && wa == ra),
            "iz_ram36 cross-port same-address collision",
        )?;
        if work_read {
            check(
                self.owner[(p.right_address & 15) as usize] == Some((p.right_address & 63) as u8),
                "T work read before logical owner publication",
            )?;
        }
        check(
            !(work_read && p.work_write && p.right_address & 15 == p.work_address & 15),
            "T work same-slot collision",
        )?;
        check(
            !p.work_write || (p.work_address & 1023) >> 6 == 6,
            "T write outside private work page",
        )?;
        if p.left_read {
            self.lq = self.left[la];
        }
        if source_read {
            self.source_rq = self.right[ra];
        }
        if p.right_read {
            self.read_work = work_read;
        }
        if work_read {
            self.work_address = (p.right_address & 15) as u8;
        }
        if p.dma_write {
            self.left[wa] = Bits::new(p.dma_data as u32 as u64, 32).extend(32, 36);
            self.right[wa] = Bits::new(p.dma_data >> 32, 32).extend(32, 36);
        }
        if p.work_write {
            self.work[(p.work_address & 15) as usize] = p.work_data.slice(0, 36);
            self.owner[(p.work_address & 15) as usize] = Some((p.work_address & 63) as u8);
        }
        Ok(())
    }
}
