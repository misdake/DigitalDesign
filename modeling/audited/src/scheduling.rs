//! Optional Step-2 resource scheduling. Numerical mode never consults capacities.
use super::*;

impl Frame<'_> {
    pub(crate) fn unit_for(
        &self,
        kind: fn(u32) -> Resource,
        width: u32,
    ) -> Result<Resource, Fault> {
        if self.model.mode == ExecutionMode::Numerical {
            return Ok(kind(width));
        }
        self.model
            .hardware
            .units
            .keys()
            .copied()
            .filter(|r| match r {
                Resource::Adder(w)
                | Resource::Compare(w)
                | Resource::RoundControl(w)
                | Resource::Select(w)
                | Resource::LeadingZeros(w)
                | Resource::Shift(w) => *w >= width && kind(*w) == *r,
                _ => false,
            })
            .min()
            .ok_or_else(|| {
                self.state.borrow_mut().faults.push(Fault::MissingResource);
                Fault::MissingResource
            })
    }
    pub(super) fn schedule(
        &self,
        state: &mut State,
        resource: Option<Resource>,
        inputs: &[ValueId],
        extra: u64,
    ) -> Result<(u64, u64, Option<usize>), Fault> {
        if self.model.mode == ExecutionMode::Numerical {
            return Ok((0, 0, None));
        }
        let earliest = inputs
            .iter()
            .map(|&i| state.values[i].ready_cycle)
            .chain([extra, state.control_ready])
            .max()
            .unwrap();
        let (issue, ready, lane) = if let Some(r) = resource {
            let Some(unit) = self.model.hardware.units.get(&r) else {
                return Err(Fault::MissingResource);
            };
            let slots = &state.availability[&r];
            let (lane, &free) = slots.iter().enumerate().min_by_key(|(_, v)| **v).unwrap();
            let cycle = earliest.max(free);
            (cycle, cycle + unit.latency, Some(lane))
        } else {
            (earliest, earliest, None)
        };
        if ready > state.limits.max_cycle {
            return Err(Fault::Deadline);
        }
        if let Some(r) = resource {
            let cap = match r {
                Resource::Read(m) => Some(
                    self.model.stores.borrow()[m]
                        .description
                        .ports
                        .max_reads_per_frame,
                ),
                Resource::Write(m) => Some(
                    self.model.stores.borrow()[m]
                        .description
                        .ports
                        .max_writes_per_frame,
                ),
                _ => None,
            };
            if cap.is_some_and(|cap| state.counts.resources.get(&r).copied().unwrap_or(0) >= cap) {
                return Err(Fault::PortFrameLimit);
            }
            state.availability.get_mut(&r).unwrap()[lane.unwrap()] =
                issue + self.model.hardware.units[&r].initiation;
            *state.histogram.entry((r, issue)).or_default() += 1;
        }
        Ok((issue, ready, lane))
    }
}

impl FrameReport {
    pub(super) fn audit_timing(&self) -> Result<(), Fault> {
        let bad = |s: &str| Fault::Audit(s.into());
        if self.mode == ExecutionMode::Numerical {
            if self.cycles != 0
                || !self.issue_histogram.is_empty()
                || self
                    .events
                    .iter()
                    .any(|e| e.issue_cycle != 0 || e.ready_cycle != 0 || e.lane.is_some())
            {
                return Err(bad("numerical work has no schedule"));
            }
            return Ok(());
        }
        let mut last_issue = BTreeMap::new();
        for e in &self.events {
            if let Some(r) = e.resource {
                let unit = self
                    .hardware
                    .units
                    .get(&r)
                    .ok_or_else(|| bad("unconfigured resource"))?;
                let lane = e.lane.ok_or_else(|| bad("missing lane"))?;
                if lane >= unit.lanes || e.ready_cycle != e.issue_cycle + unit.latency {
                    return Err(bad("latency or lane"));
                }
                if let Some(previous) = last_issue.insert((r, lane), e.issue_cycle) {
                    if e.issue_cycle < previous + unit.initiation {
                        return Err(bad("initiation interval"));
                    }
                }
            } else if e.lane.is_some() || e.ready_cycle != e.issue_cycle {
                return Err(bad("unclocked operation shape"));
            }
        }
        for (i, m) in self.memories.iter().enumerate() {
            if self.hardware.units.get(&Resource::Read(i))
                != Some(&Unit::pipelined(m.ports.read_ports, m.ports.read_latency))
                || (!m.read_only
                    && self.hardware.units.get(&Resource::Write(i))
                        != Some(&Unit::pipelined(m.ports.write_ports, 1)))
            {
                return Err(bad("storage port configuration"));
            }
            if self
                .counts
                .resources
                .get(&Resource::Read(i))
                .copied()
                .unwrap_or(0)
                > m.ports.max_reads_per_frame
                || self
                    .counts
                    .resources
                    .get(&Resource::Write(i))
                    .copied()
                    .unwrap_or(0)
                    > m.ports.max_writes_per_frame
            {
                return Err(bad("frame port budget"));
            }
        }
        Ok(())
    }
}
