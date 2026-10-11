//! Ordered UV payload controller: one SDP36, one shared raw/D head.
//! RAM DO returns on the next wall edge, even when compute CE is held. The
//! physical calendar uses odd enabled phases for C; D consumes the accepted raw head.
const MASK: u64 = (1 << 18) - 1;
pub const PAYLOAD_FF_BITS: u64 = 144 + 32;
/// Fixed maximum-width owner/count/valid registers used by the RTL controller.
pub const CONTROL_FF_BITS: u64 = 3 + 3 + 1 + 1 + 3 + 2 + 1 + 1 + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coordinate {
    pub slot: usize,
    pub lane: usize,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub ce: bool,
    pub phase: u8,
    pub admission: Option<(usize, [i64; 8])>,
    pub take_derivative: bool,
    pub coordinate: Option<Coordinate>,
    pub take_coordinate: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Read {
    pub address: usize,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edge {
    pub write: Option<(usize, u64)>,
    pub read: Option<Read>,
    pub returned: Option<Read>,
    pub published: Option<usize>,
}
struct Raw {
    slot: usize,
    next: usize,
    taken: bool,
    words: [u64; 4],
}
struct Head {
    owner: Coordinate,
    data: Option<[u32; 2]>,
}
pub struct Store {
    ram: Vec<u64>,
    raw: Option<Raw>,
    coordinate: Option<Head>,
    pending: bool,
    /// Models the native synchronous DO; not another soft payload register.
    ram_do: u64,
}
fn pair(word: u64) -> [u32; 2] {
    [word as u32 & 0xffff, (word >> 18) as u32 & 0xffff]
}
fn signed(v: u64) -> i64 {
    ((v & MASK) as i64) << 46 >> 46
}
impl Store {
    pub fn new(contexts: usize) -> Result<Self, String> {
        if !(1..=8).contains(&contexts) {
            return Err("UV context capacity".into());
        }
        Ok(Self {
            ram: (0..contexts * 4).map(|i| 0xabad_1234 ^ i as u64).collect(),
            raw: None,
            coordinate: None,
            pending: false,
            ram_do: 0,
        })
    }
    pub fn input_ready(&self) -> bool {
        self.raw.is_none()
    }
    pub fn idle(&self) -> bool {
        self.raw.is_none() && self.coordinate.is_none() && !self.pending
    }
    pub fn derivative(&self) -> Option<(usize, [i64; 8])> {
        self.raw.as_ref().filter(|d| !d.taken).map(|d| {
            (
                d.slot,
                std::array::from_fn(|i| signed(d.words[i / 2] >> (18 * (i % 2)))),
            )
        })
    }
    pub fn coordinate(&self) -> Option<(Coordinate, [u32; 2])> {
        let head = self.coordinate.as_ref()?;
        let data = head
            .data
            .or_else(|| self.pending.then(|| pair(self.ram_do)))?;
        Some((head.owner, data))
    }
    pub fn coordinate_empty(&self) -> bool {
        self.coordinate.is_none()
    }
    /// Diagnostic only. Runtime arithmetic never reads the array through here.
    #[cfg(test)]
    pub fn rows(&self) -> Vec<[i64; 8]> {
        self.ram
            .as_chunks::<4>()
            .0
            .iter()
            .map(|row| std::array::from_fn(|i| signed(row[i / 2] >> (18 * (i % 2)))))
            .collect()
    }
    pub fn tick(&mut self, t: Tick) -> Result<Edge, String> {
        if t.phase >= 8
            || (!t.ce
                && (t.admission.is_some()
                    || t.take_derivative
                    || t.coordinate.is_some()
                    || t.take_coordinate))
        {
            return Err("UV disabled command/phase".into());
        }
        let mut edge = Edge::default();
        let old_ready = self.input_ready();
        // Capture the previously reserved hard return regardless of compute CE.
        if std::mem::take(&mut self.pending) {
            let head = self
                .coordinate
                .as_mut()
                .ok_or("UV C return without reservation")?;
            edge.returned = Some(Read {
                address: head.owner.slot * 4 + head.owner.lane,
            });
            if head.data.is_some() {
                return Err("UV duplicate/misowned C return".into());
            }
            head.data = Some(pair(self.ram_do));
        }
        if !t.ce {
            return Ok(edge);
        }
        if t.take_derivative {
            if self.derivative().is_none() || t.phase != 0 {
                return Err("UV D take without old full head/phase".into());
            }
            self.raw.as_mut().unwrap().taken = true;
        }
        if t.take_coordinate {
            if self.coordinate().is_none() || t.phase & 1 != 0 {
                return Err("UV C take without return/phase".into());
            }
            self.coordinate = None;
        }
        // One accepted head supplies both D and four serialized RAM writes.
        // Never sample an unaccepted offer, or borrow this edge's release.
        if t.admission.is_some() && !old_ready {
            return Err("UV admission borrowed raw-head release".into());
        }
        if let Some((slot, uv)) = t.admission {
            if slot * 4 >= self.ram.len()
                || uv.iter().any(|&v| !(-(1 << 17)..(1 << 17)).contains(&v))
            {
                return Err("UV admission range".into());
            }
            let words = std::array::from_fn(|i| {
                (uv[i * 2] as u64 & MASK) | ((uv[i * 2 + 1] as u64 & MASK) << 18)
            });
            self.raw = Some(Raw {
                slot,
                next: 0,
                taken: false,
                words,
            });
        }
        if let Some(raw) = self.raw.as_mut().filter(|r| r.next < 4) {
            edge.write = Some((raw.slot * 4 + raw.next, raw.words[raw.next]));
            raw.next += 1;
            if raw.next == 4 {
                edge.published = Some(raw.slot);
            }
        }
        if self.raw.as_ref().is_some_and(|r| r.next == 4 && r.taken) {
            self.raw = None;
        }
        if let Some(owner) = t.coordinate {
            if t.phase & 1 == 0
                || self.coordinate.is_some()
                || owner.lane >= 4
                || self
                    .raw
                    .as_ref()
                    .is_some_and(|r| r.slot == owner.slot && r.next < 4)
                || edge.published == Some(owner.slot)
            {
                return Err("UV C reservation overflow/phase/unpublished".into());
            }
            self.coordinate = Some(Head { owner, data: None });
            edge.read = Some(Read {
                address: owner.slot * 4 + owner.lane,
            });
        }
        if let Some(read) = edge.read {
            if read.address >= self.ram.len() || edge.write.is_some_and(|(a, _)| a == read.address)
            {
                return Err("UV RAM address/collision".into());
            }
            self.ram_do = self.ram[read.address];
            self.pending = true;
        }
        if let Some((address, data)) = edge.write {
            self.ram[address] = data;
        }
        Ok(edge)
    }
}
