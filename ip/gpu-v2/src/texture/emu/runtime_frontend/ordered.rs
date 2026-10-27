//! Closed Context8 physical model: only ordered frontier registers are stored.
//! The generic slot-history Controller stays an independent reference.
use super::*;
use std::{fmt::Write, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    D,
    Derived,
    L,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frontiers {
    allocate: usize,
    prepare_head: usize,
    c_head: usize,
    occupied: usize,
    raw_count: usize,
    lod_count: usize,
    phase: Phase,
    ordinal: usize,
}
impl Frontiers {
    pub(super) fn new() -> Self {
        Self {
            allocate: 0,
            prepare_head: 0,
            c_head: 0,
            occupied: 0,
            raw_count: 0,
            lod_count: 0,
            phase: Phase::Idle,
            ordinal: 0,
        }
    }
    fn busy(&self) -> usize {
        usize::from(self.phase != Phase::Idle)
    }
    fn d_head(&self) -> usize {
        (self.prepare_head + self.busy()) & 7
    }
    fn invariant(&self) -> bool {
        self.occupied <= 8
            && self.occupied == self.raw_count + self.lod_count + self.busy()
            && self.prepare_head == (self.c_head + self.lod_count) & 7
            && self.allocate == (self.c_head + self.occupied) & 7
            && self.ordinal < 4
            && (self.lod_count != 0 || self.ordinal == 0)
    }
    /// Diagnostic only. No vector is retained or used to validate commands.
    pub(super) fn view(&self) -> Controller {
        let mut slots = vec![None; 8];
        for (i, s) in slots.iter_mut().enumerate() {
            let distance = (i + 8 - self.c_head) & 7;
            if distance < self.occupied {
                let metadata = if distance < self.lod_count {
                    Metadata::Ready
                } else if i == self.prepare_head && matches!(self.phase, Phase::Derived | Phase::L)
                {
                    Metadata::Derived
                } else {
                    Metadata::Raw
                };
                *s = Some(Context { metadata });
            }
        }
        Controller {
            slots,
            reclaimable: vec![false; 8],
            allocate: self.allocate,
            reclaim: self.c_head,
            occupied: self.occupied,
            d_head: self.d_head(),
            raw_count: self.raw_count,
            d_owner: (self.phase == Phase::D).then_some(self.prepare_head),
            l_head: (self.prepare_head + usize::from(self.phase == Phase::L)) & 7,
            derived_count: usize::from(self.phase == Phase::Derived),
            l_owner: (self.phase == Phase::L).then_some(self.prepare_head),
            c_head: self.c_head,
            ordinal: self.ordinal,
            lod_count: self.lod_count,
            early: true,
        }
    }
    pub(super) fn lane(&self, mask: u8) -> Result<Option<Lane>, String> {
        if self.lod_count == 0 || mask == 0 {
            return Ok(None);
        }
        if mask > 15 {
            return Err("ordered mask width".into());
        }
        let lane = (0..4u8)
            .filter(|l| mask >> l & 1 != 0)
            .nth(self.ordinal)
            .ok_or("ordered C ordinal")?;
        Ok(Some(Lane {
            slot: self.c_head,
            ordinal: self.ordinal,
            lane,
            last: self.ordinal + 1 == mask.count_ones() as usize,
        }))
    }
    pub(super) fn reads(&self, ce: bool, dz: bool, lz: bool, uv: Option<usize>) -> Reads {
        Reads {
            raw: (ce && self.raw_count != 0 && dz && uv.is_some()).then_some(self.d_head()),
            derived: (ce && self.phase == Phase::Derived && lz).then_some(self.prepare_head),
            ready: (ce && self.lod_count != 0).then_some(self.c_head),
        }
    }
    pub(super) fn preflight(&self, p: Ports, reserve: bool) -> Result<Tick, String> {
        if p.ce && reserve && self.occupied >= 8 {
            return Err("ordered reserve credit".into());
        }
        let lane = self.lane(p.c_mask)?;
        let di = p.ce && self.raw_count != 0 && p.d_zero && p.uv_d.is_some();
        let li = p.ce && self.phase == Phase::Derived && p.l_zero;
        let ct = p.ce
            && self.lod_count != 0
            && p.c_mask != 0
            && p.c_zero
            && p.c_credit
            && p.uv_c.is_some();
        if di && p.uv_d != Some(self.d_head())
            || ct && p.uv_c != lane.map(|l| (l.slot, l.lane as usize))
        {
            return Err("ordered UV owner".into());
        }
        let t = Tick {
            ce: p.ce,
            admit: p.ce && p.admit,
            d_issue: di,
            l_issue: li,
            d_return: p.ce && p.d_return,
            l_return: p.ce && p.l_return,
            c_take: ct,
            c_empty: p.ce && self.lod_count != 0 && p.c_mask == 0,
            c_mask: p.c_mask,
            retire: if p.ce { p.late_retire } else { 0 },
        };
        self.prepare(t, false)?;
        Ok(t)
    }
    pub(super) fn prepare(
        &self,
        t: Tick,
        allow: bool,
    ) -> Result<(Self, Option<(usize, Metadata)>), String> {
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
                return Err("ordered CE0 command".into());
            }
            return Ok((*self, None));
        }
        if t.retire != 0
            || t.c_mask > 15
            || t.admit && self.occupied >= 8
            || t.d_issue
                && (self.raw_count == 0
                    || !(self.phase == Phase::Idle || self.phase == Phase::L && t.l_return))
            || t.d_return && self.phase != Phase::D
            || t.l_issue && self.phase != Phase::Derived
            || t.l_return && self.phase != Phase::L
            || (t.c_take || t.c_empty) && self.lod_count == 0
            || t.c_take && t.c_empty
            || t.c_empty && t.c_mask != 0
        {
            return Err("ordered old owner/calendar/credit".into());
        }
        let lane = if t.c_take {
            Some(self.lane(t.c_mask)?.ok_or("ordered empty take")?)
        } else {
            None
        };
        let done = t.c_empty || lane.is_some_and(|l| l.last);
        let mut n = *self;
        if t.l_return {
            n.prepare_head = (self.prepare_head + 1) & 7;
            n.phase = Phase::Idle;
            n.lod_count += 1;
        }
        if t.d_issue {
            n.phase = Phase::D;
            n.raw_count -= 1;
        }
        if t.d_return {
            n.phase = Phase::Derived;
        }
        if t.l_issue {
            n.phase = Phase::L;
        }
        if done {
            n.c_head = (self.c_head + 1) & 7;
            n.ordinal = 0;
            n.occupied -= 1;
            n.lod_count -= 1;
        } else if t.c_take {
            n.ordinal += 1;
        }
        if t.admit {
            n.allocate = (self.allocate + 1) & 7;
            n.occupied += 1;
            n.raw_count += 1;
        }
        if !n.invariant() {
            return Err("ordered frontier conservation".into());
        }
        Ok(if allow {
            (n, done.then_some((self.c_head, Metadata::Ready)))
        } else {
            (*self, None)
        })
    }
}

