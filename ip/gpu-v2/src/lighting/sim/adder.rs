//! Adders embedded in certified logic remain real hardware work.
use super::*;
#[derive(Clone, Debug, Default)]
pub struct AdderInventory {
    pub normal_work: BTreeMap<u32, usize>,
    pub increment_work: BTreeMap<u32, usize>,
    /// Standalone provisioned lanes plus copies of every adder site inside each cone.
    pub normal_provisioned: BTreeMap<u32, usize>,
    pub normal_occupied: BTreeMap<u32, usize>,
    pub increment_provisioned: BTreeMap<u32, usize>,
    pub increment_occupied: BTreeMap<u32, usize>,
    pub normal_sites_by_width: BTreeMap<u32, usize>,
    pub increment_sites_by_width: BTreeMap<u32, usize>,
    pub cone_provisioned: usize,
    pub cone_occupied: usize,
}
impl PeriodicSchedule {
    pub fn adder_inventory(&self, plan: &Plan) -> Result<AdderInventory, String> {
        self.audit(plan)?;
        let primitive = BoundDag::new(
            &plan.template,
            Hardware {
                cone_depth: 0,
                ..plan.hardware
            },
        )?;
        let mut report = AdderInventory::default();
        for k in primitive.kinds.iter().flatten() {
            let map = match k {
                LaneKind::Add(_) => &mut report.normal_work,
                LaneKind::Increment(_) | LaneKind::Negate(_) => &mut report.increment_work,
                _ => continue,
            };
            let bits = match k {
                LaneKind::Add(w) | LaneKind::Increment(w) | LaneKind::Negate(w) => *w,
                _ => unreachable!(),
            };
            *map.entry(bits).or_default() += 1;
        }
        let mut standalone = std::collections::BTreeSet::new();
        for r in &self.slots {
            if let Some(k @ (LaneKind::Add(_) | LaneKind::Increment(_) | LaneKind::Negate(_))) =
                &r.kind
            {
                standalone.insert((k.clone(), r.lane.unwrap()));
            }
        }
        let mut types = std::collections::BTreeSet::new();
        for (k, _) in standalone {
            let (occupied, provisioned, bits) = match &k {
                LaneKind::Add(w) => (
                    &mut report.normal_occupied,
                    &mut report.normal_provisioned,
                    *w,
                ),
                LaneKind::Increment(w) | LaneKind::Negate(w) => (
                    &mut report.increment_occupied,
                    &mut report.increment_provisioned,
                    *w,
                ),
                _ => unreachable!(),
            };
            *occupied.entry(bits).or_default() += 1;
            if types.insert(k.clone()) {
                *provisioned.entry(bits).or_default() += plan.hardware.unit(&k).0;
            }
        }
        let mut functions = std::collections::BTreeSet::new();
        for cone in plan.logic_cones() {
            let kind = self.slots[cone.result_event]
                .kind
                .clone()
                .ok_or("missing cone unit")?;
            if !functions.insert(kind.clone()) {
                continue;
            }
            let copies = plan.hardware.cone_lanes_per_shape;
            report.cone_provisioned += copies;
            let occupied = self
                .slots
                .iter()
                .filter(|r| r.kind.as_ref() == Some(&kind))
                .filter_map(|r| r.lane)
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            report.cone_occupied += occupied;
            for id in std::iter::once(&cone.result_event).chain(&cone.absorbed_events) {
                let (map, bits) = match primitive.kinds[*id] {
                    Some(LaneKind::Add(w)) => (&mut report.normal_provisioned, w),
                    Some(LaneKind::Increment(w) | LaneKind::Negate(w)) => {
                        (&mut report.increment_provisioned, w)
                    }
                    _ => continue,
                };
                *map.entry(bits).or_default() += copies;
                let occupied_map = if matches!(primitive.kinds[*id], Some(LaneKind::Add(_))) {
                    &mut report.normal_occupied
                } else {
                    &mut report.increment_occupied
                };
                *occupied_map.entry(bits).or_default() += occupied;
            }
        }
        for (id, k) in primitive.kinds.iter().enumerate() {
            let map = match k {
                Some(LaneKind::Add(_)) => &mut report.normal_sites_by_width,
                Some(LaneKind::Increment(_) | LaneKind::Negate(_)) => {
                    &mut report.increment_sites_by_width
                }
                _ => continue,
            };
            let bits = plan.template.values[plan.template.events[id].output.unwrap()]
                .format
                .bits;
            *map.entry(bits).or_default() += 1;
        }
        Ok(report)
    }
}
