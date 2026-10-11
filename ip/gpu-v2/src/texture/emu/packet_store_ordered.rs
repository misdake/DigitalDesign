//! Selected Runtime ordered PacketPool: fixed two-enabled-edge producer, no holes.
//! Payload key is returned by the ordered producer; expected-owner diagnostics
//! are deliberately absent. Generic production Store is unchanged.
//! `Tick.reserve` key and `Tick.owner_read` remain compatibility inputs only.
//! No owner RAM, owner prefetch or per-row publication state is retained.

pub const PAYLOAD_MASK: u128 = (1_u128 << 72) - 1;
pub const HEAD_FF_BITS: u64 = 2 * 72;
/// Five row cursors30, counts22, head valid2/pointers2, pending8 and fault1.
/// Pending row is a model/debug witness; native synthesis may prune its six bits.
pub const CONTROL_FF_BITS: u64 = 30 + 22 + 2 + 2 + 8 + 1;
pub const HARD_OUTPUT_BITS: u64 = 72;
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
    pub head_valid: [bool; 2],
    pub read_head: u8,
    pub pop_head: u8,
    pub pending: bool,
    pub pending_row: u8,
    pub pending_head: u8,
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
}
pub struct Store {
    pub control: Control,
    payload: [u128; 64],
    group_limit: u8,
    row_limit: u8,
    // These two values represent native synchronous macro outputs, not soft heads.
    payload_do: u128,
    heads: [u128; 2],
    fault: bool,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            control: Control::default(),
            group_limit: 32,
            row_limit: 48,
            payload: std::array::from_fn(|i| (PAYLOAD_MASK ^ i as u128) & PAYLOAD_MASK),
            payload_do: 0,
            heads: [0; 2],
            fault: false,
        }
    }
}
impl Store {
    /// Static Group48 profile on the same ring64; all row/count carriers remain u8.
    pub fn group48() -> Self {
        Self {
            group_limit: 48,
            row_limit: 63,
            ..Self::default()
        }
    }

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
    #[cfg(test)]
    pub(crate) fn hard_output(&self) -> u128 {
        self.payload_do
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
            if owner > 63 || c.producer >= 16 || c.rows >= self.row_limit {
                return Err("packet store reservation credit/owner".into());
            }
            e.reserve = Some(c.allocate);
            n.allocate = (c.allocate + 1) & 63;
            n.producer += 1;
            n.rows += 1;
            n.unwritten += 1;
        }
        if let Some(word) = t.write {
            if word > PAYLOAD_MASK || c.unwritten == 0 {
                return Err("packet store write reservation/return".into());
            }
            e.write = Some(c.write);
            n.write = (c.write + 1) & 63;
            n.unwritten -= 1;
        }
        if c.pending {
            let head = usize::from(c.pending_head);
            e.capture = Some((c.pending_row, c.pending_head, self.payload_do));
            n.head_valid[head] = true;
            n.pending = false;
            n.reclaim = (c.reclaim + 1) & 63;
            n.rows -= 1;
        }
        // Only old publication and old Group credit may fund the transfer.
        if c.groups < self.group_limit && c.producer != c.unwritten {
            e.transfer = Some(c.transfer);
            n.transfer = (c.transfer + 1) & 63;
            n.producer -= 1;
            n.groups += 1;
        }
        // Consumption can free a head for a future return, never a current bypass.
        if c.read != c.transfer && !n.head_valid[usize::from(c.read_head)] {
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
            self.payload_do = 0;
            self.fault = false;
            return Ok(e);
        }
        if !t.ce {
            return Ok(e);
        }
        if let Some((row, _)) = e.read {
            self.payload_do = self.payload[usize::from(row)];
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

#[cfg(test)]
#[path = "packet_store_ordered/qualification.rs"]
pub(crate) mod qualification;
