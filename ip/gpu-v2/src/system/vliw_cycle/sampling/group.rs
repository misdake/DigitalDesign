//! One logical Group48 FIFO in 64 physical rows with paid native lookahead Q.
use super::super::{require, vertex::Bits, ContractError};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub reserve: bool,
    pub write: bool,
    pub payload: Bits,
    pub consume: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub producer_ready: bool,
    pub valid: bool,
    pub payload: Bits,
    pub next_valid: bool,
    pub next_key: Bits,
    pub producer_count: u8,
    pub group_count: u8,
    pub row_count: u8,
    pub allocated: u8,
    pub write_row: u8,
    pub read_row: u8,
    pub reclaimed: u8,
    pub strobes: u8,
    pub promote: bool,
    pub load: bool,
    pub read: bool,
    pub put: bool,
    pub take: bool,
}
#[derive(Clone, Debug)]
pub struct Group {
    pub cells: [Bits; 64],
    pub raw_q: Bits,
    pub q: Bits,
    pub head: Bits,
    pub write: u8,
    pub read: u8,
    pub used: u8,
    pub head_valid: bool,
    pub q_valid: bool,
    pub pending: bool,
    held: bool,
    held_payload: Bits,
}
impl Default for Group {
    fn default() -> Self {
        Self {
            cells: [Bits::new(0, 35); 64],
            raw_q: Bits::new(0, 35),
            q: Bits::new(0, 35),
            head: Bits::default(),
            write: 0,
            read: 0,
            used: 0,
            head_valid: false,
            q_valid: false,
            pending: false,
            held: false,
            held_payload: Bits::default(),
        }
    }
}
impl Group {
    pub fn outputs(&self, i: Input) -> Output {
        let reclaimed = self
            .read
            .wrapping_sub(u8::from(self.head_valid))
            .wrapping_sub(u8::from(self.q_valid))
            .wrapping_sub(u8::from(self.pending))
            & 63;
        let allocated = reclaimed.wrapping_add(self.used) & 63;
        let missing = allocated.wrapping_sub(self.write) & 63;
        let running = i.ce && !i.reset;
        let promote = running && self.q_valid && (!self.head_valid || i.consume);
        let load = running && self.pending;
        let read =
            running && !self.pending && self.read != self.write && (!self.q_valid || promote);
        let put = running && i.write;
        let take = running && i.consume;
        Output {
            producer_ready: !i.reset && self.used < 48,
            valid: self.head_valid,
            payload: self.head,
            next_valid: self.q_valid,
            next_key: self.q.slice(0, 15),
            producer_count: missing & 31,
            group_count: self.write.wrapping_sub(reclaimed) & 63,
            row_count: self.used,
            allocated,
            write_row: self.write,
            read_row: self.read,
            reclaimed,
            strobes: (u8::from(take) << 5)
                | (u8::from(promote) << 4)
                | (u8::from(read) << 3)
                | (u8::from(put) << 2)
                | (u8::from(put) << 1)
                | u8::from(running && i.reserve),
            promote,
            load,
            read,
            put,
            take,
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let o = self.outputs(i);
        if !i.reset {
            require(
                self.used <= 48 && o.producer_count <= 2,
                "native Group queue",
                "native Group queue prefix",
            )?;
            require(
                !(o.read && o.put && self.read == self.write),
                "native Group queue",
                "native Group queue read/write collision",
            )?;
            require(
                !(o.put
                    && (self.q_valid || self.pending)
                    && self.write == self.read.wrapping_sub(1) & 63),
                "native Group queue",
                "native Group lookahead Q overwritten",
            )?;
            require(
                !self.held || (self.head_valid && self.head == self.held_payload),
                "native Group queue",
                "native Group head Q changed while held",
            )?;
            require(
                !(i.ce
                    && (i.reserve && self.used == 48
                        || i.write && o.producer_count == 0
                        || i.consume && !self.head_valid)),
                "native Group queue",
                "GPU contract: bad",
            )?;
        }
        if i.reset {
            self.write = 0;
            self.read = 0;
            self.used = 0;
            self.head_valid = false;
            self.q_valid = false;
            self.pending = false;
            self.q = Bits::new(0, 35);
            self.raw_q = Bits::new(0, 35);
            self.held = false;
            return Ok(());
        }
        self.held = self.head_valid && !o.take;
        self.held_payload = self.head;
        let returned = self.cells[usize::from(self.read)];
        let old_raw = self.raw_q;
        let old_q = self.q;
        if o.put {
            self.cells[usize::from(self.write)] = i.payload.slice(0, 35);
            self.write = self.write.wrapping_add(1) & 63;
        }
        if o.read {
            self.raw_q = returned;
            self.read = self.read.wrapping_add(1) & 63;
        }
        if o.load {
            self.q = old_raw;
        }
        if o.promote {
            self.head = old_q;
        }
        if i.ce {
            if o.promote || o.take {
                self.head_valid = o.promote;
            }
            if o.promote || o.load {
                self.q_valid = o.load;
            }
            if o.read || o.load {
                self.pending = o.read;
            }
            self.used = self
                .used
                .wrapping_add(u8::from(i.reserve))
                .wrapping_sub(u8::from(i.consume))
                & 63;
        }
        Ok(())
    }
}
