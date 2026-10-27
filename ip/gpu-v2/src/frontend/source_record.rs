//! Explicit ordered source-record RAM leaf. Words are opaque; no geometry ABI.
//! One partial producer and oldest committed consumer share one physical read
//! port. Payload is never reset. Publication/release use old-state credits.
pub const SLOTS: usize = 16;
pub const WORDS: usize = 32;
pub const WORD_MASK: u64 = (1 << 36) - 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Read {
    pub prepare: bool,
    pub row: u8,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs {
    pub reset: bool,
    pub ce: bool,
    pub abort: bool,
    pub write: Option<u64>,
    pub read: Option<Read>,
    pub return_ready: bool,
    pub release: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    pub allocate: u8,
    pub retire: u8,
    pub write_row: u8,
    pub return_valid: bool,
    pub return_slot: u8,
    pub return_row: u8,
    pub ram_do: u64,
    pub stopped: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub write_ready: bool,
    pub read_ready: bool,
    pub release_ready: bool,
    pub used: u8,
    pub producer_slot: u8,
    pub write_row: u8,
    pub return_valid: bool,
    pub return_slot: u8,
    pub return_row: u8,
    pub return_data: u64,
    pub fault: bool,
    pub written: bool,
    pub read_issued: bool,
    pub published: bool,
    pub released: bool,
}
impl Output {
    /// Full interface witness. Mask undefined RAM data until an owned return.
    pub fn packed(self) -> u64 {
        u64::from(self.write_ready)
            | u64::from(self.used) << 1
            | u64::from(self.producer_slot) << 6
            | u64::from(self.write_row) << 10
            | u64::from(self.read_ready) << 15
            | u64::from(self.release_ready) << 16
            | u64::from(self.fault) << 17
            | u64::from(self.return_valid) << 18
            | u64::from(self.return_slot) << 19
            | u64::from(self.return_row) << 23
            | if self.return_valid {
                self.return_data << 28
            } else {
                0
            }
    }
}

pub struct RecordEmu {
    pub regs: Registers,
    memory: [u64; SLOTS * WORDS],
}
impl Default for RecordEmu {
    fn default() -> Self {
        Self {
            regs: Registers::default(),
            // Unwritten words cannot be requested. This poison is not reset.
            memory: [0x0deadbeefa; SLOTS * WORDS],
        }
    }
}
impl RecordEmu {
    pub fn output(&self, i: Inputs) -> Output {
        let r = self.regs;
        let used = r.allocate.wrapping_sub(r.retire) & 31;
        let fault = i.abort || r.stopped;
        let available = !i.reset && !fault;
        let read_head = i.read.is_some_and(|q| !q.prepare);
        // A missing offer drives canonical prepare=0,row=0 on physical ports;
        // ready remains independent of read_valid.
        let q = i.read.unwrap_or_default();
        assert!(q.row < WORDS as u8);
        let readable = if q.prepare {
            q.row < r.write_row
        } else {
            used != 0
        };
        let mut o = Output {
            write_ready: available && used < SLOTS as u8,
            read_ready: available && (!r.return_valid || i.return_ready) && readable,
            release_ready: available
                && used != 0
                && !(r.return_valid && r.return_slot == (r.retire & 15))
                && !read_head,
            used,
            producer_slot: r.allocate & 15,
            write_row: r.write_row,
            return_valid: r.return_valid,
            return_slot: r.return_slot,
            return_row: r.return_row,
            return_data: r.ram_do,
            fault,
            ..Output::default()
        };
        let advance = i.ce && available;
        o.written = advance && o.write_ready && i.write.is_some();
        o.read_issued = advance && o.read_ready && i.read.is_some();
        o.published = o.written && r.write_row == 31;
        o.released = advance && o.release_ready && i.release;
        o
    }
    pub fn tick(&mut self, i: Inputs) -> Output {
        let o = self.output(i);
        let old = self.regs;
        if i.reset {
            self.regs = Registers::default();
            return o;
        }
        if i.abort {
            self.regs.stopped = true;
        }
        if !i.ce || o.fault {
            return o;
        }
        let write_address = usize::from(old.allocate & 15) * WORDS + usize::from(old.write_row);
        if o.read_issued {
            let q = i.read.unwrap();
            let slot = if q.prepare { old.allocate } else { old.retire } & 15;
            let address = usize::from(slot) * WORDS + usize::from(q.row);
            assert!(!o.written || write_address != address);
            self.regs.ram_do = self.memory[address];
            self.regs.return_slot = slot;
            self.regs.return_row = q.row;
            self.regs.return_valid = true;
        } else if i.return_ready {
            self.regs.return_valid = false;
        }
        if o.written {
            let data = i.write.unwrap();
            assert!(data <= WORD_MASK);
            self.memory[write_address] = data;
            self.regs.write_row = (old.write_row + 1) & 31;
            if o.published {
                self.regs.allocate = (old.allocate + 1) & 31;
            }
        }
        if o.released {
            self.regs.retire = (old.retire + 1) & 31;
        }
        assert!(self.output(Inputs::default()).used <= SLOTS as u8);
        o
    }
}
pub fn portable_rtl() -> String {
    include_str!("rtl/source_record.v").to_owned()
}
pub fn native_rtl() -> String {
    format!("`define GPU_SOURCE_RECORD_NATIVE\n{}", portable_rtl())
}
