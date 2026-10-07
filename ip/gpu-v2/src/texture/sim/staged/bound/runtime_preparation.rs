//! Private live D/LOD/coefficient/membership/Work/packet executor. Coordinate
//! consumes captured scalar inputs through an actual packed register calendar.
use super::{
    control::{Event, Hardware, Stats, Step},
    transport::Work,
    *,
};
use crate::texture::emu::coefficient::{self, CoefficientEmu};
use crate::texture::emu::{
    coordinate::{self, CoordinateEmu},
    derivative::{self, DerivativeEmu},
    lod::{self, LodEmu},
};
use std::collections::VecDeque;

fn calendar(
    plan: &StagePlan,
    frame: &FrameReport,
    outputs: &[&str],
) -> Result<derivative::Calendar, String> {
    let fields = plan
        .packed_fields
        .iter()
        .map(|f| {
            let iterations = plan.packed.period / plan.ii();
            let mut lows = Vec::with_capacity(iterations as usize);
            for iteration in 0..iterations {
                lows.push(
                    plan.packed
                        .placements
                        .iter()
                        .find(|p| {
                            p.value == f.value
                                && p.source_low == f.source_low
                                && p.iteration == iteration
                        })
                        .ok_or("Runtime D/LOD missing placement")?
                        .low,
                );
            }
            Ok(derivative::Field {
                value: f.value,
                source_low: f.source_low,
                width: f.width,
                birth: u8::try_from(f.birth).map_err(|_| "Runtime D/LOD field birth")?,
                last_read: u8::try_from(f.last_read).map_err(|_| "Runtime D/LOD field last")?,
                lows,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    derivative::Calendar::from_structure(
        frame,
        &plan.times,
        &plan.cones,
        &plan.lowering.wiring_adds,
        fields,
        plan.packed.ff_bits,
        u8::try_from(plan.span()).map_err(|_| "Runtime D/LOD span")?,
        u32::try_from(plan.packed.period).map_err(|_| "Runtime D/LOD period")?,
        u32::try_from(plan.ii()).map_err(|_| "Runtime D/LOD ii")?,
        outputs,
    )
}
pub(super) fn calendars(
    b: &Binding,
) -> Result<
    (
        derivative::Calendar,
        derivative::Calendar,
        derivative::Calendar,
    ),
    String,
> {
    // Constructor-only structural extraction; no sampled nonliteral raw answer
    // survives Calendar::from_structure. This is outside the live-call guard.
    let mut q = QuadInput {
        force_coarsest: false,
        quad_id: 0,
        mask: 1,
        uv: [[0.003, 0.003]; 4],
        slot: 0,
        material_size_log2: 9,
        filter: Filter::Trilinear,
        lod_bias: 0.5,
    };
    q.uv[1][0] += 1.0 / 512.0;
    let p = prepare(
        &q,
        &[Slot {
            base_address: 4096,
            max_size_log2: 9,
            has_full_mip: true,
            valid: true,
        }],
    )
    .map_err(|e| format!("Runtime D/LOD structural source {e:?}"))?;
    Ok((
        calendar(
            &b.derivative,
            &p.derivative.frame,
            &[
                "uv0", "uv1", "uv2", "uv3", "uv4", "uv5", "uv6", "uv7", "slope", "bias", "quad",
                "mask", "slot", "max_n", "has_mip", "filter",
            ],
        )?,
        calendar(
            &b.lod,
            &p.lod.frame,
            &[
                "shift0",
                "nearest",
                "halve",
                "side0",
                "side1",
                "parent0",
                "parent1",
                "n0",
                "n1",
                "last_fine",
                "quad",
                "mask",
                "slot",
            ],
        )?,
        calendar(
            &b.coordinate,
            &p.lanes[0].coordinate.frame,
            coordinate::OUTPUTS,
        )?,
    ))
}

enum Payload {
    Raw(derivative::Input),
    Derived(derivative::Output),
    Ready { uv: [u32; 8], lod: lod::Output },
}
struct Context {
    next: usize,
    d: Option<u64>,
    lod: Option<u64>,
    payload: Payload,
}
impl Context {
    fn quad(&self) -> u8 {
        match &self.payload {
            Payload::Raw(v) => v.header.quad,
            Payload::Derived(v) => v.header.quad,
            Payload::Ready { lod, .. } => lod.quad,
        }
    }
    fn mask(&self) -> u8 {
        match &self.payload {
            Payload::Raw(v) => v.header.mask,
            Payload::Derived(v) => v.header.mask,
            Payload::Ready { lod, .. } => lod.mask,
        }
    }
}
/// Metadata captured when a lane enters the actual coordinate pipeline. The
/// wrapped taps and Q8 fractions are produced later by `CoordinateEmu`'s old
/// scalar registers; only these lod/identity values are retained alongside.
struct PendingCoordinate {
    parents: [u16; 2],
    nearest: bool,
    levels: [u8; 2],
    slot: u8,
    key: u8,
    last_fine: bool,
}
impl PendingCoordinate {
    fn finish(self, out: coordinate::Output) -> coefficient::Input {
        coefficient::Input {
            parents: self.parents,
            fractions: out.fractions,
            nearest: self.nearest,
            metadata: coefficient::Metadata {
                coordinates: out.coordinates,
                levels: self.levels,
                slot: self.slot,
                key: self.key,
                last_fine: self.last_fine,
            },
        }
    }
}
struct Completion {
    slot: usize,
    remaining: u8,
}

#[cfg(test)]
#[derive(Default)]
pub(super) struct Trace {
    pub d_capture: Option<derivative::Output>,
    pub l_capture: Option<lod::Output>,
    pub d_edge: Option<derivative::Edge>,
    pub l_edge: Option<derivative::Edge>,
    pub c_edge: Option<derivative::Edge>,
    pub d_input: Option<derivative::Input>,
    pub l_input: Option<lod::Input>,
    pub c_owner: Option<(u8, usize)>,
    pub coordinate: Option<coefficient::Input>,
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
    coordinate_pending: VecDeque<PendingCoordinate>,
    ready: VecDeque<coefficient::Input>,
    derivative: DerivativeEmu,
    lod: LodEmu,
    coordinate: CoordinateEmu,
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
    pub(super) fn d_lod_state(&self) -> (Vec<u64>, Vec<u64>, u8, u8) {
        (
            self.derivative.bank().to_vec(),
            self.lod.bank().to_vec(),
            self.derivative.phase(),
            self.lod.phase(),
        )
    }
    #[cfg(test)]
    pub(super) fn coordinate_state(&self) -> (Vec<u64>, u8) {
        (self.coordinate.bank().to_vec(), self.coordinate.phase())
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
        let (d, l, c) = calendars(&binding)?;
        Ok(Self {
            binding,
            hardware,
            stats: Stats::default(),
            slots: (0..hardware.contexts).map(|_| None).collect(),
            order: VecDeque::new(),
            live: 0,
            completion: std::array::from_fn(|_| None),
            coordinate_pending: VecDeque::new(),
            ready: VecDeque::new(),
            derivative: DerivativeEmu::new(d)?,
            lod: LodEmu::new(l)?,
            coordinate: CoordinateEmu::new(c)?,
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
            && self.derivative.idle()
            && self.lod.idle()
            && self.coordinate.idle()
            && self.coordinate_pending.is_empty()
            && self.ready.is_empty()
    }
    fn shared_release(&mut self, slot: usize, events: &mut Vec<Event>) -> Result<(), String> {
        let c = self.slots[slot]
            .take()
            .ok_or("Runtime context release owner")?;
        self.order.retain(|&s| s != slot);
        events.push(Event::SharedRelease {
            program: usize::from(c.quad()),
            slot,
        });
        Ok(())
    }
    fn release(&mut self, quad: u8, events: &mut Vec<Event>) -> Result<(), String> {
        let completion = self.completion[usize::from(quad)]
            .take()
            .ok_or("Runtime completion release owner")?;
        if completion.remaining != 0 {
            return Err("Runtime release precedes actual last lane packets".into());
        }
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
        offer: Option<derivative::Input>,
        ce: bool,
        ready: bool,
        packet_issue_ready: bool,
        masks: &[u8; 16],
    ) -> Result<Step, String> {
        if self.stats.cycles >= self.hardware.max_cycles {
            return Err("bound watchdog".into());
        }
        let ingress_ready = offer
            .as_ref()
            .is_some_and(|p| self.input_ready(p.header.quad));
        let offered = offer.as_ref().map(|p| usize::from(p.header.quad));
        let pre_work = self.work_count;
        let pre_coordinates = self.coordinate_count;
        let pre_packets = self.packets.inflight();
        let old_input = self.ready.front().copied();
        let old_output = self.coefficient.output();
        let old_derivative = self.derivative.output()?;
        let old_lod = self.lod.output()?;
        let old_coordinate = self.coordinate.output()?;
        let mut events = vec![];
        let mut accepted = false;
        let t = self.stats.enabled;
        self.stats.cycles += 1;
        let mut member_input = None;
        let mut packet_input = None;
        let mut plane_capture = None;
        let mut derivative_input = None;
        let mut lod_input = None;
        let mut coordinate_input = None;
        #[cfg(test)]
        let mut coordinate_capture = None;
        #[cfg(test)]
        let mut coordinate_owner = None;
        if ce {
            self.stats.enabled += 1;
            // The actual coordinate bank publishes at age SPAN. The captured
            // taps/weights enter the ready queue for the next coefficient edge.
            if let Some(out) = old_coordinate {
                let pending = self
                    .coordinate_pending
                    .pop_front()
                    .ok_or("Runtime coordinate result owner")?;
                let input = pending.finish(out);
                #[cfg(test)]
                {
                    coordinate_capture = Some(input);
                }
                self.ready.push_back(input);
            }
            // A Pool64 old reservation guarantees W, independent of G32.
            if let Some(payload) = self.packets.output() {
                if !ready {
                    return Err("Runtime reserved packet destination not ready".into());
                }
                let quad = ((payload >> 66) & 15) as u8;
                let completion = self.completion[usize::from(quad)]
                    .as_mut()
                    .ok_or("Runtime packet completion owner")?;
                let lane = ((payload >> 70) & 3) as u8;
                let bit = 1 << lane;
                if completion.remaining & bit == 0 {
                    return Err("Runtime packet for completed/uncovered lane".into());
                }
                if payload >> 65 & 1 != 0 {
                    completion.remaining &= !bit;
                }
                self.stats.packets += 1;
                events.push(Event::Packet {
                    program: usize::from(quad),
                    payload,
                });
                if completion.remaining == 0 {
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
            if t.is_multiple_of(self.binding.coordinate.ii())
                && pre_coordinates < self.hardware.coordinate_credits
            {
                let slot = self.order.iter().copied().find(|&s| {
                    let c = self.slots[s].as_ref().unwrap();
                    c.lod.is_some_and(|u| t - u > self.binding.lod.span() + 1)
                        && matches!(c.payload, Payload::Ready { .. })
                        && c.next < c.mask().count_ones() as usize
                });
                if let Some(slot) = slot {
                    let c = self.slots[slot].as_mut().unwrap();
                    let ordinal = c.next;
                    let Payload::Ready { uv, lod } = c.payload else {
                        return Err("Runtime actual LOD context not captured".into());
                    };
                    let lane = (0..4_u8)
                        .filter(|l| lod.mask >> l & 1 != 0)
                        .nth(ordinal)
                        .ok_or("Runtime actual covered lane")?;
                    let v = lod.context;
                    let quad = lod.quad;
                    // Actual wrapped Q16 operands and captured LOD context feed
                    // the coordinate register calendar; no counted body runs.
                    coordinate_input = Some(coordinate::Input {
                        uv: [uv[usize::from(lane) * 2], uv[usize::from(lane) * 2 + 1]],
                        shift: v.shift,
                        nearest: v.nearest,
                        halve: v.halve,
                        side: v.side,
                    });
                    self.coordinate_pending.push_back(PendingCoordinate {
                        parents: v.parents,
                        nearest: v.nearest,
                        levels: v.levels,
                        slot: lod.slot,
                        key: quad * 4 + lane,
                        last_fine: v.last_fine,
                    });
                    #[cfg(test)]
                    {
                        coordinate_owner = Some((quad, ordinal));
                    }
                    c.next += 1;
                    let last = c.next == c.mask().count_ones() as usize;
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
                    c.mask() == 0 && c.lod.is_some_and(|u| t - u > self.binding.lod.span() + 1)
                })
                .collect();
            for slot in empty {
                let quad = self.slots[slot].as_ref().unwrap().quad();
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
                                && matches!(c.payload, Payload::Derived(_))
                        } else {
                            c.d.is_none()
                        }
                    });
                    if let Some(slot) = slot {
                        let c = self.slots[slot].as_mut().unwrap();
                        if stage == "lod" {
                            let Payload::Derived(d) = c.payload else {
                                return Err("Runtime actual D context".into());
                            };
                            lod_input = Some(lod::Input::from(d));
                            c.lod = Some(t);
                        } else {
                            let Payload::Raw(d) = c.payload else {
                                return Err("Runtime actual D admission context".into());
                            };
                            derivative_input = Some(d);
                            c.d = Some(t);
                        }
                        events.push(Event::Issue {
                            stage,
                            program: usize::from(c.quad()),
                            lane: 0,
                            plane: 0,
                            packet: 0,
                        });
                    }
                }
            }
            // Capture after all old-state issue/consumer decisions. Shared387
            // rows replace their variant, rather than retain another payload.
            if let Some(d) = old_derivative {
                let slot = self
                    .order
                    .iter()
                    .copied()
                    .find(|s| {
                        self.slots[*s]
                            .as_ref()
                            .is_some_and(|c| c.quad() == d.header.quad)
                    })
                    .ok_or("Runtime D return owner")?;
                let c = self.slots[slot].as_mut().unwrap();
                if !matches!(c.payload, Payload::Raw(_))
                    || c.d != Some(t - u64::from(derivative::SPAN))
                {
                    return Err("Runtime D return cut".into());
                }
                c.payload = Payload::Derived(d);
            }
            if let Some(l) = old_lod {
                let slot = self
                    .order
                    .iter()
                    .copied()
                    .find(|s| self.slots[*s].as_ref().is_some_and(|c| c.quad() == l.quad))
                    .ok_or("Runtime LOD return owner")?;
                let c = self.slots[slot].as_mut().unwrap();
                let Payload::Derived(d) = c.payload else {
                    return Err("Runtime LOD return context".into());
                };
                if c.lod != Some(t - u64::from(lod::SPAN)) {
                    return Err("Runtime LOD return cut".into());
                }
                c.payload = Payload::Ready { uv: d.uv, lod: l };
            }
            if ingress_ready {
                let captured = offer.ok_or("Runtime ingress offer")?;
                let quad = captured.header.quad;
                let slot = self
                    .slots
                    .iter()
                    .position(Option::is_none)
                    .ok_or("Runtime context credit")?;
                self.completion[usize::from(quad)] = Some(Completion {
                    slot,
                    remaining: captured.header.mask,
                });
                self.live |= 1 << quad;
                self.slots[slot] = Some(Context {
                    next: 0,
                    d: None,
                    lod: None,
                    payload: Payload::Raw(captured),
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
        let _d_edge = self.derivative.tick(ce, derivative_input)?;
        let _l_edge = self.lod.tick(ce, lod_input)?;
        let _c_edge = self.coordinate.tick(ce, coordinate_input)?;
        let materialized = self.work.snapshot().materialized;
        if self.coordinate_count > self.hardware.coordinate_credits
            || self.coordinate_count != self.coordinate_pending.len() + self.ready.len()
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
                d_capture: old_derivative.filter(|_| ce),
                l_capture: old_lod.filter(|_| ce),
                d_edge: Some(_d_edge),
                l_edge: Some(_l_edge),
                c_edge: Some(_c_edge),
                d_input: derivative_input,
                l_input: lod_input,
                c_owner: coordinate_owner,
                coordinate: coordinate_capture,
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
