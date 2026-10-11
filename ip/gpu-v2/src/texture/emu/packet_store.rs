//! Ordered Runtime packet pool. Payload and owner RAM have one W/one R each.
//! Owner prefetch is supplied by the actual compact packet E0 valid, not time.
//! Generic Session packet timing remains in the independent legacy Pool.

pub const PAYLOAD_MASK: u128 = (1_u128 << 72) - 1;
pub const HEAD_FF_BITS: u64 = 2 * 72;
/// Five6-bit row cursors, counts5/5/6/6, publication16, head valid2,
/// two head pointers, pending row/head/valid8, owner row/valid5 and fault1.
pub const CONTROL_FF_BITS: u64 = 30 + 22 + 16 + 2 + 2 + 8 + 5 + 1;
pub const HARD_OUTPUT_BITS: u64 = 72 + 36;
#[cfg(test)]
pub(crate) mod qualification;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tick {
    pub reset: bool,
    pub ce: bool,
    pub reserve: Option<u8>,
    pub write: Option<u128>,
    pub owner_read: bool,
    pub consume: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Control {
    pub allocate: u8,
    pub write: u8,
    pub transfer: u8,
    pub read: u8,
    pub reclaim: u8,
    pub producer: u8,
    pub groups: u8,
    pub rows: u8,
    pub unwritten: u8,
    pub published: u16,
    pub head_valid: [bool; 2],
    pub read_head: u8,
    pub pop_head: u8,
    pub pending: bool,
    pub pending_row: u8,
    pub pending_head: u8,
    pub owner_valid: bool,
    pub owner_row: u8,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edge {
    pub next: Control,
    pub reserve: Option<u8>,
    pub write: Option<u8>,
    pub transfer: Option<u8>,
    pub read: Option<(u8, u8)>,
    pub capture: Option<(u8, u8, u128)>,
    pub consume: Option<(u8, u128)>,
    pub owner_read: Option<u8>,
}
pub struct Store {
    pub control: Control,
    owners: [u8; 16],
    payload: [u128; 64],
    // These two values represent native synchronous macro outputs, not soft heads.
    owner_do: u8,
    payload_do: u128,
    heads: [u128; 2],
    fault: bool,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            control: Control::default(),
            owners: std::array::from_fn(|i| (i as u8) ^ 63),
            payload: std::array::from_fn(|i| (PAYLOAD_MASK ^ i as u128) & PAYLOAD_MASK),
            owner_do: 0,
            payload_do: 0,
            heads: [0; 2],
            fault: false,
        }
    }
}
impl Store {
    pub fn front(&self) -> Option<u128> {
        let c = self.control;
        c.head_valid[usize::from(c.pop_head)].then_some(self.heads[usize::from(c.pop_head)])
    }
    pub fn producer_ready(&self) -> bool {
        !self.fault && self.control.producer < 16
    }
    pub fn idle(&self) -> bool {
        let c = self.control;
        c.producer == 0 && c.groups == 0 && c.rows == 0
    }
    pub fn faulted(&self) -> bool {
        self.fault
    }
    /// Combinational preflight. No mutation, including on invalid input/control.
    pub fn prepare(&self, t: Tick) -> Result<Edge, String> {
        if t.reset {
            return Ok(Edge::default());
        }
        if self.fault {
            return Err("packet store terminal fault".into());
        }
        let c = self.control;
        let mut e = Edge {
            next: c,
            ..Default::default()
        };
        if !t.ce {
            return Ok(e);
        }
        if [
            c.allocate,
            c.write,
            c.transfer,
            c.read,
            c.reclaim,
            c.pending_row,
        ]
        .iter()
        .any(|v| *v >= 64)
            || c.read_head > 1
            || c.pop_head > 1
            || c.pending_head > 1
            || c.owner_row > 15
            || c.producer > 16
            || c.groups > 32
            || c.rows > 48
            || c.unwritten > c.producer
        {
            return Err("packet store control range".into());
        }
        let heads = c.head_valid.iter().filter(|v| **v).count() as u8;
        let held = heads + u8::from(c.pending);
        if held > 2 || c.groups < held || c.rows != c.producer + c.groups - heads {
            return Err("packet store occupancy".into());
        }
        if (c.read_head ^ c.pop_head) != (held & 1)
            || (heads != 0 && !c.head_valid[usize::from(c.pop_head)])
            || (c.pending
                && (c.pending_head != (c.read_head ^ 1)
                    || c.head_valid[usize::from(c.pending_head)]))
        {
            return Err("packet store head reservation identity".into());
        }
        let unread = c.groups - heads - u8::from(c.pending);
        let distance = |a: u8, b: u8| a.wrapping_sub(b) & 63;
        if distance(c.allocate, c.transfer) != c.producer
            || distance(c.allocate, c.write) != c.unwritten
            || distance(c.allocate, c.reclaim) != c.rows
            || distance(c.transfer, c.read) != unread
            || distance(c.read, c.reclaim) != u8::from(c.pending)
            || (c.owner_valid && c.owner_row != c.write & 15)
            || (c.producer != 0
                && (c.published >> (c.transfer & 15) & 1 != 0) != (c.producer > c.unwritten))
        {
            return Err("packet store ordered cursor/publication identity".into());
        }
        let n = &mut e.next;
        if t.consume {
            let head = usize::from(c.pop_head);
            if !c.head_valid[head] {
                return Err("packet store consume without old head".into());
            }
            e.consume = Some((c.pop_head, self.heads[head]));
            n.head_valid[head] = false;
            n.pop_head ^= 1;
            n.groups -= 1;
        }
        if let Some(owner) = t.reserve {
            if owner > 63
                || c.producer >= 16
                || c.rows >= 48
                || c.published >> (c.allocate & 15) & 1 != 0
            {
                return Err("packet store reservation credit/owner".into());
            }
            e.reserve = Some(c.allocate);
            n.allocate = (c.allocate + 1) & 63;
            n.producer += 1;
            n.rows += 1;
            n.unwritten += 1;
        }
        if let Some(word) = t.write {
            if word > PAYLOAD_MASK
                || c.unwritten == 0
                || !c.owner_valid
                || c.owner_row != c.write & 15
                || c.published >> (c.write & 15) & 1 != 0
            {
                return Err("packet store write reservation/return".into());
            }
            let owner = (((word >> 66) & 15) * 4 + ((word >> 70) & 3)) as u8;
            if owner != self.owner_do {
                return Err("packet store write owner mismatch".into());
            }
            e.write = Some(c.write);
            n.write = (c.write + 1) & 63;
            n.unwritten -= 1;
            n.published |= 1 << (c.write & 15);
            n.owner_valid = false;
        }
        if t.owner_read {
            if n.owner_valid || c.unwritten <= u8::from(t.write.is_some()) {
                return Err("packet store owner read reservation".into());
            }
            let row = (c.write + u8::from(t.write.is_some())) & 15;
            if t.reserve.is_some() && row == c.allocate & 15 {
                return Err("packet store owned owner R/W collision".into());
            }
            e.owner_read = Some(row);
            n.owner_valid = true;
            n.owner_row = row;
        }
        if c.pending {
            let head = usize::from(c.pending_head);
            if n.head_valid[head] || c.pending_row != c.reclaim || c.rows == 0 {
                return Err("packet store synchronous return owner".into());
            }
            e.capture = Some((c.pending_row, c.pending_head, self.payload_do));
            n.head_valid[head] = true;
            n.pending = false;
            n.reclaim = (c.reclaim + 1) & 63;
            n.rows -= 1;
        }
        // Only old publication and old Group credit may fund the transfer.
        if c.producer != 0 && c.groups < 32 && c.published >> (c.transfer & 15) & 1 != 0 {
            if e.write == Some(c.transfer) {
                return Err("packet store publication collision".into());
            }
            e.transfer = Some(c.transfer);
            n.transfer = (c.transfer + 1) & 63;
            n.published &= !(1 << (c.transfer & 15));
            n.producer -= 1;
            n.groups += 1;
        }
        // Consumption can free a head for a future return, never a current bypass.
        if unread != 0 && !n.head_valid[usize::from(c.read_head)] {
            if e.write == Some(c.read) {
                return Err("packet store payload R/W collision".into());
            }
            e.read = Some((c.read, c.read_head));
            n.read = (c.read + 1) & 63;
            n.pending = true;
            n.pending_row = c.read;
            n.pending_head = c.read_head;
            n.read_head ^= 1;
        }
        Ok(e)
    }
    pub fn tick(&mut self, t: Tick) -> Result<Edge, String> {
        let e = match self.prepare(t) {
            Ok(v) => v,
            Err(err) => {
                self.fault = true;
                return Err(err);
            }
        };
        if t.reset {
            self.control = Control::default();
            self.owner_do = 0;
            self.payload_do = 0;
            self.fault = false;
            return Ok(e);
        }
        if !t.ce {
            return Ok(e);
        }
        if let Some(row) = e.owner_read {
            self.owner_do = self.owners[usize::from(row)];
        }
        if let Some((row, _)) = e.read {
            self.payload_do = self.payload[usize::from(row)];
        }
        if let Some(row) = e.reserve {
            self.owners[usize::from(row & 15)] = t.reserve.unwrap();
        }
        if let Some(row) = e.write {
            self.payload[usize::from(row)] = t.write.unwrap();
        }
        if let Some((_, head, word)) = e.capture {
            self.heads[usize::from(head)] = word;
        }
        self.control = e.next;
        Ok(e)
    }
}
