//! Private live D/LOD/coefficient/membership/Work/packet executor. Coordinate
//! consumes U then V through one actual checked-domain axis kernel.
use super::{
    compact_transport::Work,
    control::{Event, Hardware, Stats, Step},
    *,
};
use crate::texture::emu::coefficient::{self, compact_stage::CompactCoefficientEmu};
use crate::texture::emu::{
    context_store::{self, Raw},
    coordinate::{self, compact_axis::CompactCoordinateEmu},
    coordinate_store,
    derivative::{self, DerivativeEmu},
    lod::{self, LodEmu},
    uv_store,
};

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

// Accepted raw UV is serialized to SDP36 and also read by D from one head.
// Coordinate reads low16 slices from RAM; D never writes wrapped UV back.
use super::derived_reference_frontend as runtime_frontend;
use runtime_frontend::Metadata;
struct Completion {
    slot: usize,
    remaining: u8,
}

#[cfg(test)]
#[derive(Default)]
pub(super) struct Trace {
    pub frontend_before: Option<runtime_frontend::Controller>,
    pub frontend_tick: runtime_frontend::Tick,
    pub frontend_ports: runtime_frontend::Ports,
    pub metadata_tick: context_store::Tick,
    pub metadata_edge: context_store::Edge,
    pub uv: uv_store::Edge,
    pub uv_tick: uv_store::Tick,
    pub coordinate_store_tick: coordinate_store::Tick,
    pub coordinate_store_edge: coordinate_store::Edge,
    pub d_capture: Option<derivative::Output>,
    pub l_capture: Option<lod::Output>,
    pub d_edge: Option<derivative::Edge>,
    pub l_edge: Option<derivative::Edge>,
    pub c_edge: Option<coordinate::SharedEdge>,
    pub d_input: Option<derivative::Input>,
    pub l_input: Option<lod::Input>,
    pub c_owner: Option<(u8, usize)>,
    pub c_input: Option<coordinate::Input>,
    pub coordinate: Option<coefficient::Input>,
    pub work: compact_transport::Edge,
    pub work_input: Option<compact_transport::Member>,
    pub coefficient: Option<coefficient::Step>,
    pub coefficient_tick: Option<coefficient::Tick>,
    pub pre_work: usize,
    pub offer_cost: usize,
    pub plane: Option<(coefficient::Output, usize)>,
    pub packet: Option<i128>,
    pub member_input: Option<runtime_membership::Input>,
    pub member_banks: [Option<u128>; 7],
    pub packet_banks: [Option<u128>; 9],
}
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct OrderedState {
    pub cursors: Vec<usize>,
    pub contexts: Vec<Option<(u8, usize)>>,
    pub uv: Vec<[i64; 8]>,
    pub coordinates: Vec<Option<(u8, bool)>>,
}
/// Combinational outcomes of this edge, not a retained owner or extra pipeline.
pub(super) struct PacketPortStep {
    pub preparation: Step,
    pub packet_key: Option<u8>,
}

