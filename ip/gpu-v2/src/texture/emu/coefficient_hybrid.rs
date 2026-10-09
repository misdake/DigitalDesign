//! Private default-profile candidate. Upstream and membership/packet arithmetic
//! remain counted replay; coefficient operands/results are live FF handshakes.
//! This does not replace Runtime or certify physical area/clock timing.
use super::{coefficient, color};
use crate::texture::{
    ports::{QuadInput, RefillPort, Slot},
    sim::{
        staged::{bound, Stage},
        timed,
    },
};
use std::sync::Arc;
mod counted;
mod tests;
const BOUND: u64 = 20_000;

/// Fixed host ring: capacity is enforced, including all in-flight ownership.
struct Ring<T, const N: usize> {
    rows: [Option<T>; N],
    read: usize,
    write: usize,
    len: usize,
}
impl<T, const N: usize> Ring<T, N> {
    fn new() -> Self {
        Self {
            rows: std::array::from_fn(|_| None),
            read: 0,
            write: 0,
            len: 0,
        }
    }
    fn front(&self) -> Option<&T> {
        self.rows[self.read].as_ref()
    }
    fn front_mut(&mut self) -> Option<&mut T> {
        self.rows[self.read].as_mut()
    }
    fn push(&mut self, value: T) -> Result<(), String> {
        if self.len == N || self.rows[self.write].is_some() {
            return Err("hybrid fixed ring full".into());
        }
        self.rows[self.write] = Some(value);
        self.write = (self.write + 1) % N;
        self.len += 1;
        Ok(())
    }
    fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let value = self.rows[self.read].take();
        self.read = (self.read + 1) % N;
        self.len -= 1;
        value
    }
}
fn raw(stage: &Stage, name: &str) -> i128 {
    stage
        .frame
        .outputs
        .iter()
        .find(|o| o.name == name)
        .expect("fixed output")
        .raw
}
// The exact92-bit membership result; no Stage/Source/packet vector in Work.
const FIELDS: [(&str, u8); 21] = [
    ("w0", 9),
    ("w1", 9),
    ("w2", 9),
    ("w3", 9),
    ("emit0", 1),
    ("emit1", 1),
    ("emit2", 1),
    ("emit3", 1),
    ("tx0", 7),
    ("tx1", 7),
    ("ty0", 7),
    ("ty1", 7),
    ("lx", 3),
    ("ly", 3),
    ("same_x", 1),
    ("same_y", 1),
    ("slot", 4),
    ("n", 4),
    ("quad", 4),
    ("lane", 2),
    ("fine", 1),
];
#[derive(Clone, Copy)]
struct Member(u128);
impl Member {
    fn capture(stage: &Stage) -> Self {
        let mut word = 0;
        let mut low = 0;
        for (name, bits) in FIELDS {
            word |= (raw(stage, name) as u128) << low;
            low += bits;
        }
        assert_eq!(low, 91);
        word |= (raw(stage, "final") as u128) << 91;
        Self(word)
    }
    fn raw(self, name: &str) -> i128 {
        if name == "final" {
            return ((self.0 >> 91) & 1) as i128;
        }
        let mut low = 0;
        for (field, bits) in FIELDS {
            if field == name {
                return ((self.0 >> low) & ((1 << bits) - 1)) as i128;
            }
            low += bits;
        }
        panic!("unknown member field {name}");
    }
    fn key(self) -> u8 {
        (self.raw("quad") * 4 + self.raw("lane")) as u8
    }
    fn emit(self) -> u8 {
        ((self.0 >> 36) & 15) as u8
    }
}
struct Context {
    source: Arc<bound::Preparation>,
    quad: u8,
    d: Option<u64>,
    lod: Option<u64>,
    next: usize,
}
struct Coordinate {
    input: coefficient::Input,
    issue: u64,
}
struct Membership {
    stage: Stage,
    issue: u64,
}
struct Work {
    member: Member,
    cursor: u8,
}
struct Packet {
    payload: i128,
    issue: u64,
}
#[derive(Clone, Copy, Default)]
struct Stimulus {
    control: timed::Control,
    membership_ready: bool,
    packet_ready: bool,
}
impl Stimulus {
    fn run() -> Self {
        Self {
            control: timed::Control::default(),
            membership_ready: true,
            packet_ready: true,
        }
    }
}
#[derive(Default)]
struct Counters {
    accepted: u64,
    poisoned: u64,
    products: u64,
    returns: u64,
    planes: u64,
    issues: u64,
    packets: u64,
    captures: u64,
    consumes: u64,
    reserved: u64,
    ack: u64,
    peak_work: u8,
    peak_coordinate: u8,
    peak_members: usize,
}
struct Edge {
    accepted: bool,
    results: Vec<color::Output>,
    cache: timed::Step,
    coefficient: coefficient::Step,
    base_ce: bool,
    pre_work: u8,
    post_work: u8,
    offer_cost: u8,
    ack: bool,
    packet: Option<i128>,
    plane: Option<(coefficient::Output, usize)>,
    issue_ordinal: Option<(u8, usize)>,
}
struct Hybrid {
    binding: Arc<bound::Binding>,
    coefficient: coefficient::CoefficientEmu,
    contexts: [Option<Context>; 8],
    order: Ring<usize, 8>,
    coordinates: Ring<Coordinate, 6>,
    coordinate_ready: Ring<coefficient::Input, 6>,
    members: Ring<Membership, 8>,
    work: Ring<Work, 16>,
    packets: Ring<Packet, 16>,
    coordinate_count: u8,
    work_count: u8,
    ready_plane: u8,
    completion: [Option<u8>; 16],
    masks: [u8; 16],
    remaining: [u8; 16],
    cache: timed::Machine,
    color: color::ColorEmu,
    color_input: Option<color::Input>,
    wall: u64,
    enabled: u64,
    faulted: bool,
    stats: Counters,
}
impl Hybrid {
    fn new(slots: &[Slot]) -> Result<Self, String> {
        let binding = bound::Binding::build()?;
        if binding.plane.span() + 2 > 8 {
            return Err("hybrid fixed membership pipeline8".into());
        }
        let cache = timed::Machine::new(
            slots.to_vec(),
            timed::Hardware {
                preparation: timed::PreparationMode::BoundStages,
                packet_storage: timed::PacketStorage::Pool64,
                prefetch: false,
                max_cycles: BOUND,
                max_quads: 1,
                ..Default::default()
            },
        )
        .map_err(|e| format!("cache: {e:?}"))?;
        Ok(Self {
            binding,
            coefficient: coefficient::CoefficientEmu::new(BOUND)
                .map_err(|e| format!("coefficient: {e:?}"))?,
            contexts: std::array::from_fn(|_| None),
            order: Ring::new(),
            coordinates: Ring::new(),
            coordinate_ready: Ring::new(),
            members: Ring::new(),
            work: Ring::new(),
            packets: Ring::new(),
            coordinate_count: 0,
            work_count: 0,
            ready_plane: 0,
            completion: [None; 16],
            masks: [0; 16],
            remaining: [0; 16],
            cache,
            color: color::ColorEmu::new(BOUND)?,
            color_input: None,
            wall: 0,
            enabled: 0,
            faulted: false,
            stats: Counters::default(),
        })
    }
    fn ready(&self, quad: u8) -> bool {
        quad < 16
            && !self.faulted
            && self.color_input.is_none()
            && self.remaining[usize::from(quad)] == 0
            && self.completion[usize::from(quad)].is_none()
            && self.contexts.iter().any(Option::is_none)
            && self.cache.external_ready(quad)
    }
    fn idle(&self) -> bool {
        self.completion.iter().all(Option::is_none)
            && self.coordinate_count == 0
            && self.work_count == 0
            && self.packets.len == 0
            && self.coefficient.idle()
            && self.color_input.is_none()
            && self.remaining == [0; 16]
            && self.cache.idle()
            && self.color.idle()
    }
    fn step<M: RefillPort>(
        &mut self,
        memory: &mut M,
        offer: Option<&QuadInput>,
        stimulus: Stimulus,
        poison: bool,
    ) -> Result<Edge, String> {
        if self.faulted || self.wall == BOUND {
            self.faulted = true;
            return Err("hybrid terminal/watchdog".into());
        }
        let result = self.advance(memory, offer, stimulus, poison);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }
    fn advance<M: RefillPort>(
        &mut self,
        memory: &mut M,
        offer: Option<&QuadInput>,
        stimulus: Stimulus,
        poison: bool,
    ) -> Result<Edge, String> {
        let caller = stimulus.control;
        // Eligible ingress compilation is replay, never a hardware arithmetic claim.
        let compiled = if caller.ce && offer.is_some_and(|q| self.ready(q.quad_id)) {
            let q = offer.unwrap();
            let (slots, hardware) = self.cache.external_context();
            let mut prep = bound::prepare(q, slots).map_err(|e| format!("prepare: {e:?}"))?;
            self.binding.audit(&prep)?;
            let checker = timed::Program::compile(q, slots, hardware)
                .map_err(|e| format!("checker: {e:?}"))?;
            if poison {
                for lane in &mut prep.lanes {
                    for output in &mut lane.coefficient.frame.outputs {
                        output.raw ^= 0x1ff;
                        self.stats.poisoned += 1;
                    }
                    for member in &mut lane.memberships {
                        for output in &mut member.frame.outputs {
                            if output.name.starts_with('w') {
                                output.raw ^= 0x1ff;
                                self.stats.poisoned += 1;
                            }
                        }
                    }
                    for plane in &mut lane.packets {
                        for packet in plane {
                            for output in &mut packet.frame.outputs {
                                output.raw ^= 1 << 28;
                                self.stats.poisoned += 1;
                            }
                        }
                    }
                }
            }
            Some((q, Arc::new(prep), checker))
        } else {
            None
        };
        self.wall += 1;
        let color = self.color.tick(color::Tick {
            ce: caller.ce,
            input: self.color_input,
            output_ready: caller.result_ready,
        })?;
        let mut results = vec![];
        for event in &color.events {
            if let color::Event::Commit(output) = event {
                let row = &mut self.remaining[usize::from(output.key / 4)];
                let bit = 1 << (output.key % 4);
                if *row & bit == 0 {
                    return Err("unowned public lane".into());
                }
                *row &= !bit;
                results.push(*output);
                self.stats.consumes += 1;
            }
        }
        if color.accepted {
            self.color_input = None;
        }
        let base_ce = caller.ce && self.color_input.is_none();
        let pre_work = self.work_count;
        let pre_coordinate = self.coordinate_count;
        let old_input = self.coordinate_ready.front().copied();
        let offer_cost =
            old_input.map_or(0, |i| i.parents.iter().filter(|&&p| p != 0).count() as u8);
        let old_output = self.coefficient.output();
        let mut packet = None;
        let mut done = vec![];
        let mut issues = vec![];
        let mut plane = None;
        let mut ack = false;
        let mut issue_ordinal = None;
        let t = self.enabled;
        if base_ce {
            self.enabled += 1;
            // Old packet already owns its operands; W cannot reread Work/Source.
            if self.cache.packet_ready()
                && self
                    .packets
                    .front()
                    .is_some_and(|p| t - p.issue > self.binding.packet.span() + 1)
            {
                let p = self.packets.pop().unwrap();
                packet = Some(p.payload);
                self.stats.packets += 1;
                let id = ((p.payload >> 66) & 15) as usize;
                let left = self.completion[id]
                    .as_mut()
                    .ok_or("packet without completion")?;
                *left = left.checked_sub(1).ok_or("completion underflow")?;
                if *left == 0 {
                    self.completion[id] = None;
                    done.push(id as u8);
                }
            }
            if stimulus.packet_ready && self.cache.packet_issue_ready() {
                if let Some(w) = self.work.front() {
                    let member = w.member;
                    let tap = w.cursor;
                    if member.emit() >> tap & 1 == 0 {
                        return Err("invalid actual work cursor".into());
                    }
                    let stage = counted::packet(&member, usize::from(tap))
                        .map_err(|e| format!("packet body: {e:?}"))?;
                    self.binding.packet.audit(&stage.frame)?;
                    let payload = raw(&stage, "packet");
                    let key = member.key();
                    let mask = self.masks[usize::from(key / 4)];
                    if mask >> (key % 4) & 1 == 0 {
                        return Err("physical lane not covered".into());
                    }
                    let ordinal = (mask & ((1 << (key % 4)) - 1)).count_ones() as usize;
                    issue_ordinal = Some((key, ordinal));
                    issues.push(timed::packet::Owner {
                        quad: key / 4,
                        lane: key % 4,
                    });
                    self.packets.push(Packet { payload, issue: t })?;
                    self.stats.issues += 1;
                    let next = (tap + 1..4).find(|&i| member.emit() >> i & 1 != 0);
                    if let Some(next) = next {
                        self.work.front_mut().unwrap().cursor = next;
                    } else {
                        self.work.pop();
                        self.work_count -= 1;
                        self.stats.ack += 1;
                        ack = true;
                    }
                }
            }
            // Existing reserved destination, one W/edge; old Work read above.
            if self
                .members
                .front()
                .is_some_and(|m| t - m.issue > self.binding.plane.span())
            {
                let member = Member::capture(&self.members.pop().unwrap().stage);
                let cursor = member.emit().trailing_zeros() as u8;
                if cursor >= 4 {
                    return Err("active plane has no actual group".into());
                }
                self.work.push(Work { member, cursor })?;
            }
            if stimulus.membership_ready {
                if let Some(output) = old_output {
                    let which = usize::from(self.ready_plane);
                    if output.weights[which].iter().all(|&w| w == 0) {
                        return Err("invalid ready plane".into());
                    }
                    let stage = counted::membership(output, which)
                        .map_err(|e| format!("member body: {e:?}"))?;
                    self.binding.plane.audit(&stage.frame)?;
                    self.members.push(Membership { stage, issue: t })?;
                    plane = Some((output, which));
                    self.stats.planes += 1;
                }
            }
        }
        let output_ready = plane.is_some_and(|(o, w)| w == 1 || o.metadata.last_fine);
        let coefficient = self
            .coefficient
            .tick(coefficient::Tick {
                ce: base_ce,
                input: old_input,
                output_ready,
                work_available: 16 - pre_work,
            })
            .map_err(|e| format!("coefficient: {e:?}"))?;
        if coefficient.accepted {
            if self.coordinate_ready.pop() != old_input {
                return Err("coordinate ownership".into());
            }
            self.coordinate_count -= 1;
            self.work_count += coefficient.work_reserved;
            self.stats.reserved += u64::from(coefficient.work_reserved);
        }
        if plane.is_some() {
            self.ready_plane = if output_ready { 0 } else { 1 };
        }
        if coefficient.consumed != output_ready {
            return Err("whole coefficient row handshake".into());
        }
        for e in &coefficient.events {
            match e {
                coefficient::Event::Issued {
                    site: coefficient::Site::Multiply(_),
                    ..
                } => self.stats.products += 1,
                coefficient::Event::Returned { .. } => self.stats.returns += 1,
                _ => {}
            }
        }
        if base_ce {
            if self
                .coordinates
                .front()
                .is_some_and(|c| t - c.issue > self.binding.coordinate.span())
            {
                self.coordinate_ready
                    .push(self.coordinates.pop().unwrap().input)?;
            }
            if t.is_multiple_of(self.binding.coordinate.ii()) && pre_coordinate < 6 {
                // Source order is the existing shared context FIFO, never a golden choice.
                let slot = self.ordered().find(|&s| {
                    let c = self.contexts[s].as_ref().unwrap();
                    c.lod.is_some_and(|u| t - u > self.binding.lod.span() + 1)
                        && c.next < c.source.lanes.len()
                });
                if let Some(slot) = slot {
                    let c = self.contexts[slot].as_mut().unwrap();
                    let lane = &c.source.lanes[c.next];
                    let lod = &c.source.lod;
                    let coord = &lane.coordinate;
                    let input = coefficient::Input {
                        parents: std::array::from_fn(|w| raw(lod, &format!("parent{w}")) as u16),
                        fractions: std::array::from_fn(|w| {
                            std::array::from_fn(|a| raw(coord, &format!("f{w}.{a}")) as u8)
                        }),
                        nearest: raw(lod, "nearest") != 0,
                        metadata: coefficient::Metadata {
                            coordinates: std::array::from_fn(|w| {
                                std::array::from_fn(|i| {
                                    raw(coord, &format!("t{w}.{}.{}", i / 2, i % 2)) as u16
                                })
                            }),
                            levels: std::array::from_fn(|w| raw(lod, &format!("n{w}")) as u8),
                            slot: raw(lod, "slot") as u8,
                            key: c.quad * 4 + lane.lane,
                            last_fine: raw(lod, "last_fine") != 0,
                        },
                    };
                    c.next += 1;
                    let last = c.next == c.source.lanes.len();
                    self.coordinates.push(Coordinate { input, issue: t })?;
                    self.coordinate_count += 1;
                    if last {
                        self.drop_context(slot);
                    }
                }
            }
            let empty = self.ordered().find(|&s| {
                let c = self.contexts[s].as_ref().unwrap();
                c.source.lanes.is_empty()
                    && c.lod.is_some_and(|u| t - u > self.binding.lod.span() + 1)
            });
            if let Some(slot) = empty {
                let q = self.contexts[slot].as_ref().unwrap().quad;
                self.drop_context(slot);
                self.completion[usize::from(q)] = None;
                done.push(q);
            }
            if t.is_multiple_of(self.binding.lod.ii()) {
                let slot = self.ordered().find(|&s| {
                    let c = self.contexts[s].as_ref().unwrap();
                    c.lod.is_none() && c.d.is_some_and(|u| t - u > self.binding.derivative.span())
                });
                if let Some(s) = slot {
                    self.contexts[s].as_mut().unwrap().lod = Some(t);
                }
            }
            if t.is_multiple_of(self.binding.derivative.ii()) {
                let slot = self
                    .ordered()
                    .find(|&s| self.contexts[s].as_ref().unwrap().d.is_none());
                if let Some(s) = slot {
                    self.contexts[s].as_mut().unwrap().d = Some(t);
                }
            }
        }
        let accepted = compiled.is_some();
        if let Some((q, prep, _)) = &compiled {
            if !base_ce {
                return Err("eligible compile base CE mismatch".into());
            }
            let slot = self
                .contexts
                .iter()
                .position(Option::is_none)
                .ok_or("context credit")?;
            let total = u8::try_from(prep.payloads.len()).map_err(|_| "completion6")?;
            self.completion[usize::from(q.quad_id)] = Some(total);
            self.masks[usize::from(q.quad_id)] = q.mask;
            self.remaining[usize::from(q.quad_id)] = q.mask;
            self.contexts[slot] = Some(Context {
                source: prep.clone(),
                quad: q.quad_id,
                d: None,
                lod: None,
                next: 0,
            });
            self.order.push(slot)?;
            self.stats.accepted += 1;
        }
        let cache = self
            .cache
            .step_pooled(
                memory,
                compiled.map(|(q, _, p)| (usize::from(q.quad_id), p)),
                timed::Control {
                    ce: base_ce,
                    result_ready: true,
                },
                packet,
                done.clone(),
                issues,
            )
            .map_err(|e| format!("cache step: {e:?}"))?;
        if cache.accepted != accepted {
            return Err("cache ingress mismatch".into());
        }
        for q in done {
            self.masks[usize::from(q)] = 0;
        }
        for event in &cache.events {
            if let timed::Event::Captured { group, words, .. } = event {
                if self.color_input.is_some() {
                    return Err("capture137 overwrite".into());
                }
                self.color_input = Some(color::Input {
                    payload: group.pack72()? as i128,
                    texels: *words,
                });
                self.stats.captures += 1;
            }
        }
        if self.work_count > 16
            || self.coordinate_count > 6
            || self.work.len + self.members.len > usize::from(self.work_count)
        {
            return Err("hybrid ownership bound".into());
        }
        self.stats.peak_work = self.stats.peak_work.max(self.work_count);
        self.stats.peak_coordinate = self.stats.peak_coordinate.max(self.coordinate_count);
        self.stats.peak_members = self.stats.peak_members.max(self.members.len);
        Ok(Edge {
            accepted,
            results,
            cache,
            coefficient,
            base_ce,
            pre_work,
            post_work: self.work_count,
            offer_cost,
            ack,
            packet,
            plane,
            issue_ordinal,
        })
    }
    fn ordered(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.order.len).map(|i| *self.order.rows[(self.order.read + i) % 8].as_ref().unwrap())
    }
    fn drop_context(&mut self, slot: usize) {
        self.contexts[slot] = None;
        // Stable bounded removal, no persistent second order queue.
        let count = self.order.len;
        for _ in 0..count {
            let s = self.order.pop().unwrap();
            if s != slot {
                self.order.push(s).unwrap();
            }
        }
    }
}
