//! Runtime specialization: Raw and canonical Ready are the only payload RAMs.
//! Derived reads borrow D's real terminal bank until the next D acceptance.
use super::*;
pub const SDP4_CELLS: u64 = 9 + 7;
pub const RAM16_BITS: u64 = 4 * SDP4_CELLS;
pub struct Store {
    capacity: usize,
    raw: [u64; 16],
    ready: [u32; 16],
}
impl Store {
    #[cfg(test)]
    pub(crate) fn rows(&self) -> ([u64; 16], [u128; 16]) {
        (
            self.raw,
            self.ready
                .map(|w| pack_ready(ReadyDecision(w).expand()).unwrap()),
        )
    }
    pub fn new(capacity: usize) -> Result<Self, String> {
        if !(1..=8).contains(&capacity) {
            return Err("context capacity".into());
        }
        Ok(Self {
            capacity,
            raw: std::array::from_fn(|i| 0xaba123456 ^ i as u64),
            ready: std::array::from_fn(|i| ((0x02ab_ab70 ^ i as u32) & !15) | 7),
        })
    }
    /// Probe raw attempted operations before the shared commit gate. A peer's
    /// denial suppresses accesses only after all local validation succeeds.
    pub fn prepare_shared(
        &self,
        proposal: Tick,
        loan: Option<(usize, lod::Input)>,
        commit_allow: bool,
    ) -> Result<Edge, String> {
        let edge = self.prepare(proposal, loan)?;
        Ok(if commit_allow { edge } else { Edge::default() })
    }

    pub fn prepare(&self, t: Tick, loan: Option<(usize, lod::Input)>) -> Result<Edge, String> {
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
                let word = match bank {
                    0 => self.raw[a] as u128,
                    1 => {
                        let (owner, value) = loan.ok_or("Runtime Derived loan missing")?;
                        if owner != a {
                            return Err("Runtime Derived loan owner".into());
                        }
                        pack_derived(value)? as u128
                    }
                    _ => pack_ready(ReadyDecision(self.ready[a]).expand())?,
                };
                e.reads[bank] = Some((a, word));
            }
        }
        if let Some((a, v)) = t.admission {
            e.writes[0] = Some((a, v.pack()? as u128));
        }
        if let Some((_, v)) = t.d_return {
            pack_derived(v)?;
            if Raw::unpack(e.reads[0].unwrap().1 as u64).header.quad != v.header.quad {
                return Err("context D return identity".into());
            }
        }
        if let Some((a, v)) = t.l_return {
            if unpack_derived(e.reads[1].unwrap().1 as u64).header.quad != v.quad {
                return Err("context LOD return identity".into());
            }
            ReadyDecision::capture(v)?;
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
    pub fn commit(&mut self, e: Edge) {
        if let Some((a, w)) = e.writes[0] {
            self.raw[a] = w as u64;
        }
        assert!(e.writes[1].is_none(), "Runtime has no Derived write port");
        if let Some((a, w)) = e.writes[2] {
            self.ready[a] = (((w >> 62) & 15)
                | (((w >> 18) & 1) << 4)
                | (((w >> 53) & 511) << 5)
                | (((w >> 71) & 4095) << 14)) as u32;
        }
    }
    /// Diagnostic only; Derived's caller supplies the actual D wire, not a row.
    pub fn diagnostic_quad(&self, slot: usize, stage: u8, derived_quad: u8) -> u8 {
        match stage {
            0 => Raw::unpack(self.raw[slot]).header.quad,
            1 => derived_quad,
            _ => ReadyDecision(self.ready[slot]).expand().quad,
        }
    }
}