pub(super) struct Machine {
    pub binding: Arc<Binding>,
    hardware: Hardware,
    pub stats: Stats,
    frontend: runtime_frontend::Controller,
    metadata: context_store::Store,
    uv: uv_store::Store,
    live: u16,
    completion: [Option<Completion>; 16],
    coordinate_store: coordinate_store::Store,
    derivative: DerivativeEmu,
    lod: LodEmu,
    coordinate: CompactCoordinateEmu,
    coefficient: CompactCoefficientEmu,
    ready_plane: usize,
    members: compact_membership::Pipeline,
    work: Work,
    packets: compact_packet::Pipeline,
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
    pub(super) fn frontend_state(&self) -> runtime_frontend::Controller {
        self.frontend.clone()
    }
    pub(super) fn packet_owner_read(&self) -> bool {
        self.packets.owner_read()
    }
    #[cfg(test)]
    pub(super) fn ordered_state(&self) -> OrderedState {
        OrderedState {
            cursors: vec![
                self.frontend.allocate,
                self.frontend.reclaim,
                self.frontend.occupied,
                self.frontend.d_head,
                self.frontend.raw_count,
                self.frontend.d_owner.unwrap_or(self.hardware.contexts),
                self.frontend.l_head,
                self.frontend.derived_count,
                self.frontend.l_owner.unwrap_or(self.hardware.contexts),
                self.frontend.c_head,
                self.frontend.lod_count,
                self.coordinate_store.state().write,
                self.coordinate_store.state().returned,
                self.coordinate_store.state().read,
                self.coordinate_store.state().count,
                self.coordinate_store.inflight(),
            ],
            contexts: self
                .frontend
                .slots
                .iter()
                .enumerate()
                .map(|(slot, c)| {
                    c.as_ref().map(|v| {
                        (
                            self.metadata
                                .diagnostic_quad(slot, v.metadata.diagnostic_stage()),
                            v.next,
                        )
                    })
                })
                .collect(),
            uv: self.uv.rows(),
            coordinates: self.coordinate_store.rows(),
        }
    }
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
    pub(super) fn work_state(&self) -> compact_transport::Snapshot {
        self.work.snapshot()
    }
    #[cfg(test)]
    pub(super) fn counts(&self) -> (usize, usize, usize) {
        (
            self.work_count,
            self.coordinate_store.state().count,
            self.members.inflight(),
        )
    }
    pub(super) fn new(binding: Arc<Binding>, hardware: Hardware) -> Result<Self, String> {
        hardware.validate()?;
        let (d, l, c) = calendars(&binding)?;
        // Each producer is captured from old state on its return edge. The
        // consumer issues on the next edge, at a fixed phase chosen at reset.
        let lod_offset = (u32::from(d.span()) + 1) % l.ii();
        let l = l.with_issue_offset(lod_offset)?;
        // UV reserves reads on odd phases and consumes their old returns on
        // even phases. C keeps that fixed physical port calendar and waits for
        // old Ready metadata; a later D return must not invert the phases.
        let c = c.with_issue_offset(0)?;
        Ok(Self {
            binding,
            hardware,
            stats: Stats::default(),
            frontend: runtime_frontend::Controller::new(
                hardware.contexts,
                hardware.release_after_capture,
            )?,
            metadata: context_store::Store::new_lod_ready(hardware.contexts)?,
            uv: uv_store::Store::new(hardware.contexts)?,
            live: 0,
            completion: std::array::from_fn(|_| None),
            coordinate_store: coordinate_store::Store::new_compact(hardware.coordinate_credits)?,
            derivative: DerivativeEmu::new(d)?,
            lod: LodEmu::new(l)?,
            coordinate: CompactCoordinateEmu::new(c)?,
            coefficient: CompactCoefficientEmu::new(hardware.max_cycles)
                .map_err(|e| format!("coefficient: {e:?}"))?,
            ready_plane: 0,
            members: compact_membership::Pipeline::with_short_alignment(true),
            work: Work::new(hardware.work_credits)?,
            packets: compact_packet::Pipeline::with_short_alignment(true),
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
        quad < 16
            && self.live >> quad & 1 == 0
            && self.frontend.occupied < self.hardware.contexts
            && self.uv.input_ready()
    }
    pub(super) fn idle(&self) -> bool {
        self.live == 0
            && self.coordinate_store.state().count == 0
            && self.work_count == 0
            && self.packets.inflight() == 0
            && self.coefficient.idle()
            && self.derivative.idle()
            && self.lod.idle()
            && self.coordinate.idle()
            && self.frontend.occupied == 0
            && self.frontend.raw_count == 0
            && self.frontend.derived_count == 0
            && self.frontend.lod_count == 0
            && self.frontend.d_owner.is_none()
            && self.frontend.l_owner.is_none()
            && self.coordinate_store.inflight() == 0
            && self.uv.idle()
    }
    fn retire_context(&self, slot: usize, retire: &mut u8) -> Result<(), String> {
        if self.frontend.slots.get(slot).is_none_or(Option::is_none)
            || self.frontend.reclaimable[slot]
            || *retire >> slot & 1 != 0
        {
            return Err("Runtime duplicate context retirement".into());
        }
        *retire |= 1 << slot;
        Ok(())
    }
    fn release(
        &mut self,
        quad: u8,
        events: &mut Vec<Event>,
        retire: &mut u8,
    ) -> Result<(), String> {
        let completion = self.completion[usize::from(quad)]
            .take()
            .ok_or("Runtime completion release owner")?;
        if completion.remaining != 0 {
            return Err("Runtime release precedes actual last lane packets".into());
        }
        self.live &= !(1 << quad);
        if !self.hardware.release_after_capture {
            self.retire_context(completion.slot, retire)?;
        }
        self.stats.released += 1;
        events.push(Event::Release {
            program: usize::from(quad),
        });
        Ok(())
    }
    fn issue(
        &self,
        events: &mut Vec<Event>,
        stage: &'static str,
        key: u8,
        plane: usize,
        packet: usize,
    ) -> Result<(), String> {
        // The existing preparation completion owner validates this live lane.
        // No accepted-coverage copy or ordinal selector participates in capture.
        // This host invariant check is not a multi-read SRAM port allocation;
        // physical stage legality must follow the retained local owner/valid.
        if key >= 64
            || self.completion[usize::from(key / 4)]
                .as_ref()
                .is_none_or(|c| c.remaining >> (key % 4) & 1 == 0)
        {
            return Err("Runtime physical lane not covered".into());
        }
        events.push(Event::KeyIssue {
            stage,
            key,
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
    ) -> Result<PacketPortStep, String> {
        if self.stats.cycles >= self.hardware.max_cycles {
            return Err("bound watchdog".into());
        }
        let ingress_ready = offer
            .as_ref()
            .is_some_and(|p| self.input_ready(p.header.quad));
        let offered = offer.as_ref().map(|p| usize::from(p.header.quad));
        #[cfg(test)]
        let frontend_before = self.frontend.clone();
        let pre_work = self.work_count;
        let pre_coordinates = self.coordinate_store.state().count;
        let pre_packets = self.packets.inflight();
        let old_input = self.coordinate_store.input();
        let old_output = self.coefficient.output();
        let old_derivative = self.derivative.output()?;
        let old_lod = self.lod.output()?;
        let old_coordinate = self.coordinate.output()?;
        let mut frontend_ports = runtime_frontend::Ports {
            ce,
            admit: ce && ingress_ready,
            d_zero: self.derivative.phase() == 0,
            l_zero: self.lod.phase() == 0,
            d_return: old_derivative.is_some(),
            l_return: old_lod.is_some(),
            uv_d: self.uv.derivative().map(|v| v.0),
            uv_odd: self.derivative.phase() & 1 != 0,
            uv_empty: self.uv.coordinate_empty(),
            uv_c: self.uv.coordinate().map(|v| (v.0.slot, v.0.lane)),
            c_zero: u64::from(self.coordinate.phase()).is_multiple_of(self.binding.coordinate.ii()),
            c_credit: pre_coordinates < self.hardware.coordinate_credits,
            ..Default::default()
        };
        // Validate all metadata accesses from old controls before any pipeline,
        // payload bank or ownership transition. Returned words are combinational
        // wires; they do not become an extra persistent metadata head.
        let mut metadata_tick = context_store::Tick {
            ce,
            phase: self.derivative.phase(),
            ..Default::default()
        };
        if ce {
            let reads = self.frontend.reads(
                ce,
                frontend_ports.d_zero,
                frontend_ports.l_zero,
                frontend_ports.uv_d,
            );
            metadata_tick.raw_issue = reads.raw;
            metadata_tick.derived_issue = reads.derived;
            metadata_tick.ready_read = reads.ready;
            metadata_tick.d_return = old_derivative
                .map(|v| {
                    self.frontend
                        .d_owner
                        .map(|a| (a, lod::Input::from(v)))
                        .ok_or("Runtime D return owner")
                })
                .transpose()?;
            metadata_tick.l_return = old_lod
                .map(|v| {
                    self.frontend
                        .l_owner
                        .map(|a| (a, v))
                        .ok_or("Runtime LOD return owner")
                })
                .transpose()?;
            if ingress_ready {
                let i = offer.ok_or("Runtime ingress offer")?;
                metadata_tick.admission = Some((
                    self.frontend.allocate,
                    Raw {
                        bias: i.bias,
                        force_coarsest: i.force_coarsest,
                        header: i.header,
                    },
                ));
                metadata_tick.control_bad |= self
                    .frontend
                    .slots
                    .get(self.frontend.allocate)
                    .is_none_or(Option::is_some);
            }
            for (address, stage) in [
                (metadata_tick.raw_issue, Metadata::Raw),
                (metadata_tick.d_return.map(|x| x.0), Metadata::Raw),
                (metadata_tick.derived_issue, Metadata::Derived),
                (metadata_tick.l_return.map(|x| x.0), Metadata::Derived),
                (metadata_tick.ready_read, Metadata::Ready),
            ] {
                if let Some(a) = address {
                    metadata_tick.control_bad |= self
                        .frontend
                        .slots
                        .get(a)
                        .and_then(Option::as_ref)
                        .is_none_or(|c| c.metadata != stage);
                }
            }
            metadata_tick.control_bad |=
                metadata_tick.raw_issue.is_some() && self.frontend.d_owner.is_some();
            metadata_tick.control_bad |=
                metadata_tick.derived_issue.is_some() && self.frontend.l_owner.is_some();
        }
        let metadata_edge = self.metadata.prepare(metadata_tick)?;
        if let Some(slot) = metadata_tick.raw_issue {
            if self.uv.derivative().map(|v| v.0) != Some(slot) {
                return Err("Runtime UV D owner mismatch".into());
            }
        }
        let ready_metadata = metadata_edge.reads[2].map(|(_, v)| context_store::unpack_ready(v));
        frontend_ports.c_mask = ready_metadata.map_or(0, |v| v.mask);
        let mut frontend_tick = self.frontend.commands(frontend_ports)?;
        if let Some(l) = ready_metadata.filter(|l| l.mask != 0) {
            let lane = self
                .frontend
                .lane(l.mask)?
                .ok_or("Runtime actual covered lane")?
                .lane;
            if self.coordinate.phase() == 0 && pre_coordinates < self.hardware.coordinate_credits {
                if let Some((owner, _)) = self.uv.coordinate() {
                    if owner
                        != (uv_store::Coordinate {
                            slot: self.frontend.c_head,
                            lane: usize::from(lane),
                        })
                    {
                        return Err("Runtime UV Coordinate owner mismatch".into());
                    }
                }
            }
        }
        let mut events = vec![];
        let mut accepted = false;
        self.stats.cycles += 1;
        let mut member_input = None;
        let mut packet_input = None;
        let mut plane_capture = None;
        let mut derivative_input = None;
        let mut lod_input = None;
        let mut coordinate_input = None;
        let mut coordinate_store_tick = coordinate_store::Tick {
            ce,
            returned: old_coordinate.filter(|_| ce),
            ..Default::default()
        };
        let mut uv_tick = uv_store::Tick {
            ce,
            phase: self.derivative.phase(),
            ..Default::default()
        };
        #[cfg(test)]
        let mut coordinate_capture = None;
        #[cfg(test)]
        let mut coordinate_owner = None;
        if ce {
            self.stats.enabled += 1;
            #[cfg(test)]
            if let Some(out) = old_coordinate {
                coordinate_capture = self.coordinate_store.return_input(out);
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
                    self.release(quad, &mut events, &mut frontend_tick.retire)?;
                }
            }
            let consumer_ready = packet_issue_ready && pre_packets < self.hardware.packet_credits;
            #[cfg(test)]
            let consumer_ready = consumer_ready && self.packet_ready;
            if consumer_ready {
                if let Some((member, tap)) = self.work.front() {
                    packet_input = Some(compact_packet::Input { member, tap });
                    self.issue(
                        &mut events,
                        "packet",
                        member.key(),
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
                    self.issue(&mut events, "membership", output.metadata.key, which, 0)?;
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
            let input = old_input.ok_or("Runtime coordinate owner")?;
            coordinate_store_tick.take = true;
            self.work_count += usize::from(coefficient.work_reserved);
            self.issue(&mut events, "coefficient", input.metadata.key, 0, 0)?;
        }
        if plane_capture.is_some() {
            self.ready_plane = if output_ready { 0 } else { 1 };
        }
        if coefficient.consumed != output_ready {
            return Err("Runtime coefficient row handshake".into());
        }
        if ce {
            // Each stage consumes only its ordered old queue head. Queue counts
            // cover disjoint intervals of the same bounded context ring.
            if self.frontend.lod_count != 0 {
                let slot = self.frontend.c_head;
                let c = self.frontend.slots[slot]
                    .as_ref()
                    .ok_or("Runtime Coordinate head")?;
                let Metadata::Ready = c.metadata else {
                    return Err("Runtime LOD head not returned".into());
                };
                let l = ready_metadata.ok_or("Runtime metadata Ready read")?;
                let quad = l.quad;
                if l.mask == 0 {
                    self.release(quad, &mut events, &mut frontend_tick.retire)?;
                } else {
                    let selected = self
                        .frontend
                        .lane(l.mask)?
                        .ok_or("Runtime actual covered lane")?;
                    let ordinal = selected.ordinal;
                    let lane = selected.lane;
                    if frontend_ports.uv_odd && frontend_ports.uv_empty {
                        uv_tick.coordinate = Some(uv_store::Coordinate {
                            slot,
                            lane: usize::from(lane),
                        });
                    }
                    if frontend_ports.c_zero
                        && frontend_ports.c_credit
                        && frontend_ports.uv_c.is_some()
                    {
                        let (owner, uv) = self.uv.coordinate().unwrap();
                        if owner
                            != (uv_store::Coordinate {
                                slot,
                                lane: usize::from(lane),
                            })
                        {
                            return Err("Runtime UV Coordinate owner mismatch".into());
                        }
                        uv_tick.take_coordinate = true;
                        let v = l.context;
                        coordinate_input = Some(coordinate::Input {
                            uv,
                            shift: v.shift,
                            nearest: v.nearest,
                            halve: v.halve,
                            side: v.side,
                        });
                        coordinate_store_tick.issue = Some(coordinate_store::Owner {
                            parents: v.parents,
                            nearest: v.nearest,
                            levels: v.levels,
                            slot: l.slot,
                            key: quad * 4 + lane,
                            last_fine: v.last_fine,
                        });
                        #[cfg(test)]
                        {
                            coordinate_owner = Some((quad, ordinal));
                        }
                        events.push(Event::Issue {
                            stage: "coordinate",
                            program: usize::from(quad),
                            lane: ordinal,
                            plane: 0,
                            packet: 0,
                        });
                    }
                }
            }
            if metadata_tick.derived_issue.is_some() {
                let slot = self.frontend.l_head;
                let c = self.frontend.slots[slot]
                    .as_ref()
                    .ok_or("Runtime LOD head")?;
                let Metadata::Derived = c.metadata else {
                    return Err("Runtime D head not returned".into());
                };
                let input = context_store::unpack_derived(
                    metadata_edge.reads[1]
                        .ok_or("Runtime metadata Derived read")?
                        .1 as u64,
                );

                lod_input = Some(input);
                events.push(Event::Issue {
                    stage: "lod",
                    program: usize::from(input.header.quad),
                    lane: 0,
                    plane: 0,
                    packet: 0,
                });
            }
            if metadata_tick.raw_issue.is_some() {
                let slot = self.frontend.d_head;
                let c = self.frontend.slots[slot].as_ref().ok_or("Runtime D head")?;
                let Metadata::Raw = c.metadata else {
                    return Err("Runtime raw head not admitted".into());
                };
                let Raw {
                    bias,
                    force_coarsest,
                    header,
                } = Raw::unpack(
                    metadata_edge.reads[0].ok_or("Runtime metadata Raw read")?.1 as u64,
                );

                let (owner, uv) = self.uv.derivative().unwrap();
                if owner != slot {
                    return Err("Runtime UV D owner mismatch".into());
                }
                uv_tick.take_derivative = true;
                derivative_input = Some(derivative::Input {
                    uv,
                    bias,
                    force_coarsest,
                    header,
                });
                events.push(Event::Issue {
                    stage: "derivative",
                    program: usize::from(header.quad),
                    lane: 0,
                    plane: 0,
                    packet: 0,
                });
            }
            // Queue registers consume the actual leaf handshakes. This is
            // the only queue commit; no arithmetic return can fund an old issue.
            let (next_frontend, released) = self.frontend.prepare(frontend_tick)?;
            if let Some((slot, stage)) = released {
                events.push(Event::SharedRelease {
                    program: usize::from(
                        self.metadata
                            .diagnostic_quad(slot, stage.diagnostic_stage()),
                    ),
                    slot,
                });
            }
            self.frontend = next_frontend;
            if ingress_ready {
                let captured = offer.ok_or("Runtime ingress offer")?;
                let quad = captured.header.quad;
                let slot = metadata_tick
                    .admission
                    .ok_or("Runtime ingress metadata owner")?
                    .0;
                self.completion[usize::from(quad)] = Some(Completion {
                    slot,
                    remaining: captured.header.mask,
                });
                self.live |= 1 << quad;
                uv_tick.admission = Some((slot, captured.uv));
                self.stats.accepted += 1;
                accepted = true;
                events.push(Event::Accept {
                    program: usize::from(quad),
                    slot,
                });
            }
        }
        let _coordinate_store_edge = self.coordinate_store.tick(coordinate_store_tick)?;
        let _uv_edge = self.uv.tick(uv_tick)?;
        let _d_edge = self.derivative.tick(ce, derivative_input)?;
        let _l_edge = self.lod.tick(ce, lod_input)?;
        let _c_edge = self.coordinate.tick(ce, coordinate_input)?;
        let materialized = self.work.snapshot().materialized;
        if self.coordinate_store.state().count > self.hardware.coordinate_credits
            || self.coordinate_store.inflight() > self.coordinate_store.state().count
            || self.frontend.raw_count
                + self.frontend.derived_count
                + self.frontend.lod_count
                + usize::from(self.frontend.d_owner.is_some())
                + usize::from(self.frontend.l_owner.is_some())
                > self.frontend.occupied
            || self.frontend.occupied > self.hardware.contexts
            || self.work_count > self.hardware.work_credits
            || materialized + self.members.inflight() > self.work_count
            || self.packets.inflight() > self.hardware.packet_credits
            || self.members.inflight() > (self.binding.plane.span() + 2) as usize
        {
            return Err("Runtime preparation ownership bound".into());
        }
        self.stats.peak_contexts = self.stats.peak_contexts.max(self.frontend.occupied);
        self.stats.peak_live = self.stats.peak_live.max(self.live.count_ones() as usize);
        self.stats.peak_coordinates = self
            .stats
            .peak_coordinates
            .max(self.coordinate_store.state().count);
        self.stats.peak_work = self.stats.peak_work.max(self.work_count);
        self.stats.peak_packet = self.stats.peak_packet.max(self.packets.inflight());
        self.stats.blocked += u64::from(ce && !ready && self.packets.inflight() != 0);
        self.metadata.commit(metadata_edge);
        #[cfg(test)]
        {
            frontend_ports.late_retire = frontend_tick.retire;
            self.trace = Trace {
                frontend_before: Some(frontend_before),
                frontend_tick,
                frontend_ports,
                metadata_tick,
                metadata_edge,
                uv: _uv_edge,
                uv_tick,
                coordinate_store_tick,
                coordinate_store_edge: _coordinate_store_edge,
                d_capture: old_derivative.filter(|_| ce),
                l_capture: old_lod.filter(|_| ce),
                d_edge: Some(_d_edge),
                l_edge: Some(_l_edge),
                c_edge: Some(coordinate::SharedEdge {
                    accepted: _c_edge.accepted,
                    axis: _c_edge.axis.map(|(i, a)| {
                        (
                            i,
                            coordinate::AxisResult {
                                fractions: a.fractions,
                                coordinates: a.coordinates,
                            },
                        )
                    }),
                }),
                d_input: derivative_input,
                l_input: lod_input,
                c_owner: coordinate_owner,
                c_input: coordinate_input,
                coordinate: coordinate_capture,
                work: work_edge,
                work_input: write,
                coefficient: Some(coefficient),
                coefficient_tick: Some(coefficient::Tick {
                    ce,
                    input: old_input,
                    output_ready,
                    work_available: (self.hardware.work_credits - pre_work).min(16) as u8,
                }),
                pre_work,
                offer_cost: old_input.map_or(0, |i| i.parents.iter().filter(|&&p| p != 0).count()),
                plane: plane_capture,
                packet: packet_word,
                member_input,
                member_banks: self.members.banks(),
                packet_banks: self.packets.banks(),
            };
        }
        Ok(PacketPortStep {
            packet_key: packet_input.map(|i| i.member.key()),
            preparation: Step {
                cycle: self.stats.cycles,
                ce,
                ready,
                packet_issue_ready,
                offered,
                accepted,
                events,
            },
        })
    }
}
