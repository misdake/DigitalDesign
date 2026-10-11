//! Split-write ordered Coordinate reservation store. Payload exists only in
//! two asynchronous-read SRAM banks; no full-row FF head or shadow is retained.
use super::{coefficient, coordinate};
pub mod canonical;
pub const OWNER_BITS: u32 = 38;
pub const RESULT_BITS: u32 = 112;
pub const COMPACT_RESULT_BITS: u32 = 36;
pub const COMPACT_SDP4_CELLS: u64 = 10 + 9;
pub const COMPACT_RAM16_BITS: u64 = COMPACT_SDP4_CELLS * 4;
pub const SDP4_CELLS: u64 = 10 + 28;
pub const RAM16_BITS: u64 = SDP4_CELLS * 4;
pub fn control_bits(capacity: usize) -> u64 {
    let a = (usize::BITS - (capacity - 1).leading_zeros()).max(1);
    let q = usize::BITS - capacity.leading_zeros();
    u64::from(3 * a + 2 * q)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner {
    pub parents: [u16; 2],
    pub nearest: bool,
    pub levels: [u8; 2],
    pub slot: u8,
    pub key: u8,
    pub last_fine: bool,
}
impl Owner {
    pub fn pack(self) -> Result<u64, String> {
        if self.parents.iter().any(|&p| p > 511)
            || self.levels.iter().any(|&n| n > 15)
            || self.slot > 15
            || self.key > 63
        {
            return Err("Coordinate owner width".into());
        }
        Ok(u64::from(self.parents[0])
            | (u64::from(self.parents[1]) << 9)
            | (u64::from(self.nearest) << 18)
            | (u64::from(self.levels[0]) << 19)
            | (u64::from(self.levels[1]) << 23)
            | (u64::from(self.slot) << 27)
            | (u64::from(self.key) << 31)
            | (u64::from(self.last_fine) << 37))
    }
    pub fn unpack(raw: u64) -> Self {
        Self {
            parents: [(raw & 511) as u16, ((raw >> 9) & 511) as u16],
            nearest: raw >> 18 & 1 != 0,
            levels: [((raw >> 19) & 15) as u8, ((raw >> 23) & 15) as u8],
            slot: ((raw >> 27) & 15) as u8,
            key: ((raw >> 31) & 63) as u8,
            last_fine: raw >> 37 & 1 != 0,
        }
    }
    pub fn finish(self, out: coordinate::Output) -> coefficient::Input {
        coefficient::Input {
            parents: self.parents,
            fractions: out.fractions,
            nearest: self.nearest,
            metadata: coefficient::Metadata {
                coordinates: out.coordinates,
                levels: self.levels,
                slot: self.slot,
                key: self.key,
                last_fine: self.last_fine,
            },
        }
    }
}
pub fn pack_result(out: coordinate::Output) -> Result<u128, String> {
    if out.coordinates.iter().flatten().any(|&v| v > 1023) {
        return Err("Coordinate result width".into());
    }
    let mut raw = 0;
    for (i, &v) in out.fractions.iter().flatten().enumerate() {
        raw |= u128::from(v) << (i * 8);
    }
    for (i, &v) in out.coordinates.iter().flatten().enumerate() {
        raw |= u128::from(v) << (32 + i * 10);
    }
    Ok(raw)
}
pub fn unpack_result(raw: u128) -> coordinate::Output {
    coordinate::Output {
        fractions: std::array::from_fn(|w| {
            std::array::from_fn(|a| ((raw >> ((w * 2 + a) * 8)) & 255) as u8)
        }),
        coordinates: std::array::from_fn(|w| {
            std::array::from_fn(|a| ((raw >> (32 + (w * 4 + a) * 10)) & 1023) as u16)
        }),
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub reset: bool,
    pub ce: bool,
    pub issue: Option<Owner>,
    pub returned: Option<coordinate::Output>,
    pub take: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub write: usize,
    pub returned: usize,
    pub read: usize,
    pub count: usize,
    pub ready: usize,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edge {
    pub owner_write: Option<(usize, u64)>,
    pub result_write: Option<(usize, u128)>,
    pub consume: Option<(usize, u64, u128)>,
}
pub struct Store {
    compact: bool,
    canonical_owner: bool,
    capacity: usize,
    owner_ram: [u64; 16],
    result_ram: [u128; 16],
    state: State,
}
impl Store {
    pub fn new(capacity: usize) -> Result<Self, String> {
        if !(1..=16).contains(&capacity) {
            return Err("Coordinate store capacity".into());
        }
        Ok(Self {
            compact: false,
            canonical_owner: false,
            capacity,
            owner_ram: std::array::from_fn(|i| 0x123456789 ^ i as u64),
            result_ram: std::array::from_fn(|i| 0xaba123456789abcdef ^ i as u128),
            state: State::default(),
        })
    }
    pub fn state(&self) -> State {
        self.state
    }
    /// Selected adjacent production profile; the generic constructor is unchanged.
    pub fn new_compact(capacity: usize) -> Result<Self, String> {
        let mut s = Self::new(capacity)?;
        s.compact = true;
        Ok(s)
    }
    /// Static selected profile: only owner24 and fine result36 are retained.
    /// Edge payloads expose actual stored words, while input() restores the
    /// independent wide arithmetic interface combinationally.
    pub fn new_canonical(capacity: usize) -> Result<Self, String> {
        let mut s = Self::new_compact(capacity)?;
        s.canonical_owner = true;
        Ok(s)
    }
    fn owner(&self, row: usize) -> Owner {
        if self.canonical_owner {
            canonical::unpack(self.owner_ram[row] as u32)
        } else {
            Owner::unpack(self.owner_ram[row])
        }
    }
    pub fn input_ready(&self) -> bool {
        self.state.count < self.capacity
    }
    pub fn inflight(&self) -> usize {
        self.state.count - self.state.ready
    }
    pub fn input(&self) -> Option<coefficient::Input> {
        (self.state.ready != 0).then(|| {
            self.owner(self.state.read).finish(if self.compact {
                super::compact::expand_result(
                    self.result_ram[self.state.read] as u64,
                    self.owner(self.state.read).levels[0],
                )
            } else {
                unpack_result(self.result_ram[self.state.read])
            })
        })
    }
    #[cfg(test)]
    pub fn return_input(&self, out: coordinate::Output) -> Option<coefficient::Input> {
        (self.inflight() != 0).then(|| self.owner(self.state.returned).finish(out))
    }
    #[cfg(test)]
    pub fn rows(&self) -> Vec<Option<(u8, bool)>> {
        (0..self.capacity)
            .map(|slot| {
                let age = (slot + self.capacity - self.state.read) % self.capacity;
                (age < self.state.count).then(|| (self.owner(slot).key, age < self.state.ready))
            })
            .collect()
    }
    pub fn tick(&mut self, t: Tick) -> Result<Edge, String> {
        let mut edge = Edge::default();
        if t.reset {
            self.state = State::default();
            return Ok(edge);
        }
        if !t.ce {
            return Ok(edge);
        }
        let old = self.state;
        if (t.issue.is_some() && !self.input_ready())
            || (t.returned.is_some() && self.inflight() == 0)
            || (t.take && old.ready == 0)
        {
            return Err("Coordinate store old ownership/credit".into());
        }
        // Validate both writes before changing any RAM or ownership state.
        let owner = t
            .issue
            .map(|o| {
                if self.canonical_owner {
                    canonical::pack(o).map(u64::from)
                } else {
                    o.pack()
                }
            })
            .transpose()?;
        let result = t
            .returned
            .map(|o| {
                if self.compact {
                    let owner = self.owner(old.returned);
                    let raw = super::compact::pack_result(o);
                    if super::compact::expand_result(raw, owner.levels[0]) != o {
                        return Err("Coordinate compact domain".into());
                    }
                    Ok(u128::from(raw))
                } else {
                    pack_result(o)
                }
            })
            .transpose()?;
        if t.take {
            edge.consume = Some((
                old.read,
                self.owner_ram[old.read],
                self.result_ram[old.read],
            ));
        }
        if let Some(data) = owner {
            edge.owner_write = Some((old.write, data));
            self.owner_ram[old.write] = data;
            self.state.write = (old.write + 1) % self.capacity;
        }
        if let Some(data) = result {
            edge.result_write = Some((old.returned, data));
            self.result_ram[old.returned] = data;
            self.state.returned = (old.returned + 1) % self.capacity;
        }
        if t.take {
            self.state.read = (old.read + 1) % self.capacity;
        }
        self.state.count = old.count + usize::from(owner.is_some()) - usize::from(t.take);
        self.state.ready = old.ready + usize::from(result.is_some()) - usize::from(t.take);
        Ok(edge)
    }
}
