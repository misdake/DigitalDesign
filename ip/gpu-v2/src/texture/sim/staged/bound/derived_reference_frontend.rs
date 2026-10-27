//! Ordered context queue registers. Payload, UV and public ID owners live in
//! their existing stores; no mask/quad copy or arithmetic calendar is retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Metadata {
    Raw,
    Derived,
    Ready,
}
impl Metadata {
    pub(crate) fn diagnostic_stage(self) -> u8 {
        match self {
            Self::Raw => 0,
            Self::Derived => 1,
            Self::Ready => 2,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Context {
    pub next: usize,
    pub metadata: Metadata,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Controller {
    pub slots: Vec<Option<Context>>,
    pub reclaimable: Vec<bool>,
    pub allocate: usize,
    pub reclaim: usize,
    pub occupied: usize,
    pub d_head: usize,
    pub raw_count: usize,
    pub d_owner: Option<usize>,
    pub l_head: usize,
    pub derived_count: usize,
    pub l_owner: Option<usize>,
    pub c_head: usize,
    pub lod_count: usize,
    early: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Reads {
    pub raw: Option<usize>,
    pub derived: Option<usize>,
    pub ready: Option<usize>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tick {
    pub ce: bool,
    pub admit: bool,
    pub d_issue: bool,
    pub d_return: bool,
    pub l_issue: bool,
    pub l_return: bool,
    pub c_take: bool,
    pub c_empty: bool,
    /// Actual Ready bank mask on this edge; never saved in this controller.
    pub c_mask: u8,
    /// Completion-to-context handshake for the retained late-release profile.
    pub retire: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Lane {
    pub slot: usize,
    pub ordinal: usize,
    pub lane: u8,
    pub last: bool,
}
/// Live leaf signals observed before the queue edge. Admission already includes
/// the external public-ID and UV-ingress old-state guards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ports {
    pub ce: bool,
    pub admit: bool,
    pub d_zero: bool,
    pub l_zero: bool,
    pub d_return: bool,
    pub l_return: bool,
    pub uv_d: Option<usize>,
    pub c_mask: u8,
    pub uv_odd: bool,
    pub uv_empty: bool,
    pub uv_c: Option<(usize, usize)>,
    pub c_zero: bool,
    pub c_credit: bool,
    pub late_retire: u8,
}
impl Controller {
    pub fn new(capacity: usize, early: bool) -> Result<Self, String> {
        if !(1..=8).contains(&capacity) {
            return Err("frontend capacity".into());
        }
        Ok(Self {
            slots: vec![None; capacity],
            reclaimable: vec![false; capacity],
            allocate: 0,
            reclaim: 0,
            occupied: 0,
            d_head: 0,
            raw_count: 0,
            d_owner: None,
            l_head: 0,
            derived_count: 0,
            l_owner: None,
            c_head: 0,
            lod_count: 0,
            early,
        })
    }
    pub fn input_ready(&self) -> bool {
        self.occupied < self.slots.len()
    }
    pub fn commands(&self, p: Ports) -> Result<Tick, String> {
        let reads = self.reads(p.ce, p.d_zero, p.l_zero, p.uv_d);
        let selected = self.lane(p.c_mask)?;
        let take = p.ce
            && self.lod_count != 0
            && p.c_mask != 0
            && p.c_zero
            && p.c_credit
            && p.uv_c.is_some();
        if reads.raw.is_some() && p.uv_d != Some(self.d_head)
            || take && p.uv_c != selected.map(|s| (s.slot, usize::from(s.lane)))
        {
            return Err("frontend UV owner mismatch".into());
        }
        let tick = Tick {
            ce: p.ce,
            admit: p.ce && p.admit,
            d_issue: reads.raw.is_some(),
            l_issue: reads.derived.is_some(),
            d_return: p.ce && p.d_return,
            l_return: p.ce && p.l_return,
            c_mask: p.c_mask,
            c_take: take,
            c_empty: p.ce && self.lod_count != 0 && p.c_mask == 0,
            retire: if p.ce { p.late_retire } else { 0 },
        };
        self.prepare(tick)?;
        Ok(tick)
    }
    pub fn reads(&self, ce: bool, d_zero: bool, l_zero: bool, uv_d: Option<usize>) -> Reads {
        Reads {
            raw: (ce && self.raw_count != 0 && d_zero && uv_d.is_some()).then_some(self.d_head),
            derived: (ce && self.derived_count != 0 && l_zero).then_some(self.l_head),
            ready: (ce && self.lod_count != 0).then_some(self.c_head),
        }
    }
    pub fn lane(&self, mask: u8) -> Result<Option<Lane>, String> {
        if self.lod_count == 0 || mask == 0 {
            return Ok(None);
        }
        let c = self
            .slots
            .get(self.c_head)
            .and_then(Option::as_ref)
            .ok_or("frontend C head")?;
        if c.metadata != Metadata::Ready || mask > 15 {
            return Err("frontend C metadata".into());
        }
        let lane = (0..4u8)
            .filter(|l| mask >> l & 1 != 0)
            .nth(c.next)
            .ok_or("Runtime actual covered lane")?;
        Ok(Some(Lane {
            slot: self.c_head,
            ordinal: c.next,
            lane,
            last: c.next + 1 == mask.count_ones() as usize,
        }))
    }
    fn stage(&self, a: usize, stage: Metadata) -> bool {
        self.slots
            .get(a)
            .and_then(Option::as_ref)
            .is_some_and(|c| c.metadata == stage)
    }
    /// Compute the full next state before committing any register. Same-edge
    /// reclaim cannot fund admit; returns cannot fund D/LOD/C issue.
    pub fn prepare(&self, t: Tick) -> Result<(Self, Option<(usize, Metadata)>), String> {
        if !t.ce {
            if t.admit
                || t.d_issue
                || t.d_return
                || t.l_issue
                || t.l_return
                || t.c_take
                || t.c_empty
                || t.retire != 0
            {
                return Err("frontend disabled command".into());
            }
            return Ok((self.clone(), None));
        }
        let n = self.slots.len();
        if t.admit
            && (!self.input_ready() || self.slots.get(self.allocate).is_none_or(Option::is_some))
            || t.d_issue
                && (self.raw_count == 0
                    || self.d_owner.is_some()
                    || !self.stage(self.d_head, Metadata::Raw))
            || t.l_issue
                && (self.derived_count == 0
                    || self.l_owner.is_some()
                    || !self.stage(self.l_head, Metadata::Derived))
            || t.d_return && self.d_owner.is_none_or(|a| !self.stage(a, Metadata::Raw))
            || t.l_return
                && self
                    .l_owner
                    .is_none_or(|a| !self.stage(a, Metadata::Derived))
            || (t.c_take || t.c_empty)
                && (self.lod_count == 0 || !self.stage(self.c_head, Metadata::Ready))
            || t.c_take && t.c_empty
            || t.c_empty && t.c_mask != 0
            || t.c_mask > 15
        {
            return Err("frontend old owner/credit".into());
        }
        let lane = if t.c_take {
            Some(self.lane(t.c_mask)?.ok_or("frontend empty take")?)
        } else {
            None
        };
        let done = t.c_empty || lane.is_some_and(|l| l.last);
        let retire = t.retire
            | if done && self.early {
                1 << self.c_head
            } else {
                0
            };
        if u16::from(retire) >> n != 0 {
            return Err("frontend retirement address".into());
        }
        for i in 0..n {
            if retire >> i & 1 != 0 && (self.slots[i].is_none() || self.reclaimable[i]) {
                return Err("Runtime duplicate context retirement".into());
            }
        }
        let mut next = self.clone();
        if t.d_issue {
            next.d_owner = Some(self.d_head);
            next.d_head = (self.d_head + 1) % n;
            next.raw_count -= 1;
        }
        if t.l_issue {
            next.l_owner = Some(self.l_head);
            next.l_head = (self.l_head + 1) % n;
            next.derived_count -= 1;
        }
        if t.d_return {
            let a = self.d_owner.unwrap();
            next.d_owner = None;
            next.slots[a].as_mut().unwrap().metadata = Metadata::Derived;
            next.derived_count += 1;
        }
        if t.l_return {
            let a = self.l_owner.unwrap();
            next.l_owner = None;
            next.slots[a].as_mut().unwrap().metadata = Metadata::Ready;
            next.lod_count += 1;
        }
        if t.c_take {
            next.slots[self.c_head].as_mut().unwrap().next += 1;
        }
        if done {
            next.c_head = (self.c_head + 1) % n;
            next.lod_count -= 1;
        }
        for i in 0..n {
            next.reclaimable[i] |= retire >> i & 1 != 0;
        }
        let released = if self.occupied != 0 && next.reclaimable[self.reclaim] {
            let c = self.slots[self.reclaim].ok_or("Runtime context release owner")?;
            next.slots[self.reclaim] = None;
            next.reclaimable[self.reclaim] = false;
            next.reclaim = (self.reclaim + 1) % n;
            next.occupied -= 1;
            Some((self.reclaim, c.metadata))
        } else {
            None
        };
        if t.admit {
            next.slots[self.allocate] = Some(Context {
                next: 0,
                metadata: Metadata::Raw,
            });
            next.allocate = (self.allocate + 1) % n;
            next.occupied += 1;
            next.raw_count += 1;
        }
        if next.raw_count
            + next.derived_count
            + next.lod_count
            + usize::from(next.d_owner.is_some())
            + usize::from(next.l_owner.is_some())
            > next.occupied
            || next.occupied > n
        {
            return Err("frontend queue bound".into());
        }
        Ok((next, released))
    }
}
