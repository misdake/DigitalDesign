//! Fixed stage metadata banks. Read selection and fault validation precede all
//! writes; the caller retains the ordered context controls and terminal fault.
use super::{derivative::Header, lod};
#[cfg(test)]
pub(crate) mod qualification;
pub mod runtime;
pub const SDP4_CELLS: u64 = 9 + 14 + 21;
pub const RAM16_BITS: u64 = 4 * SDP4_CELLS;
/// Runtime's canonical LOD format uses seven4-bit Ready cells, not21.
pub const LOD_SDP4_CELLS: u64 = 9 + 14 + 7;
pub const LOD_RAM16_BITS: u64 = 4 * LOD_SDP4_CELLS;

/// Immutable26-bit LOD decision. Expanded coordinate operands are wires at use;
/// the general83-bit Ready contract remains available for independent callers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadyDecision(u32);
impl ReadyDecision {
    pub fn capture(v: lod::Output) -> Result<Self, String> {
        pack_ready(v)?;
        let c = v.context;
        if c.levels[0] > 10 {
            return Err("canonical Ready fine level".into());
        }
        let decision = Self(
            u32::from(c.levels[0])
                | (u32::from(c.nearest) << 4)
                | (u32::from(c.parents[1]) << 5)
                | (u32::from(v.quad) << 14)
                | (u32::from(v.mask) << 18)
                | (u32::from(v.slot) << 22),
        );
        if decision.expand() != v {
            return Err("noncanonical Ready operands".into());
        }
        Ok(decision)
    }
    pub fn bits(self) -> u32 {
        self.0
    }
    pub fn expand(self) -> lod::Output {
        let fine = (self.0 & 15) as u8;
        let coarse = fine.saturating_sub(1);
        let parent = ((self.0 >> 5) & 511) as u16;
        lod::Output {
            quad: ((self.0 >> 14) & 15) as u8,
            mask: ((self.0 >> 18) & 15) as u8,
            slot: ((self.0 >> 22) & 15) as u8,
            context: lod::CoordinateContext {
                shift: i32::from(fine.max(1)) - 8,
                nearest: self.0 >> 4 & 1 != 0,
                halve: fine > 1,
                side: [fine, coarse].map(|n| (1_u16 << n.max(1)) as i16),
                parents: [511 - parent, parent],
                levels: [fine, coarse],
                last_fine: parent == 0,
            },
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Raw {
    pub bias: i16,
    pub force_coarsest: bool,
    pub header: Header,
}
fn header(h: Header) -> Result<u64, String> {
    if h.quad > 15 || h.mask > 15 || h.slot > 15 || h.max_n > 10 || h.filter > 2 {
        return Err("context header range".into());
    }
    Ok(u64::from(h.quad)
        | (u64::from(h.mask) << 4)
        | (u64::from(h.slot) << 8)
        | (u64::from(h.max_n) << 12)
        | (u64::from(h.has_mip) << 16)
        | (u64::from(h.filter) << 17))
}
fn unpack_header(w: u64) -> Header {
    Header {
        quad: (w & 15) as u8,
        mask: ((w >> 4) & 15) as u8,
        slot: ((w >> 8) & 15) as u8,
        max_n: ((w >> 12) & 15) as u8,
        has_mip: w >> 16 & 1 != 0,
        filter: ((w >> 17) & 3) as u8,
    }
}
impl Raw {
    pub fn pack(self) -> Result<u64, String> {
        if !(-8192..=8192).contains(&self.bias) {
            return Err("context bias range".into());
        }
        Ok(u64::from(self.bias as u16)
            | (u64::from(self.force_coarsest) << 16)
            | (header(self.header)? << 17))
    }
    pub fn unpack(w: u64) -> Self {
        Self {
            bias: w as u16 as i16,
            force_coarsest: w >> 16 & 1 != 0,
            header: unpack_header(w >> 17),
        }
    }
}
pub fn pack_derived(v: lod::Input) -> Result<u64, String> {
    if v.slope >= 1 << 18 || !(-8192..=8192).contains(&v.bias) {
        return Err("context Derived range".into());
    }
    Ok(v.slope | (u64::from(v.bias as u16) << 18) | (header(v.header)? << 34))
}
pub fn unpack_derived(w: u64) -> lod::Input {
    lod::Input {
        slope: w & ((1 << 18) - 1),
        bias: (w >> 18) as u16 as i16,
        header: unpack_header(w >> 34),
    }
}
pub fn pack_ready(v: lod::Output) -> Result<u128, String> {
    let c = v.context;
    if !(-(1 << 17)..(1 << 17)).contains(&c.shift)
        || c.side.iter().any(|v| !(-2048..2048).contains(v))
        || c.parents.iter().any(|&v| v > 511)
        || c.levels.iter().any(|&v| v > 15)
        || v.quad > 15
        || v.mask > 15
        || v.slot > 15
    {
        return Err("context Ready range".into());
    }
    Ok((c.shift as u32 as u128 & 0x3ffff)
        | (u128::from(c.nearest) << 18)
        | (u128::from(c.halve) << 19)
        | ((c.side[0] as u16 as u128 & 4095) << 20)
        | ((c.side[1] as u16 as u128 & 4095) << 32)
        | (u128::from(c.parents[0]) << 44)
        | (u128::from(c.parents[1]) << 53)
        | (u128::from(c.levels[0]) << 62)
        | (u128::from(c.levels[1]) << 66)
        | (u128::from(c.last_fine) << 70)
        | (u128::from(v.quad) << 71)
        | (u128::from(v.mask) << 75)
        | (u128::from(v.slot) << 79))
}
pub fn unpack_ready(w: u128) -> lod::Output {
    lod::Output {
        context: lod::CoordinateContext {
            shift: ((w as u32 & 0x3ffff) << 14) as i32 >> 14,
            nearest: w >> 18 & 1 != 0,
            halve: w >> 19 & 1 != 0,
            side: std::array::from_fn(|i| {
                ((((w >> (20 + 12 * i)) as u16 & 4095) << 4) as i16) >> 4
            }),
            parents: std::array::from_fn(|i| ((w >> (44 + 9 * i)) & 511) as u16),
            levels: std::array::from_fn(|i| ((w >> (62 + 4 * i)) & 15) as u8),
            last_fine: w >> 70 & 1 != 0,
        },
        quad: ((w >> 71) & 15) as u8,
        mask: ((w >> 75) & 15) as u8,
        slot: ((w >> 79) & 15) as u8,
    }
}
/// Old control supplies the addresses and type/free checks. No field below is
/// retained after an edge; return values are wires, not another payload head.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub reset: bool,
    pub ce: bool,
    pub fault: bool,
    pub phase: u8,
    pub control_bad: bool,
    pub raw_issue: Option<usize>,
    pub derived_issue: Option<usize>,
    pub ready_read: Option<usize>,
    pub admission: Option<(usize, Raw)>,
    pub d_return: Option<(usize, lod::Input)>,
    pub l_return: Option<(usize, lod::Output)>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edge {
    pub reads: [Option<(usize, u128)>; 3],
    pub writes: [Option<(usize, u128)>; 3],
}
pub struct Store {
    capacity: usize,
    lod_ready: bool,
    raw: [u64; 16],
    derived: [u64; 16],
    ready: [u128; 16],
}
impl Store {
    #[cfg(test)]
    pub(crate) fn rows(&self) -> ([u64; 16], [u64; 16], [u128; 16]) {
        (
            self.raw,
            self.derived,
            self.ready.map(|w| self.ready_word(w)),
        )
    }
    pub fn new(capacity: usize) -> Result<Self, String> {
        if !(1..=8).contains(&capacity) {
            return Err("context capacity".into());
        }
        Ok(Self {
            capacity,
            lod_ready: false,
            raw: std::array::from_fn(|i| 0xaba123456 ^ i as u64),
            derived: std::array::from_fn(|i| 0x123456789abcde ^ i as u64),
            ready: std::array::from_fn(|i| 0x123456789abcdef01234 ^ i as u128),
        })
    }
    /// Only actual canonical LOD returns are accepted by this format. Logical
    /// Edge words remain the83-bit interface; native writes retain decision26.
    pub fn new_lod_ready(capacity: usize) -> Result<Self, String> {
        let mut store = Self::new(capacity)?;
        store.lod_ready = true;
        store.ready = std::array::from_fn(|i| ((0x02ab_ab70 ^ i as u128) & !15) | 7);
        Ok(store)
    }
    fn ready_word(&self, word: u128) -> u128 {
        if self.lod_ready {
            pack_ready(ReadyDecision(word as u32).expand()).expect("validated Ready decision")
        } else {
            word
        }
    }
    /// Combinational port evaluation. Errors occur before any bank/control commit.
    /// Raw attempted CE is evaluated first even if a peer denies shared commit.
    pub fn prepare_shared(&self, proposal: Tick, commit_allow: bool) -> Result<Edge, String> {
        let edge = self.prepare(proposal)?;
        Ok(if commit_allow { edge } else { Edge::default() })
    }

    /// Combinational port evaluation. Errors occur before any bank/control commit.
    pub fn prepare(&self, t: Tick) -> Result<Edge, String> {
        if t.reset || !t.ce || t.fault {
            return Ok(Edge::default());
        }
        if t.control_bad
            || t.phase > 7
            || (t.raw_issue.is_some() && t.phase != 0)
            || (t.d_return.is_some() && t.phase != 3)
            || (t.derived_issue.is_some() && t.phase != 4)
            || (t.l_return.is_some() && t.phase != 0)
        {
            return Err("context old control/phase".into());
        }
        let mut e = Edge::default();
        let addresses = [
            t.raw_issue.or(t.d_return.map(|v| v.0)),
            t.derived_issue.or(t.l_return.map(|v| v.0)),
            t.ready_read,
        ];
        for (bank, address) in addresses.into_iter().enumerate() {
            if let Some(a) = address {
                if a >= self.capacity {
                    return Err("context read address".into());
                }
                e.reads[bank] = Some((
                    a,
                    match bank {
                        0 => self.raw[a] as u128,
                        1 => self.derived[a] as u128,
                        _ => self.ready_word(self.ready[a]),
                    },
                ));
            }
        }
        if let Some((a, v)) = t.admission {
            e.writes[0] = Some((a, v.pack()? as u128));
        }
        if let Some((a, v)) = t.d_return {
            if Raw::unpack(e.reads[0].unwrap().1 as u64).header.quad != v.header.quad {
                return Err("context D return identity".into());
            }
            e.writes[1] = Some((a, pack_derived(v)? as u128));
        }
        if let Some((a, v)) = t.l_return {
            if unpack_derived(e.reads[1].unwrap().1 as u64).header.quad != v.quad {
                return Err("context LOD return identity".into());
            }
            if self.lod_ready {
                ReadyDecision::capture(v)?;
            }
            e.writes[2] = Some((a, pack_ready(v)?));
        }
        for (read, write) in e.reads.iter().zip(e.writes) {
            if let Some((a, _)) = write {
                if a >= self.capacity || read.is_some_and(|(r, _)| a == r) {
                    return Err("context write address/owned collision".into());
                }
            }
        }
        Ok(e)
    }
    /// Commit only the already validated bank strobes at the successful edge.
    pub fn commit(&mut self, e: Edge) {
        if let Some((a, w)) = e.writes[0] {
            self.raw[a] = w as u64;
        }
        if let Some((a, w)) = e.writes[1] {
            self.derived[a] = w as u64;
        }
        if let Some((a, w)) = e.writes[2] {
            self.ready[a] = if self.lod_ready {
                // prepare already validated every derived operand and range.
                ((w >> 62) & 15)
                    | (((w >> 18) & 1) << 4)
                    | (((w >> 53) & 511) << 5)
                    | (((w >> 71) & 4095) << 14)
            } else {
                w
            };
        }
    }
    /// Host-only diagnostic peek for events/snapshots. Never used for readiness,
    /// arithmetic, identity checks or state transitions; not a hardware RAM port.
    pub fn diagnostic_quad(&self, slot: usize, stage: u8) -> u8 {
        match stage {
            0 => Raw::unpack(self.raw[slot]).header.quad,
            1 => unpack_derived(self.derived[slot]).header.quad,
            _ => unpack_ready(self.ready_word(self.ready[slot])).quad,
        }
    }
}
