//! Physical Context8 preparation owner shared by D and LOD.
//! The generic Controller remains an independent qualification reference.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Derivative,
    Derived,
    Lod,
}

#[cfg(test)]
mod qualification {
    use super::*;

    #[test]
    fn continuous_prepare_is_an_explicit_construction() {
        assert!(matches!(
            PhysicalController::new(8, true, false).unwrap(),
            PhysicalController::Reference(_)
        ));
        assert!(matches!(
            PhysicalController::new(8, true, true).unwrap(),
            PhysicalController::Continuous(_)
        ));
        assert!(PhysicalController::new(3, true, true).is_err());
        assert!(PhysicalController::new(8, false, true).is_err());
    }

    #[test]
    fn physical_prepare_owner_matches_frozen_ring_across_held_phase_zero() {
        let mut actual = PhysicalController::new(8, true, true).unwrap();
        let mut reference = Controller::new(8, true).unwrap();
        let mut phase = 0;
        let mut admitted = 0;
        let mut peak = 0;
        let mut simultaneous = 0;
        let mut denies = 0;
        for wall in 0..6000 {
            let ce = wall % 13 != 2 && wall % 7 != 4;
            let allow = wall % 11 != 3;
            let mask = if reference.c_head % 3 == 0 { 0 } else { 15 };
            let lane = reference
                .lane(mask)
                .unwrap()
                .map(|v| (v.slot, usize::from(v.lane)));
            let p = Ports {
                ce,
                admit: admitted < 160 && reference.input_ready(),
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
            let t = actual.preflight(p, false).unwrap();
            assert_eq!(t, reference.preflight(p, false).unwrap());
            let old = actual.view();
            let (next, released) = actual.prepare_shared(t, allow).unwrap();
            let (expected, expected_release) = reference.prepare_shared(t, allow).unwrap();
            assert_eq!(next.view(), expected);
            assert_eq!(released, expected_release);
            if !ce || !allow {
                assert_eq!(next.view(), old);
                denies += usize::from(ce && !allow);
            } else {
                admitted += usize::from(t.admit);
                simultaneous += usize::from(t.d_issue && t.l_return);
                phase = (phase + 1) & 7;
            }
            actual = next;
            reference = expected;
            peak = peak.max(reference.occupied);
            if admitted == 160 && reference.occupied == 0 {
                assert_eq!(peak, 8);
                assert!(simultaneous > 40 && denies > 20);
                return;
            }
        }
        panic!("bounded preparation qualification did not drain");
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Registers {
    slots: Vec<Option<Context>>,
    allocate: usize,
    occupied: usize,
    raw_count: usize,
    prepare_head: usize,
    phase: Phase,
    c_head: usize,
    ordinal: usize,
    lod_count: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum PhysicalController {
    Reference(Controller),
    Continuous(Registers),
}

impl PhysicalController {
    pub fn new(capacity: usize, early: bool, continuous: bool) -> Result<Self, String> {
        if !continuous {
            return Controller::new(capacity, early).map(Self::Reference);
        }
        if capacity != 8 || !early {
            return Err("Continuous preparation requires ordered Context8".into());
        }
        Ok(Self::Continuous(Registers {
            slots: vec![None; 8],
            allocate: 0,
            occupied: 0,
            raw_count: 0,
            prepare_head: 0,
            phase: Phase::Idle,
            c_head: 0,
            ordinal: 0,
            lod_count: 0,
        }))
    }

    /// Temporary diagnostic view; none of these reconstructed heads, owners,
    /// derived count or reclaim flags is a retained physical register.
    pub fn view(&self) -> Controller {
        match self {
            Self::Reference(c) => c.clone(),
            Self::Continuous(r) => Controller {
                slots: r.slots.clone(),
                reclaimable: vec![false; 8],
                allocate: r.allocate,
                reclaim: r.c_head,
                occupied: r.occupied,
                d_head: (r.prepare_head + usize::from(r.phase != Phase::Idle)) & 7,
                raw_count: r.raw_count,
                d_owner: (r.phase == Phase::Derivative).then_some(r.prepare_head),
                l_head: (r.prepare_head + usize::from(r.phase == Phase::Lod)) & 7,
                derived_count: usize::from(r.phase == Phase::Derived),
                l_owner: (r.phase == Phase::Lod).then_some(r.prepare_head),
                c_head: r.c_head,
                ordinal: r.ordinal,
                lod_count: r.lod_count,
                early: true,
            },
        }
    }

    #[cfg(test)]
    pub fn reference_fault_state(&mut self) -> &mut Controller {
        if matches!(self, Self::Continuous(_)) {
            *self = Self::Reference(self.view());
        }
        let Self::Reference(c) = self else {
            unreachable!()
        };
        c
    }

    pub fn reads(&self, ce: bool, dz: bool, lz: bool, uv: Option<usize>) -> Reads {
        self.view().reads(ce, dz, lz, uv)
    }

    pub fn lane(&self, mask: u8) -> Result<Option<Lane>, String> {
        self.view().lane(mask)
    }

    pub fn preflight(&self, p: Ports, reserve: bool) -> Result<Tick, String> {
        let t = self.view().preflight(p, reserve)?;
        self.prepare_shared(t, false)?;
        Ok(t)
    }

    pub fn prepare_shared(
        &self,
        t: Tick,
        allow: bool,
    ) -> Result<(Self, Option<(usize, Metadata)>), String> {
        match self {
            Self::Reference(c) => {
                let (next, released) = c.prepare_shared(t, allow)?;
                Ok((Self::Reference(next), released))
            }
            Self::Continuous(r) => {
                // Check the same old-state port contract, then independently
                // update only the physical shared-owner register allocation.
                self.view().prepare(t)?;
                if !t.ce {
                    return Ok((self.clone(), None));
                }
                if t.retire != 0
                    || t.d_issue && !(r.phase == Phase::Idle || r.phase == Phase::Lod && t.l_return)
                    || t.d_return && r.phase != Phase::Derivative
                    || t.l_issue && r.phase != Phase::Derived
                    || t.l_return && r.phase != Phase::Lod
                {
                    return Err("continuous preparation owner/calendar".into());
                }
                let done = t.c_empty || t.c_take && r.ordinal + 1 == t.c_mask.count_ones() as usize;
                let mut n = r.clone();
                let released = done.then(|| {
                    let metadata = r.slots[r.c_head].unwrap().metadata;
                    n.slots[r.c_head] = None;
                    n.c_head = (r.c_head + 1) & 7;
                    n.ordinal = 0;
                    n.occupied -= 1;
                    n.lod_count -= 1;
                    (r.c_head, metadata)
                });
                if t.c_take && !done {
                    n.ordinal += 1;
                }
                if t.l_return {
                    n.slots[r.prepare_head].as_mut().unwrap().metadata = Metadata::Ready;
                    n.prepare_head = (r.prepare_head + 1) & 7;
                    n.phase = Phase::Idle;
                    n.lod_count += 1;
                }
                if t.d_issue {
                    n.phase = Phase::Derivative;
                    n.raw_count -= 1;
                }
                if t.d_return {
                    n.slots[r.prepare_head].as_mut().unwrap().metadata = Metadata::Derived;
                    n.phase = Phase::Derived;
                }
                if t.l_issue {
                    n.phase = Phase::Lod;
                }
                if t.admit {
                    n.slots[r.allocate] = Some(Context {
                        metadata: Metadata::Raw,
                    });
                    n.allocate = (r.allocate + 1) & 7;
                    n.occupied += 1;
                    n.raw_count += 1;
                }
                if !allow {
                    return Ok((self.clone(), None));
                }
                Ok((Self::Continuous(n), released))
            }
        }
    }
}
