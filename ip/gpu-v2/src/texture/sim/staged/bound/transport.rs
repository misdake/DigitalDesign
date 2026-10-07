//! SPSC Work94 SSRAM with two fixed, ordered 92-bit registered heads.
//! SSRAM has asynchronous read: R captures data and valid together; only an
//! old valid head can be consumed. There is no extra return/publication edge.
//! A full row is written before publication; neither a same-edge write nor
//! release funds R. Source rows remain owned until their last packet capture.

pub(super) const HEADS: usize = 2;
pub(super) const HEAD_DATA_BITS: u64 = HEADS as u64 * 92;
// Two cursor2/valid1, read/fill head1 and occupancy2.
pub(super) const HEAD_CONTROL_BITS: u64 = 2 * 3 + 2 + 2;

const FIELDS: [(&str, u8); 22] = [
    ("w0", 9),
    ("w1", 9),
    ("w2", 9),
    ("w3", 9),
    ("emit0", 1),
    ("emit1", 1),
    ("emit2", 1),
    ("emit3", 1),
    ("tx0", 7),
    ("tx1", 7),
    ("ty0", 7),
    ("ty1", 7),
    ("lx", 3),
    ("ly", 3),
    ("same_x", 1),
    ("same_y", 1),
    ("slot", 4),
    ("n", 4),
    ("quad", 4),
    ("lane", 2),
    ("fine", 1),
    ("final", 1),
];
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Member(u128);
#[derive(Clone, Copy, Debug)]
pub(super) struct MemberFields {
    pub weights: [u16; 4],
    pub emit: [bool; 4],
    pub tiles: [u8; 4],
    pub local: [u8; 2],
    pub same: [bool; 2],
    pub slot: u8,
    pub level: u8,
    pub key: u8,
    pub fine: bool,
    pub final_plane: bool,
}
impl Member {
    /// Owned scalar capture, independent of any Stage or numerical frame.
    pub(super) fn from_fields(v: MemberFields) -> Result<Self, String> {
        let values = [
            v.weights[0].into(),
            v.weights[1].into(),
            v.weights[2].into(),
            v.weights[3].into(),
            v.emit[0].into(),
            v.emit[1].into(),
            v.emit[2].into(),
            v.emit[3].into(),
            v.tiles[0].into(),
            v.tiles[1].into(),
            v.tiles[2].into(),
            v.tiles[3].into(),
            v.local[0].into(),
            v.local[1].into(),
            v.same[0].into(),
            v.same[1].into(),
            v.slot.into(),
            v.level.into(),
            (v.key / 4).into(),
            (v.key % 4).into(),
            v.fine.into(),
            v.final_plane.into(),
        ];
        if v.key > 63 {
            return Err("Work member key width".into());
        }
        let mut word = 0;
        let mut low = 0;
        for ((name, bits), value) in FIELDS.into_iter().zip(values) {
            let value: u128 = value;
            if value >= 1_u128 << bits {
                return Err(format!("Work member field {name}"));
            }
            word |= value << low;
            low += bits;
        }
        let result = Self(word);
        if result.emit() == 0 {
            return Err("Work active plane has no group".into());
        }
        Ok(result)
    }
    pub(super) fn bits(self) -> u128 {
        self.0
    }
    pub(super) fn raw(self, name: &str) -> i128 {
        let mut low = 0;
        for (field, bits) in FIELDS {
            if name == field {
                return ((self.0 >> low) & ((1 << bits) - 1)) as i128;
            }
            low += bits;
        }
        panic!("unknown Work field {name}");
    }
    pub(super) fn key(self) -> u8 {
        (self.raw("quad") * 4 + self.raw("lane")) as u8
    }
    pub(super) fn emit(self) -> u8 {
        ((self.0 >> 36) & 15) as u8
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub read: usize,
    pub write: usize,
    /// Includes source rows whose prefetched/active head is still expanding.
    pub materialized: usize,
    pub valid: bool,
    pub cursor: u8,
    pub payload: Member,
    pub fetch: usize,
    pub loaded: usize,
    pub heads: [Member; HEADS],
    pub cursors: [u8; HEADS],
    pub head_valid: u8,
    pub consume_head: usize,
    pub fill_head: usize,
}
/// Per-edge diagnostic wires, never an additional retained payload/owner bank.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Edge {
    pub read: Option<usize>,
    pub write: Option<usize>,
    /// Asynchronous SSRAM capture/publication on R, not an extra return age.
    pub returned: bool,
    pub capture: Option<(Member, u8)>,
    pub ack: bool,
}
pub(super) struct Work {
    rows: Vec<u128>,
    capacity: usize,
    read: usize,
    write: usize,
    materialized: usize,
    fetch: usize,
    loaded: usize,
    heads: [Member; HEADS],
    cursors: [u8; HEADS],
    head_valid: u8,
    consume_head: usize,
    fill_head: usize,
}

