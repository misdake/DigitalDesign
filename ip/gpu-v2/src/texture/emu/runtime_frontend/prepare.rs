//! Physical Context8 preparation uses ordered frontiers, without slot classes.
//! The generic Controller remains an independent qualification reference.
use super::*;

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
            PhysicalController::Ordered(_)
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
            let mask = if reference.c_head.is_multiple_of(3) {
                0
            } else {
                15
            };
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
pub(crate) enum PhysicalController {
    Reference(Controller),
    Ordered(super::ordered::Frontiers),
}
impl PhysicalController {
    pub fn new(capacity: usize, early: bool, continuous: bool) -> Result<Self, String> {
        if !continuous {
            return Controller::new(capacity, early).map(Self::Reference);
        }
        if capacity != 8 || !early {
            return Err("Continuous preparation requires ordered Context8".into());
        }
        Ok(Self::Ordered(super::ordered::Frontiers::new()))
    }
    /// Reconstruct the diagnostic history; no per-slot classes are retained.
    pub fn view(&self) -> Controller {
        match self {
            Self::Reference(c) => c.clone(),
            Self::Ordered(r) => r.view(),
        }
    }
    #[cfg(test)]
    pub fn reference_fault_state(&mut self) -> &mut Controller {
        if matches!(self, Self::Ordered(_)) {
            *self = Self::Reference(self.view());
        }
        let Self::Reference(c) = self else {
            unreachable!()
        };
        c
    }
    pub fn reads(&self, ce: bool, dz: bool, lz: bool, uv: Option<usize>) -> Reads {
        match self {
            Self::Reference(c) => c.reads(ce, dz, lz, uv),
            Self::Ordered(r) => r.reads(ce, dz, lz, uv),
        }
    }
    pub fn lane(&self, mask: u8) -> Result<Option<Lane>, String> {
        match self {
            Self::Reference(c) => c.lane(mask),
            Self::Ordered(r) => r.lane(mask),
        }
    }
    pub fn preflight(&self, p: Ports, reserve: bool) -> Result<Tick, String> {
        match self {
            Self::Reference(c) => c.preflight(p, reserve),
            Self::Ordered(r) => r.preflight(p, reserve),
        }
    }
    pub fn prepare_shared(
        &self,
        t: Tick,
        allow: bool,
    ) -> Result<(Self, Option<(usize, Metadata)>), String> {
        match self {
            Self::Reference(c) => c
                .prepare_shared(t, allow)
                .map(|(n, release)| (Self::Reference(n), release)),
            Self::Ordered(r) => r
                .prepare(t, allow)
                .map(|(n, release)| (Self::Ordered(n), release)),
        }
    }
}
