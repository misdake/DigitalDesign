//! Mutable Work94 SSRAM with one paid92-bit registered return/head bank.
//! Native SSRAM fanout is sampled at R; a later C publishes valid, and only
//! an old valid head can be consumed. No hard BSRAM output register is assumed.
use super::Stage;

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
impl Member {
    pub(super) fn capture(stage: &Stage) -> Result<Self, String> {
        let mut word = 0;
        let mut low = 0;
        for (name, bits) in FIELDS {
            let value = stage.raw(name);
            if !(0..1_i128 << bits).contains(&value) {
                return Err(format!("Work member field {name}"));
            }
            word |= (value as u128) << low;
            low += bits;
        }
        debug_assert_eq!(low, 92);
        let result = Self(word);
        if result.emit() == 0 {
            return Err("Work active plane has no group".into());
        }
        Ok(result)
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
    /// Includes source rows whose pending/valid head is still expanding.
    pub materialized: usize,
    pub valid: bool,
    pub pending: bool,
    pub cursor: u8,
    pub payload: Member,
}
/// Per-edge diagnostic wires, never an additional retained payload/owner bank.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Edge {
    pub read: Option<usize>,
    pub write: Option<usize>,
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
    head: Member,
    cursor: u8,
    valid: bool,
    pending: bool,
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
impl Work {
    #[cfg(test)]
    pub(super) fn corrupt_next_read(&mut self) -> bool {
        if self.materialized == 0 || self.valid || self.pending {
            return false;
        }
        self.rows[self.read] |= 1 << 92;
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
            head: Member::default(),
            cursor: 0,
            valid: false,
            pending: false,
        })
    }
    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            read: self.read,
            write: self.write,
            materialized: self.materialized,
            valid: self.valid,
            pending: self.pending,
            cursor: self.cursor,
            payload: self.head,
        }
    }
    pub(super) fn front(&self) -> Option<(Member, u8)> {
        self.valid.then_some((self.head, self.cursor))
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
        let read = !old.valid && !old.pending && old.materialized != 0;
        if read && write.is_some() && self.read == self.write {
            return Err("Work same-row R/W".into());
        }
        if old.pending {
            self.pending = false;
            self.valid = true;
            edge.returned = true;
        }
        if read {
            // One physical R; reserve the only return/head location first.
            self.pending = true;
            let row = self.rows[self.read];
            if row >> 92 != 0 {
                return Err("Work immutable initial cursor".into());
            }
            self.head = Member(row);
            self.cursor = self.head.emit().trailing_zeros() as u8;
            if self.cursor >= 4 {
                return Err("Work empty RAM group".into());
            }
            edge.read = Some(self.read);
        }
        if consume {
            if old.payload.emit() >> old.cursor & 1 == 0 {
                return Err("Work cursor not emitted".into());
            }
            // Caller has captured all94 packet operands before this handshake.
            edge.capture = Some((old.payload, old.cursor));
            if let Some(next) = (old.cursor + 1..4).find(|&tap| old.payload.emit() >> tap & 1 != 0)
            {
                self.cursor = next;
            } else {
                self.valid = false;
                self.read = (self.read + 1) % self.capacity;
                self.materialized -= 1;
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
        if self.materialized > self.capacity || self.valid && self.pending {
            return Err("Work ownership bound".into());
        }
        Ok(edge)
    }
}
