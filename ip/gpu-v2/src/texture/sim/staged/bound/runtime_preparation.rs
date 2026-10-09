//! Private live coefficient/membership/Work/packet executor. D/LOD/coordinate
//! remain counted; downstream arithmetic advances actual scalar registers.
use super::{
    control::{Event, Hardware, Stats, Step},
    transport::Work,
    *,
};
use crate::texture::emu::coefficient::{self, CoefficientEmu};
use std::collections::VecDeque;

struct Context {
    source: Arc<Program>,
    next: usize,
    d: Option<u64>,
    lod: Option<u64>,
}
struct Coordinate {
    input: coefficient::Input,
    issue: u64,
}
struct Completion {
    slot: usize,
    left: usize,
}

#[cfg(test)]
#[derive(Default)]
pub(super) struct Trace {
    pub work: transport::Edge,
    pub coefficient: Option<coefficient::Step>,
    pub pre_work: usize,
    pub offer_cost: usize,
    pub plane: Option<(coefficient::Output, usize)>,
    pub packet: Option<i128>,
    pub member_input: Option<runtime_membership::Input>,
    pub member_banks: [Option<u128>; 7],
    pub packet_banks: [Option<u128>; 9],
}
pub(super) struct Machine {
    pub binding: Arc<Binding>,
    hardware: Hardware,
    pub stats: Stats,
    slots: Vec<Option<Context>>,
    order: VecDeque<usize>,
    live: u16,
    completion: [Option<Completion>; 16],
    coordinates: VecDeque<Coordinate>,
    ready: VecDeque<coefficient::Input>,
    coefficient: CoefficientEmu,
    ready_plane: usize,
    members: runtime_membership::Pipeline,
    work: Work,
    packets: runtime_packet::Pipeline,
    coordinate_count: usize,
    work_count: usize,
    #[cfg(test)]
    pub trace: Trace,
    #[cfg(test)]
    pub membership_ready: bool,
    #[cfg(test)]
    pub packet_ready: bool,
}
impl Machine {
    #[cfg(test)]
    pub(super) fn numerical_banks(&self) -> ([Option<u128>; 7], [Option<u128>; 9]) {
        (self.members.banks(), self.packets.banks())
    }
    #[cfg(test)]
    pub(super) fn corrupt_head_tap(&mut self) -> bool {
        self.work.corrupt_head_tap()
    }
    #[cfg(test)]
    pub(super) fn corrupt_next_work_read(&mut self) -> bool {
        self.work.corrupt_next_read()
    }
    #[cfg(test)]
    pub(super) fn coefficient_snapshot(&self) -> coefficient::Snapshot {
        self.coefficient.snapshot()
    }
    #[cfg(test)]
    pub(super) fn work_state(&self) -> transport::Snapshot {
        self.work.snapshot()
    }
    #[cfg(test)]
    pub(super) fn counts(&self) -> (usize, usize, usize) {
        (
            self.work_count,
            self.coordinate_count,
            self.members.inflight(),
        )
    }
    pub(super) fn new(binding: Arc<Binding>, hardware: Hardware) -> Result<Self, String> {
        hardware.validate()?;
        Ok(Self {
            binding,
            hardware,
            stats: Stats::default(),
            slots: (0..hardware.contexts).map(|_| None).collect(),
            order: VecDeque::new(),
            live: 0,
            completion: std::array::from_fn(|_| None),
            coordinates: VecDeque::new(),
            ready: VecDeque::new(),
            coefficient: CoefficientEmu::new(hardware.max_cycles)
                .map_err(|e| format!("coefficient: {e:?}"))?,
            ready_plane: 0,
            members: runtime_membership::Pipeline::default(),
            work: Work::new(hardware.work_credits)?,
            packets: runtime_packet::Pipeline::default(),
            coordinate_count: 0,
            work_count: 0,
            #[cfg(test)]
            trace: Trace::default(),
            #[cfg(test)]
            membership_ready: true,
            #[cfg(test)]
            packet_ready: true,
        })
    }
    pub(super) fn live_mask(&self) -> u16 {
        self.live
    }
    pub(super) fn input_ready(&self, quad: u8) -> bool {
        quad < 16 && self.live >> quad & 1 == 0 && self.slots.iter().any(Option::is_none)
    }
    pub(super) fn idle(&self) -> bool {
        self.live == 0
            && self.coordinate_count == 0
            && self.work_count == 0
            && self.packets.inflight() == 0
            && self.coefficient.idle()
    }
    fn shared_release(&mut self, slot: usize, events: &mut Vec<Event>) -> Result<(), String> {
        let c = self.slots[slot]
            .take()
            .ok_or("Runtime context release owner")?;
        self.order.retain(|&s| s != slot);
        events.push(Event::SharedRelease {
            program: usize::from(c.source.input.quad_id),
            slot,
        });
        Ok(())
    }
    fn release(&mut self, quad: u8, events: &mut Vec<Event>) -> Result<(), String> {
        let completion = self.completion[usize::from(quad)]
            .take()
            .ok_or("Runtime completion release owner")?;
        self.live &= !(1 << quad);
        if !self.hardware.release_after_capture {
            self.shared_release(completion.slot, events)?;
        }
        self.stats.released += 1;
        events.push(Event::Release {
            program: usize::from(quad),
        });
        Ok(())
    }
    fn issue(
        events: &mut Vec<Event>,
        stage: &'static str,
        key: u8,
        mask: u8,
        plane: usize,
        packet: usize,
    ) -> Result<(), String> {
        if mask >> (key % 4) & 1 == 0 {
            return Err("Runtime physical lane not covered".into());
        }
        let lane = (mask & ((1 << (key % 4)) - 1)).count_ones() as usize;
        events.push(Event::Issue {
            stage,
            program: usize::from(key / 4),
            lane,
            plane,
            packet,
        });
        Ok(())
    }
    pub(super) fn step_packet_port(
        &mut self,
        offer: Option<(usize, Arc<Program>)>,
        ce: bool,
        ready: bool,
        packet_issue_ready: bool,
        masks: &[u8; 16],
    ) -> Result<Step, String> {
        #[cfg(test)]
        let _counted_exclusion = counted_call_guard::Scope::enter();
        if self.stats.cycles >= self.hardware.max_cycles {
            return Err("bound watchdog".into());
        }
        let ingress_ready = offer.as_ref().is_some_and(|(id, p)| {
            *id == usize::from(p.input.quad_id) && self.input_ready(p.input.quad_id)
        });
        let offered = offer.as_ref().map(|(id, _)| *id);
        let pre_work = self.work_count;
        let pre_coordinates = self.coordinate_count;
        let pre_packets = self.packets.inflight();
        let old_input = self.ready.front().copied();
        let old_output = self.coefficient.output();
        let mut events = vec![];
        let mut accepted = false;
        let t = self.stats.enabled;
        self.stats.cycles += 1;
        let mut member_input = None;
        let mut packet_input = None;
        let mut plane_capture = None;
        if ce {
            self.stats.enabled += 1;
            // A Pool64 old reservation guarantees W, independent of G32.
            if let Some(payload) = self.packets.output() {
                if !ready {
                    return Err("Runtime reserved packet destination not ready".into());
                }
                let quad = ((payload >> 66) & 15) as u8;
                let completion = self.completion[usize::from(quad)]
                    .as_mut()
                    .ok_or("Runtime packet completion owner")?;
                completion.left = completion
                    .left
                    .checked_sub(1)
                    .ok_or("Runtime completion underflow")?;
                self.stats.packets += 1;
                events.push(Event::Packet {
                    program: usize::from(quad),
                    payload,
                });
                if completion.left == 0 {
                    self.release(quad, &mut events)?;
                }
            }
            let consumer_ready = packet_issue_ready && pre_packets < self.hardware.packet_credits;
            #[cfg(test)]
            let consumer_ready = consumer_ready && self.packet_ready;
            if consumer_ready {
                if let Some((member, tap)) = self.work.front() {
                    packet_input = Some(runtime_packet::Input { member, tap });
                    Self::issue(
                        &mut events,
                        "packet",
                        member.key(),
                        masks[usize::from(member.key() / 4)],
                        usize::from(member.raw("fine") == 0),
                        // Numerical tap and emitted ordinal remain distinct.
                        (member.emit() & ((1 << tap) - 1)).count_ones() as usize,
                    )?;
                }
            }
            let member_ready = self.members.inflight() < (self.binding.plane.span() + 2) as usize;
            #[cfg(test)]
            let member_ready = member_ready && self.membership_ready;
            if member_ready {
                if let Some(output) = old_output {
                    let which = if self.ready_plane == 0 && output.weights[0][0] == 0 {
                        1
                    } else {
                        self.ready_plane
                    };
                    if output.weights[which][0] == 0 {
                        return Err("Runtime inactive ready plane".into());
                    }
                    member_input = Some(runtime_membership::Input {
                        weights: output.weights[which],
                        coordinates: output.metadata.coordinates[which],
                        slot: output.metadata.slot,
                        level: output.metadata.levels[which],
                        key: output.metadata.key,
                        fine: which == 0,
                        last_fine: output.metadata.last_fine,
                    });
                    Self::issue(
                        &mut events,
                        "membership",
                        output.metadata.key,
                        masks[usize::from(output.metadata.key / 4)],
                        which,
                        0,
                    )?;
                    plane_capture = Some((output, which));
                }
            }
        }
        // Operand capture succeeds before any source cursor/ACK can change.
        let _packet = self.packets.tick(ce, packet_input)?;
        #[cfg(test)]
        let packet_word = _packet;
        let write = self.members.tick(ce, member_input)?;
        let capture = packet_input.is_some();
        let work_edge = self.work.tick(ce, write, capture)?;
        if work_edge.ack {
            self.work_count = self.work_count.checked_sub(1).ok_or("Runtime Work ACK")?;
        }
        let output_ready =
            plane_capture.is_some_and(|(o, which)| which == 1 || o.weights[1][0] == 0);
        let coefficient = self
            .coefficient
            .tick(coefficient::Tick {
                ce,
                input: old_input,
                output_ready,
                work_available: (self.hardware.work_credits - pre_work).min(16) as u8,
            })
            .map_err(|e| format!("coefficient: {e:?}"))?;
        if coefficient.accepted {
            let input = self.ready.pop_front().ok_or("Runtime coordinate owner")?;
            if Some(input) != old_input {
                return Err("Runtime coordinate capture mismatch".into());
            }
            self.coordinate_count -= 1;
            self.work_count += usize::from(coefficient.work_reserved);
            Self::issue(
                &mut events,
                "coefficient",
                input.metadata.key,
                masks[usize::from(input.metadata.key / 4)],
                0,
                0,
            )?;
        }
        if plane_capture.is_some() {
            self.ready_plane = if output_ready { 0 } else { 1 };
        }
        if coefficient.consumed != output_ready {
            return Err("Runtime coefficient row handshake".into());
        }
        if ce {
            // Consumers only saw the old coordinate ready; no registered-cut bypass.
            if let Some(coordinate) = self
                .coordinates
                .pop_front_if(|c| t - c.issue > self.binding.coordinate.span())
            {
                self.ready.push_back(coordinate.input);
            }
            if t.is_multiple_of(self.binding.coordinate.ii())
                && pre_coordinates < self.hardware.coordinate_credits
            {
                let slot = self.order.iter().copied().find(|&s| {
                    let c = self.slots[s].as_ref().unwrap();
                    c.lod.is_some_and(|u| t - u > self.binding.lod.span() + 1)
                        && c.next < c.source.preparation.lanes.len()
                });
                if let Some(slot) = slot {
                    let c = self.slots[slot].as_mut().unwrap();
                    let ordinal = c.next;
                    let lane = &c.source.preparation.lanes[ordinal];
                    let lod = &c.source.preparation.lod;
                    let coord = &lane.coordinate;
                    let quad = c.source.input.quad_id;
                    let input = coefficient::Input {
                        parents: std::array::from_fn(|w| lod.raw(&format!("parent{w}")) as u16),
                        fractions: std::array::from_fn(|w| {
                            std::array::from_fn(|a| coord.raw(&format!("f{w}.{a}")) as u8)
                        }),
                        nearest: lod.raw("nearest") != 0,
                        metadata: coefficient::Metadata {
                            coordinates: std::array::from_fn(|w| {
                                std::array::from_fn(|i| {
                                    coord.raw(&format!("t{w}.{}.{}", i / 2, i % 2)) as u16
                                })
                            }),
                            levels: std::array::from_fn(|w| lod.raw(&format!("n{w}")) as u8),
                            slot: lod.raw("slot") as u8,
                            key: quad * 4 + lane.lane,
                            last_fine: lod.raw("last_fine") != 0,
                        },
                    };
                    c.next += 1;
                    let last = c.next == c.source.preparation.lanes.len();
                    self.coordinates.push_back(Coordinate { input, issue: t });
                    self.coordinate_count += 1;
                    events.push(Event::Issue {
                        stage: "coordinate",
                        program: usize::from(quad),
                        lane: ordinal,
                        plane: 0,
                        packet: 0,
                    });
                    if last && self.hardware.release_after_capture {
                        self.shared_release(slot, &mut events)?;
                    }
                }
            }
            let empty: Vec<_> = self
                .order
                .iter()
                .copied()
                .filter(|&s| {
                    let c = self.slots[s].as_ref().unwrap();
                    c.source.preparation.lanes.is_empty()
                        && c.lod.is_some_and(|u| t - u > self.binding.lod.span() + 1)
                })
                .collect();
            for slot in empty {
                let quad = self.slots[slot].as_ref().unwrap().source.input.quad_id;
                if self.hardware.release_after_capture {
                    self.shared_release(slot, &mut events)?;
                }
                self.release(quad, &mut events)?;
            }
            for (stage, ii) in [
                ("lod", self.binding.lod.ii()),
                ("derivative", self.binding.derivative.ii()),
            ] {
                if t.is_multiple_of(ii) {
                    let slot = self.order.iter().copied().find(|&s| {
                        let c = self.slots[s].as_ref().unwrap();
                        if stage == "lod" {
                            c.lod.is_none()
                                && c.d.is_some_and(|u| t - u > self.binding.derivative.span())
                        } else {
                            c.d.is_none()
                        }
                    });
                    if let Some(slot) = slot {
                        let c = self.slots[slot].as_mut().unwrap();
                        if stage == "lod" {
                            c.lod = Some(t);
                        } else {
                            c.d = Some(t);
                        }
                        events.push(Event::Issue {
                            stage,
                            program: usize::from(c.source.input.quad_id),
                            lane: 0,
                            plane: 0,
                            packet: 0,
                        });
                    }
                }
            }
            if ingress_ready {
                let (_, source) = offer.ok_or("Runtime ingress offer")?;
                if !Arc::ptr_eq(&source.binding, &self.binding) {
                    return Err("binding source identity".into());
                }
                let quad = source.input.quad_id;
                let slot = self
                    .slots
                    .iter()
                    .position(Option::is_none)
                    .ok_or("Runtime context credit")?;
                self.completion[usize::from(quad)] = Some(Completion {
                    slot,
                    left: source.preparation.payloads.len(),
                });
                self.live |= 1 << quad;
                self.slots[slot] = Some(Context {
                    source,
                    next: 0,
                    d: None,
                    lod: None,
                });
                self.order.push_back(slot);
                self.stats.accepted += 1;
                accepted = true;
                events.push(Event::Accept {
                    program: usize::from(quad),
                    slot,
                });
            }
        }
        let materialized = self.work.snapshot().materialized;
        if self.coordinate_count > self.hardware.coordinate_credits
            || self.coordinate_count != self.coordinates.len() + self.ready.len()
            || self.work_count > self.hardware.work_credits
            || materialized + self.members.inflight() > self.work_count
            || self.packets.inflight() > self.hardware.packet_credits
            || self.members.inflight() > (self.binding.plane.span() + 2) as usize
        {
            return Err("Runtime preparation ownership bound".into());
        }
        self.stats.peak_contexts = self
            .stats
            .peak_contexts
            .max(self.slots.iter().filter(|s| s.is_some()).count());
        self.stats.peak_live = self.stats.peak_live.max(self.live.count_ones() as usize);
        self.stats.peak_coordinates = self.stats.peak_coordinates.max(self.coordinate_count);
        self.stats.peak_work = self.stats.peak_work.max(self.work_count);
        self.stats.peak_packet = self.stats.peak_packet.max(self.packets.inflight());
        self.stats.blocked += u64::from(ce && !ready && self.packets.inflight() != 0);
        #[cfg(test)]
        {
            self.trace = Trace {
                work: work_edge,
                coefficient: Some(coefficient),
                pre_work,
                offer_cost: old_input.map_or(0, |i| i.parents.iter().filter(|&&p| p != 0).count()),
                plane: plane_capture,
                packet: packet_word,
                member_input,
                member_banks: self.members.banks(),
                packet_banks: self.packets.banks(),
            };
        }
        Ok(Step {
            cycle: self.stats.cycles,
            ce,
            ready,
            packet_issue_ready,
            offered,
            accepted,
            events,
        })
    }
}