fn packed(c: &Controller) -> u128 {
    let mut word = 0u128;
    let mut shift = 0;
    let mut push = |v: usize, w: usize| {
        word |= (v as u128) << shift;
        shift += w;
    };
    for (v, w) in [
        (c.allocate, 3),
        (c.c_head, 3),
        (c.occupied, 4),
        (c.d_head, 3),
        (c.raw_count, 4),
        (usize::from(c.d_owner.is_some()), 1),
        (c.d_owner.unwrap_or(0), 3),
        (c.l_head, 3),
        (c.derived_count, 4),
        (usize::from(c.l_owner.is_some()), 1),
        (c.l_owner.unwrap_or(0), 3),
        (c.c_head, 3),
        (c.lod_count, 4),
    ] {
        push(v, w);
    }
    push(
        c.slots
            .iter()
            .enumerate()
            .fold(0, |m, (i, s)| m | usize::from(s.is_some()) << i),
        8,
    );
    push(0, 8);
    push(
        c.slots.iter().enumerate().fold(0, |m, (i, s)| {
            m | s.map_or(0, |v| v.metadata.diagnostic_stage() as usize) << (2 * i)
        }),
        16,
    );
    push(c.ordinal, 2);
    word
}

/// Finite inductive-region proof plus independent capture-history replay.
/// Fixture rows describe only the sequential replay, paired with actual RTL.
pub fn certify(path: &Path) -> Result<(), String> {
    let mut states = 0usize;
    let mut edges = 0usize;
    let mut good = 0usize;
    for c_head in 0..8 {
        for occupied in 0..=8 {
            for lod in 0..=occupied {
                for phase in [Phase::Idle, Phase::D, Phase::Derived, Phase::L] {
                    let busy = usize::from(phase != Phase::Idle);
                    if lod + busy > occupied {
                        continue;
                    }
                    for ordinal in 0..(if lod == 0 { 1 } else { 4 }) {
                        let r = Frontiers {
                            allocate: (c_head + occupied) & 7,
                            prepare_head: (c_head + lod) & 7,
                            c_head,
                            occupied,
                            raw_count: occupied - lod - busy,
                            lod_count: lod,
                            phase,
                            ordinal,
                        };
                        assert!(r.invariant());
                        states += 1;
                        for mask in 0..16 {
                            for events in 0..128 {
                                let bit = |i| events & (1 << i) != 0;
                                let t = Tick {
                                    ce: true,
                                    admit: bit(0),
                                    d_issue: bit(1),
                                    d_return: bit(2),
                                    l_issue: bit(3),
                                    l_return: bit(4),
                                    c_take: bit(5),
                                    c_empty: bit(6),
                                    c_mask: mask,
                                    retire: 0,
                                };
                                let old = r.view().prepare(t);
                                let calendar = t.d_issue
                                    && !(phase == Phase::Idle || phase == Phase::L && t.l_return)
                                    || t.d_return && phase != Phase::D
                                    || t.l_issue && phase != Phase::Derived
                                    || t.l_return && phase != Phase::L;
                                let new = r.prepare(t, true);
                                assert_eq!(new.is_err(), old.is_err() || calendar, "{r:?} {t:?}");
                                if let Ok((n, release)) = new {
                                    let (o, orel) = old.unwrap();
                                    assert_eq!(n.view(), o);
                                    assert_eq!(release, orel);
                                    good += 1;
                                }
                                edges += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    // The reference vector evolves from actual commands; it is not synthesized
    // from the new frontier counts on this continuous replay.
    let mut r = Frontiers::new();
    let mut physical = PhysicalController::new(8, true, true)?;
    assert!(PhysicalController::new(4, true, true).is_err());
    assert!(PhysicalController::new(8, false, true).is_err());
    let mut reference = Controller::new(8, true)?;
    let mut phase = 0;
    let mut admitted = 0;
    let mut peak = 0;
    let mut overlap = 0;
    let mut adone = 0;
    let mut held = 0;
    let mut text = String::new();
    for wall in 0..12000 {
        assert_eq!(r.view(), reference);
        let ce = wall % 13 != 2 && wall % 7 != 4;
        let allow = wall % 11 != 3;
        let mask = [0, 1, 3, 5, 8, 10, 14, 15][reference.c_head];
        let lane = reference.lane(mask)?.map(|l| (l.slot, l.lane as usize));
        let p = Ports {
            ce,
            admit: admitted < 320 && reference.input_ready(),
            d_zero: phase == 0,
            l_zero: phase == 4,
            d_return: phase == 3 && reference.d_owner.is_some(),
            l_return: phase == 0 && reference.l_owner.is_some(),
            uv_d: (reference.raw_count != 0).then_some(reference.d_head),
            c_mask: mask,
            uv_odd: phase & 1 != 0,
            uv_empty: false,
            uv_c: lane,
            c_zero: phase & 1 == 0,
            c_credit: wall > 160 && wall % 9 < 6,
            ..Ports::default()
        };
        let t = r.preflight(p, false)?;
        assert_eq!(physical.preflight(p, false)?, t);
        assert_eq!(
            physical.reads(p.ce, p.d_zero, p.l_zero, p.uv_d),
            reference.reads(p.ce, p.d_zero, p.l_zero, p.uv_d)
        );
        assert_eq!(physical.lane(mask)?, reference.lane(mask)?);
        assert_eq!(t, reference.preflight(p, false)?);
        let (n, release) = r.prepare(t, allow)?;
        let (physical_next, physical_release) = physical.prepare_shared(t, allow)?;
        assert_eq!(physical_next.view(), n.view());
        assert_eq!(physical_release, release);
        let (o, orel) = reference.prepare_shared(t, allow)?;
        assert_eq!(n.view(), o);
        assert_eq!(release, orel);
        let flags = u32::from(p.ce)
            | (u32::from(allow) << 1)
            | (u32::from(p.admit) << 2)
            | (u32::from(p.d_zero) << 3)
            | (u32::from(p.l_zero) << 4)
            | (u32::from(p.d_return) << 5)
            | (u32::from(p.l_return) << 6)
            | (u32::from(p.uv_d.is_some()) << 7)
            | (u32::from(p.uv_odd) << 8)
            | (u32::from(p.uv_empty) << 9)
            | (u32::from(p.uv_c.is_some()) << 10)
            | (u32::from(p.c_zero) << 11)
            | (u32::from(p.c_credit) << 12);
        let done = t.c_empty || t.c_take && r.ordinal + 1 == mask.count_ones() as usize;
        let cmd = (u8::from(done) << 6)
            | (u8::from(t.c_empty) << 5)
            | (u8::from(t.c_take) << 4)
            | (u8::from(ce && r.lod_count != 0 && mask != 0 && p.uv_odd && p.uv_empty) << 3)
            | (u8::from(ce && r.lod_count != 0) << 2)
            | (u8::from(t.l_issue) << 1)
            | u8::from(t.d_issue);
        writeln!(
            text,
            "{flags:x} {:x} {mask:x} {:x} {:x} {cmd:x} {:x} {:x}",
            p.uv_d.unwrap_or(0),
            p.uv_c.map_or(0, |v| v.0),
            p.uv_c.map_or(0, |v| v.1),
            packed(&reference),
            packed(&o)
        )
        .unwrap();
        if !ce || !allow {
            assert_eq!(n, r);
            held += 1;
        } else {
            admitted += usize::from(t.admit);
            overlap += usize::from(t.d_issue && t.l_return);
            adone += usize::from(t.admit && done);
            phase = (phase + 1) & 7;
        }
        r = n;
        physical = physical_next;
        reference = o;
        peak = peak.max(r.occupied);
        if admitted == 320 && r.occupied == 0 {
            assert_eq!(peak, 8);
            assert!(overlap > 80 && adone > 40 && held > 100);
            std::fs::write(path, text).map_err(|e| e.to_string())?;
            println!("PASS ordered regions states={states} event_edges={edges} valid_edges={good} max_event_edges=20000000");
            println!("PASS capture-history replay walls={} admitted={admitted} peak={peak} Lreturn_Dissue={overlap} admit_done={adone} held={held} max_walls=12000",wall+1);
            return Ok(());
        }
    }
    Err("ordered replay bounded drain".into())
}