#[cfg(test)]
#[path = "transport_rtl_tests.rs"]
mod rtl_tests;
#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
impl Work {
    /// Transport-only reset. The surrounding sampler must drain external work
    /// before recreation; this local queue owns no memory-controller request.
    /// Payload rows/heads are deliberately not cleared.
    #[cfg(test)]
    fn reset(&mut self) {
        self.read = 0;
        self.write = 0;
        self.fetch = 0;
        self.materialized = 0;
        self.loaded = 0;
        self.head_valid = 0;
        self.consume_head = 0;
        self.fill_head = 0;
    }
    #[cfg(test)]
    pub(super) fn corrupt_head_tap(&mut self) -> bool {
        if self.front().is_none() {
            return false;
        }
        self.cursors[self.consume_head] = 4;
        true
    }
    #[cfg(test)]
    pub(super) fn corrupt_next_read(&mut self) -> bool {
        if self.materialized <= self.loaded || self.loaded == HEADS {
            return false;
        }
        self.rows[self.fetch] |= 1 << 92;
        true
    }
    pub(super) fn new(capacity: usize) -> Result<Self, String> {
        if !(2..=32).contains(&capacity) {
            return Err("Work capacity".into());
        }
        Ok(Self {
            rows: vec![0; capacity.next_power_of_two().max(16)],
            capacity,
            read: 0,
            write: 0,
            materialized: 0,
            fetch: 0,
            loaded: 0,
            heads: [Member::default(); HEADS],
            cursors: [0; HEADS],
            head_valid: 0,
            consume_head: 0,
            fill_head: 0,
        })
    }
    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            read: self.read,
            write: self.write,
            materialized: self.materialized,
            valid: self.head_valid >> self.consume_head & 1 != 0,
            cursor: self.cursors[self.consume_head],
            payload: self.heads[self.consume_head],
            fetch: self.fetch,
            loaded: self.loaded,
            heads: self.heads,
            cursors: self.cursors,
            head_valid: self.head_valid,
            consume_head: self.consume_head,
            fill_head: self.fill_head,
        }
    }
    pub(super) fn front(&self) -> Option<(Member, u8)> {
        (self.head_valid >> self.consume_head & 1 != 0).then_some((
            self.heads[self.consume_head],
            self.cursors[self.consume_head],
        ))
    }
    pub(super) fn tick(
        &mut self,
        ce: bool,
        write: Option<Member>,
        consume: bool,
    ) -> Result<Edge, String> {
        let mut edge = Edge::default();
        if !ce {
            return Ok(edge);
        }
        let old = self.snapshot();
        if consume && !old.valid {
            return Err("Work capture without old head".into());
        }
        if write.is_some() && old.materialized == self.capacity {
            return Err("Work write lacks old row credit".into());
        }
        let read = old.loaded < HEADS && old.materialized > old.loaded;
        if read && write.is_some() && old.fetch == old.write {
            return Err("Work same-row R/W".into());
        }
        if read {
            // Reserve an OLD empty destination. Neither a consumption nor a
            // producer publication on this edge can fund this read.
            if old.head_valid >> old.fill_head & 1 != 0 {
                return Err("Work prefetch overwrote an owned head".into());
            }
            let row = self.rows[old.fetch];
            if row >> 92 != 0 {
                return Err("Work immutable initial cursor".into());
            }
            let member = Member(row);
            let cursor = member.emit().trailing_zeros() as u8;
            if cursor >= 4 {
                return Err("Work empty RAM group".into());
            }
            self.heads[old.fill_head] = member;
            self.cursors[old.fill_head] = cursor;
            self.head_valid |= 1 << old.fill_head;
            self.fill_head = (old.fill_head + 1) % HEADS;
            self.fetch = (old.fetch + 1) % self.capacity;
            self.loaded += 1;
            edge.read = Some(old.fetch);
            edge.returned = true;
        }
        if consume {
            if old.payload.emit() >> old.cursor & 1 == 0 {
                return Err("Work cursor not emitted".into());
            }
            // Caller has captured all94 packet operands before this handshake.
            edge.capture = Some((old.payload, old.cursor));
            if let Some(next) = (old.cursor + 1..4).find(|&tap| old.payload.emit() >> tap & 1 != 0)
            {
                self.cursors[old.consume_head] = next;
            } else {
                self.head_valid &= !(1 << old.consume_head);
                self.consume_head = (old.consume_head + 1) % HEADS;
                self.read = (self.read + 1) % self.capacity;
                self.materialized -= 1;
                self.loaded -= 1;
                edge.ack = true;
            }
        }
        if let Some(member) = write {
            if member.emit() == 0 {
                return Err("Work empty write".into());
            }
            edge.write = Some(self.write);
            self.rows[self.write] = member.0; // full94 row, initial cursor2 zero
            self.write = (self.write + 1) % self.capacity;
            self.materialized += 1;
        }
        if self.materialized > self.capacity
            || self.loaded > HEADS
            || self.loaded > self.materialized
            || self.loaded != self.head_valid.count_ones() as usize
        {
            return Err("Work ownership bound".into());
        }
        Ok(edge)
    }
}
