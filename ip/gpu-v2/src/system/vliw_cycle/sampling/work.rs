//! Work16 metadata/control rows, two paid control heads and live payload row.
use super::{
    super::{require, vertex::Bits, ContractError},
    word::Word,
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub valid: bool,
    pub member: Word,
    pub out_ready: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub ready: bool,
    pub valid: bool,
    pub member: Word,
    pub tap: Bits,
    pub read: bool,
    pub write: bool,
    pub capture: bool,
    pub ack: bool,
    pub read_row: u8,
    pub write_row: u8,
    pub materialized: u8,
    pub loaded: u8,
}
#[derive(Clone, Debug, Default)]
pub struct Work {
    pub payload: [Bits; 16],
    pub control: [Bits; 16],
    pub heads: [Bits; 2],
    pub cursors: [Bits; 2],
    pub head_valid: u8,
    pub consume: bool,
    pub fill: bool,
    pub reclaim: u8,
    pub fetch: u8,
    pub write: u8,
    pub count: u8,
    pub head_count: u8,
}
fn first(mask: u64) -> u64 {
    if mask & 1 != 0 {
        0
    } else if mask & 2 != 0 {
        1
    } else if mask & 4 != 0 {
        2
    } else {
        3
    }
}
impl Work {
    pub fn outputs(&self, i: Input) -> Output {
        let c = usize::from(self.consume);
        let mut member = Word::new(0, 36);
        member.set(40, 36, self.payload[usize::from(self.reclaim)]);
        member.set(36, 4, self.heads[c]);
        let ready = !i.reset && self.count < 16;
        let valid = !i.reset && self.head_valid & (1 << c) != 0;
        let take = i.ce && i.out_ready && valid;
        let emit = self.heads[c].value;
        let later = emit & ((14 << self.cursors[c].value) & 15);
        Output {
            ready,
            valid,
            member,
            tap: self.cursors[c],
            read: i.ce && !i.reset && self.head_count < 2 && self.count > self.head_count,
            write: i.ce && i.valid && ready,
            capture: take,
            ack: take && later == 0,
            read_row: self.fetch,
            write_row: self.write,
            materialized: self.count,
            loaded: self.head_count,
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let o = self.outputs(i);
        let read = self.control[usize::from(self.fetch)];
        let head = self.heads[usize::from(self.consume)];
        if !i.reset && i.ce {
            require(
                !(o.write && i.member.slice(36, 4).value == 0
                    || o.read
                        && (read.value >> 4 != 0
                            || read.value & 15 == 0
                            || self.head_valid & (1 << usize::from(self.fill)) != 0
                            || o.write && self.fetch == self.write)
                    || o.capture && head.value & (1 << o.tap.value) == 0),
                "texture Work16",
                "GPU contract: bad",
            )?;
        }
        if i.reset {
            self.head_valid = 0;
            self.consume = false;
            self.fill = false;
            self.reclaim = 0;
            self.fetch = 0;
            self.write = 0;
            self.count = 0;
            self.head_count = 0;
            return Ok(());
        }
        if !i.ce {
            return Ok(());
        }
        if o.write {
            let mut data = Bits::new(0, 36);
            data.set(24, 12, i.member.slice(64, 12));
            data.set(0, 20, i.member.slice(40, 20));
            self.payload[usize::from(self.write)] = data;
            self.control[usize::from(self.write)] =
                Bits::concat(Bits::new(0, 2), i.member.slice(36, 4), 4);
            self.write = self.write.wrapping_add(1) & 15;
        }
        if o.read {
            let f = usize::from(self.fill);
            self.heads[f] = read.slice(0, 4);
            self.cursors[f] = Bits::new(first(read.value & 15), 2);
            self.head_valid |= 1 << f;
            self.fill = !self.fill;
            self.fetch = self.fetch.wrapping_add(1) & 15;
        }
        if o.capture {
            let c = usize::from(self.consume);
            if o.ack {
                self.head_valid &= !(1 << c);
                self.consume = !self.consume;
                self.reclaim = self.reclaim.wrapping_add(1) & 15;
            } else {
                self.cursors[c] = Bits::new(first(head.value & ((14 << o.tap.value) & 15)), 2);
            }
        }
        self.count = self
            .count
            .wrapping_add(u8::from(o.write))
            .wrapping_sub(u8::from(o.ack))
            & 31;
        self.head_count = self
            .head_count
            .wrapping_add(u8::from(o.read))
            .wrapping_sub(u8::from(o.ack))
            & 3;
        Ok(())
    }
}
