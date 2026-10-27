//! Sequential UV ingress. Each accepted helper writes the canonical RAM and
//! one slice of the only D operand bank; incomplete quads never become D-ready.
//! Slot ownership is supplied by Context8, rather than duplicated here. The
//! parent reserves its allocation tail on raw_write and publishes only on
//! committed. D take releases this operand bank, not the context or its UV RAM.
use super::uv_store::{Coordinate, Read};

const MASK: u64 = (1 << 18) - 1;
pub const PAYLOAD_FF_BITS: u64 = 144 + 32;
pub const CONTROL_FF_BITS: u64 = 3 + 2 + 1 + 1 + 3 + 2 + 1 + 1 + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// Only row0 samples slot and the canonical Raw36 word.
    pub slot: usize,
    pub metadata: u64,
    pub uv: [i64; 2],
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub ce: bool,
    pub phase: u8,
    pub row: Option<Row>,
    pub take_derivative: bool,
    pub coordinate: Option<Coordinate>,
    pub take_coordinate: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edge {
    /// Write directly to the existing canonical Raw bank, without a shadow.
    pub raw_write: Option<(usize, u64)>,
    pub write: Option<(usize, u64)>,
    pub committed: Option<usize>,
    pub read: Option<Read>,
    pub returned: Option<Read>,
}
struct Raw {
    slot: usize,
    words: Vec<[i64; 2]>,
}
struct Head {
    owner: Coordinate,
    data: Option<[u32; 2]>,
}
pub struct Store {
    ram: Vec<[i64; 2]>,
    raw: Option<Raw>,
    coordinate: Option<Head>,
    pending: Option<[i64; 2]>,
}
fn wrapped(uv: [i64; 2]) -> [u32; 2] {
    uv.map(|v| v as u32 & 0xffff)
}
impl Store {
    pub fn new(contexts: usize) -> Result<Self, String> {
        if !(1..=8).contains(&contexts) {
            return Err("UV row context capacity".into());
        }
        Ok(Self {
            ram: vec![[0; 2]; contexts * 4],
            raw: None,
            coordinate: None,
            pending: None,
        })
    }
    pub fn input_ready(&self) -> bool {
        self.raw.as_ref().is_none_or(|r| r.words.len() < 4)
    }
    pub fn next_row(&self) -> usize {
        self.raw.as_ref().map_or(0, |r| r.words.len() & 3)
    }
    pub fn derivative(&self) -> Option<(usize, [i64; 8])> {
        let raw = self.raw.as_ref().filter(|r| r.words.len() == 4)?;
        Some((raw.slot, std::array::from_fn(|i| raw.words[i / 2][i % 2])))
    }
    pub fn coordinate_empty(&self) -> bool {
        self.coordinate.is_none()
    }
    pub fn coordinate(&self) -> Option<(Coordinate, [u32; 2])> {
        let head = self.coordinate.as_ref()?;
        let data = head.data.or_else(|| self.pending.map(wrapped))?;
        Some((head.owner, data))
    }
    pub fn idle(&self) -> bool {
        self.raw.is_none() && self.coordinate.is_none() && self.pending.is_none()
    }
    /// Test diagnostics, never an arithmetic or owner lookup port.
    #[cfg(test)]
    pub(crate) fn rows(&self) -> Vec<[i64; 8]> {
        self.ram
            .as_chunks::<4>()
            .0
            .iter()
            .map(|row| std::array::from_fn(|i| row[i / 2][i % 2]))
            .collect()
    }
    pub fn tick(&mut self, t: Tick) -> Result<Edge, String> {
        if t.phase >= 8
            || (!t.ce
                && (t.row.is_some()
                    || t.take_derivative
                    || t.coordinate.is_some()
                    || t.take_coordinate))
        {
            return Err("UV row disabled command/phase".into());
        }
        let old_ready = self.input_ready();
        let old_d = self.derivative();
        let old_c = self.coordinate();
        let old_empty = self.coordinate_empty();
        let next = self.next_row();
        if let Some(row) = t.row {
            if !old_ready || row.uv.iter().any(|v| !(-(1 << 17)..(1 << 17)).contains(v)) {
                return Err("UV row admission/precision".into());
            }
            if self.raw.is_none() && (row.slot * 4 >= self.ram.len() || row.metadata >= 1 << 36) {
                return Err("UV row begin range".into());
            }
        }
        if t.take_derivative && (old_d.is_none() || t.phase != 0) {
            return Err("UV row D take without old complete head/phase".into());
        }
        if t.take_coordinate && (old_c.is_none() || t.phase & 1 != 0) {
            return Err("UV row C take without old return/phase".into());
        }
        let writing = t
            .row
            .map(|r| (self.raw.as_ref().map_or(r.slot, |r| r.slot), next));
        if let Some(owner) = t.coordinate {
            if !old_empty
                || t.phase & 1 == 0
                || owner.slot * 4 >= self.ram.len()
                || owner.lane >= 4
                || self
                    .raw
                    .as_ref()
                    .is_some_and(|r| r.slot == owner.slot && r.words.len() < 4)
                || writing.is_some_and(|(slot, _)| slot == owner.slot)
            {
                return Err("UV row C reservation/phase/uncommitted".into());
            }
        }
        let mut edge = Edge::default();
        // A previously reserved synchronous read returns on a wall edge, CE0 included.
        if let Some(data) = self.pending.take() {
            let head = self
                .coordinate
                .as_mut()
                .ok_or("UV row return without owner")?;
            edge.returned = Some(Read {
                address: head.owner.slot * 4 + head.owner.lane,
            });
            head.data = Some(wrapped(data));
        }
        if !t.ce {
            return Ok(edge);
        }
        if t.take_derivative {
            self.raw = None;
        }
        if t.take_coordinate {
            self.coordinate = None;
        }
        if let Some(row) = t.row {
            let raw = self.raw.get_or_insert_with(|| {
                edge.raw_write = Some((row.slot, row.metadata));
                Raw {
                    slot: row.slot,
                    words: Vec::with_capacity(4),
                }
            });
            let address = raw.slot * 4 + raw.words.len();
            let word = (row.uv[0] as u64 & MASK) | ((row.uv[1] as u64 & MASK) << 18);
            edge.write = Some((address, word));
            self.ram[address] = row.uv;
            raw.words.push(row.uv);
            if raw.words.len() == 4 {
                edge.committed = Some(raw.slot);
            }
        }
        if let Some(owner) = t.coordinate {
            let address = owner.slot * 4 + owner.lane;
            edge.read = Some(Read { address });
            self.coordinate = Some(Head { owner, data: None });
            self.pending = Some(self.ram[address]);
        }
        Ok(edge)
    }
}
