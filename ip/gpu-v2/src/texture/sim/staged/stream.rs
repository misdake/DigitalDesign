//! Fixed-stage control/credit execution. Logic-stage latencies are proposals
//! until checked lowering is available; this report is NOT sampler throughput.
use super::*;
use audited::{
    physical::{DspIssue, DspWork},
    Operation,
};
use std::{collections::VecDeque, sync::Arc};
fn audit_input(frame: &FrameReport, name: &str, wanted: &[i128]) -> Result<(), String> {
    let memory = frame
        .memories
        .iter()
        .position(|m| m.name == name)
        .ok_or("capture input memory")?;
    let mut seen = vec![false; wanted.len()];
    for e in &frame.events {
        if let Operation::Read { memory: m, row } = e.operation {
            if m == memory {
                if wanted.get(row) != e.output.map(|v| &frame.values[v].raw) {
                    return Err("capture input value".into());
                }
                seen[row] = true;
            }
        }
    }
    if seen.iter().any(|&b| !b) {
        return Err("capture input coverage".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct Hardware {
    pub contexts: usize,
    pub coordinate_slots: usize,
    pub work_credits: usize,
    /// Release shared UV/LOD after its final lane capture; detached bounded
    /// lane/plane records carry every later operand. Latencies remain proposals.
    pub release_after_capture: bool,
    pub max_cycles: u64,
    pub max_quads: usize,
}
impl Default for Hardware {
    fn default() -> Self {
        Self {
            contexts: 4,
            coordinate_slots: 4,
            work_credits: 8,
            release_after_capture: false,
            max_cycles: 2_000_000,
            max_quads: 256,
        }
    }
}
impl Hardware {
    fn validate(self) -> Result<(), String> {
        if !(1..=8).contains(&self.contexts)
            || !(1..=8).contains(&self.coordinate_slots)
            || !(2..=16).contains(&self.work_credits)
            || self.max_cycles == 0
            || self.max_cycles > 2_000_000
            || self.max_quads == 0
            || self.max_quads > 256
        {
            return Err("staged capacity/budget".into());
        }
        Ok(())
    }
}
pub struct Program {
    preparation: Preparation,
    quad: u8,
    mask: u8,
}
impl Program {
    pub fn preparation(&self) -> &Preparation {
        &self.preparation
    }
    pub fn quad_id(&self) -> u8 {
        self.quad
    }
    pub fn mask(&self) -> u8 {
        self.mask
    }
    pub fn compile(q: &QuadInput, slots: &[Slot]) -> Result<Arc<Self>, counted::Error> {
        let preparation = prepare(q, slots)?;
        Ok(Arc::new(Self {
            quad: q.quad_id,
            mask: q.mask,
            preparation,
        }))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Accepted { program: usize, slot: usize },
    Derivatives { slot: usize },
    Lod { slot: usize },
    CoordinateIssue { slot: usize, lane: u8 },
    CoordinateReady { slot: usize, lane: u8 },
    CoefficientIssue { slot: usize, lane: u8 },
    PlaneReady { slot: usize, lane: u8, which: u8 },
    Packet { slot: usize, payload: i128 },
    Release { program: usize, slot: usize },
    SharedRelease { program: usize, slot: usize },
    LaneCapture { slot: usize, data: LaneCapture },
    PlaneCapture { slot: usize, data: PlaneCapture },
}
/// Verbatim closed boundary inputs, 116 data bits including identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneCapture {
    pub uv: [i128; 2],
    pub shift: i128,
    pub nearest: i128,
    pub halve: i128,
    pub side: [i128; 2],
    pub parents: [i128; 2],
    pub n: [i128; 2],
    pub slot: i128,
    pub quad: i128,
    pub lane: u8,
}
impl LaneCapture {
    fn capture(p: &Program, lane: usize) -> Self {
        let d = &p.preparation.derivative;
        let c = &p.preparation.lod;
        let actual = p.preparation.lanes[lane].lane;
        Self {
            uv: std::array::from_fn(|a| d.raw(&format!("uv{}", usize::from(actual) * 2 + a))),
            shift: c.raw("shift0"),
            nearest: c.raw("nearest"),
            halve: c.raw("halve"),
            side: std::array::from_fn(|a| c.raw(&format!("side{a}"))),
            parents: std::array::from_fn(|a| c.raw(&format!("parent{a}"))),
            n: std::array::from_fn(|a| c.raw(&format!("n{a}"))),
            slot: c.raw("slot"),
            quad: c.raw("quad"),
            lane: actual,
        }
    }
    fn audit(&self, p: &Program, lane: usize) -> Result<(), String> {
        if *self != Self::capture(p, lane) {
            return Err("lane capture provenance".into());
        }
        let l = &p.preparation.lanes[lane];
        audit_input(&l.coordinate.frame, "wrapped_uv", &self.uv)?;
        audit_input(&l.coordinate.frame, "coordinate_shift", &[self.shift])?;
        audit_input(&l.coordinate.frame, "flags", &[self.nearest, self.halve])?;
        audit_input(&l.coordinate.frame, "side", &self.side)?;
        audit_input(&l.rows.frame, "parents", &self.parents)?;
        Ok(())
    }
}
/// Plane operands rather than four prepacked words: 92 data bits. A local
/// 2-bit cursor makes a 94-bit work record; packet computation is still closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaneCapture {
    pub weights: [i128; 4],
    pub coords: [i128; 4],
    pub identity: [i128; 3],
    pub lane: u8,
    pub which: u8,
    pub final_plane: bool,
}
impl PlaneCapture {
    fn capture(p: &Program, lane: usize, which: usize) -> Self {
        let l = &p.preparation.lanes[lane];
        let plane = &l.planes[which];
        let c = &p.preparation.lod;
        let w = usize::from(plane.which);
        Self {
            weights: std::array::from_fn(|t| l.columns.raw(&format!("w{w}.{t}"))),
            coords: std::array::from_fn(|i| l.coordinate.raw(&format!("t{w}.{}.{}", i / 2, i % 2))),
            identity: [c.raw("slot"), c.raw(&format!("n{w}")), c.raw("quad")],
            lane: l.lane,
            which: plane.which,
            final_plane: plane
                .frame
                .outputs
                .iter()
                .find(|o| o.name == "final_plane")
                .unwrap()
                .raw
                != 0,
        }
    }
    fn audit(&self, p: &Program, lane: usize, which: usize) -> Result<(), String> {
        if *self != Self::capture(p, lane, which) {
            return Err("plane capture provenance".into());
        }
        let frame = &p.preparation.lanes[lane].planes[which].frame;
        audit_input(frame, "weights", &self.weights)?;
        audit_input(frame, "coords", &self.coords)?;
        audit_input(frame, "identity", &self.identity)?;
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub cycles: u64,
    pub enabled: u64,
    pub accepted: u64,
    pub released: u64,
    pub packets: u64,
    pub coordinate_stalls: u64,
    pub coefficient_stalls: u64,
    pub output_stalls: u64,
    pub peak_contexts: usize,
    pub peak_coordinates: usize,
    pub peak_work: usize,
    pub peak_live_quads: usize,
    pub lane_captures: u64,
    pub plane_captures: u64,
    pub peak_boundary_record_bits: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub contexts: usize,
    pub coordinates: usize,
    pub work: usize,
    pub live_ids: u16,
    pub retained_record_bits: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub cycle: u64,
    pub ce: bool,
    pub ready: bool,
    pub offered: Option<usize>,
    pub accepted: bool,
    pub events: Vec<Event>,
    pub snapshot: Snapshot,
}
struct Context {
    program: usize,
    source: Arc<Program>,
    derivative_start: Option<u64>,
    lod_start: Option<u64>,
    lod_ready: bool,
    next_lane: usize,
    remaining_planes: usize,
}
#[derive(Clone)]
struct LaneToken {
    slot: usize,
    lane: usize,
    issue: u64,
    program: usize,
    source: Arc<Program>,
    capture: LaneCapture,
}
#[derive(Clone)]
struct PlaneToken {
    slot: usize,
    lane: usize,
    which: usize,
    next: usize,
    program: usize,
    source: Arc<Program>,
    capture: PlaneCapture,
}
struct Completion {
    program: usize,
    slot: usize,
    remaining: usize,
}
pub struct Machine {
    hardware: Hardware,
    slots: Vec<Option<Context>>,
    order: VecDeque<usize>,
    derivative_next: u64,
    lod_next: u64,
    coordinate_next: u64,
    coefficient_next: u64,
    coordinates: VecDeque<LaneToken>,
    coordinate_ready: VecDeque<LaneToken>,
    coefficients: VecDeque<LaneToken>,
    planes: VecDeque<PlaneToken>,
    coordinate_credits: usize,
    work_credits: usize,
    live_ids: u16,
    completions: Vec<Option<Completion>>,
    pub stats: Stats,
    pub dsp_issues: Vec<DspIssue>,
}
impl Machine {
    pub fn new(hardware: Hardware) -> Result<Self, String> {
        hardware.validate()?;
        Ok(Self {
            slots: (0..hardware.contexts).map(|_| None).collect(),
            hardware,
            order: VecDeque::new(),
            derivative_next: 0,
            lod_next: 0,
            coordinate_next: 0,
            coefficient_next: 0,
            coordinates: VecDeque::new(),
            coordinate_ready: VecDeque::new(),
            coefficients: VecDeque::new(),
            planes: VecDeque::new(),
            coordinate_credits: 0,
            work_credits: 0,
            live_ids: 0,
            completions: (0..16).map(|_| None).collect(),
            stats: Stats::default(),
            dsp_issues: vec![],
        })
    }
    pub fn idle(&self) -> bool {
        self.live_ids == 0
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            contexts: self.slots.iter().filter(|s| s.is_some()).count(),
            coordinates: self.coordinate_credits,
            work: self.work_credits,
            live_ids: self.live_ids,
            // Declared interstage boundary payloads only; neither internal
            // arithmetic registers nor fitted storage placement is certified.
            retained_record_bits: self.coordinates.len() * 116
                + self.coordinate_ready.len() * 149
                + self.coefficients.len() * 171
                + self.planes.len() * 94,
        }
    }
    fn source(&self, slot: usize) -> &Program {
        &self.slots[slot].as_ref().unwrap().source
    }
    fn release_shared(&mut self, slot: usize, events: &mut Vec<Event>) {
        let context = self.slots[slot].take().unwrap();
        self.order.retain(|&s| s != slot);
        events.push(Event::SharedRelease {
            program: context.program,
            slot,
        });
    }
    fn retire(&mut self, quad: u8, events: &mut Vec<Event>) {
        let completion = self.completions[usize::from(quad)].take().unwrap();
        self.live_ids &= !(1 << quad);
        if !self.hardware.release_after_capture {
            self.release_shared(completion.slot, events);
        }
        self.stats.released += 1;
        events.push(Event::Release {
            program: completion.program,
            slot: completion.slot,
        });
    }
    /// Consumer CE freezes every preparation state/credit/phase. Refill is owned
    /// by the separate cache and is deliberately outside this machine.
    pub fn step(
        &mut self,
        offered: Option<(usize, Arc<Program>)>,
        ce: bool,
        ready: bool,
    ) -> Result<Step, String> {
        if self.stats.cycles >= self.hardware.max_cycles {
            return Err("staged control watchdog".into());
        }
        self.stats.cycles += 1;
        let mut events = vec![];
        let mut accepted = false;
        let offered_index = offered.as_ref().map(|(i, _)| *i);
        if ce {
            let clock = self.stats.enabled;
            self.stats.enabled += 1;
            // One registered-work read/packet output per edge; no plane way-lock.
            if let Some(front) = self.planes.front().cloned() {
                if ready {
                    front
                        .capture
                        .audit(&front.source, front.lane, front.which)?;
                    let p = &front.source.preparation.lanes[front.lane].planes[front.which];
                    let payload = p.payloads[front.next];
                    let len = p.payloads.len();
                    events.push(Event::Packet {
                        slot: front.slot,
                        payload,
                    });
                    self.stats.packets += 1;
                    self.planes.front_mut().unwrap().next += 1;
                    if front.next + 1 == len {
                        self.planes.pop_front();
                        self.work_credits -= 1;
                        let context = self.completions[usize::from(front.source.quad)]
                            .as_mut()
                            .unwrap();
                        if context.program != front.program {
                            return Err("completion program alias".into());
                        }
                        context.remaining -= 1;
                        if context.remaining == 0 {
                            self.retire(front.source.quad, &mut events);
                        }
                    }
                } else {
                    self.stats.output_stalls += 1;
                }
            }
            // Fine at +9, coarse at +10: a single work-FIFO write per edge.
            let mut plane_writes = 0;
            let mut surviving = VecDeque::new();
            while let Some(token) = self.coefficients.pop_front() {
                let age = clock - token.issue;
                let source = &token.source;
                let planes = &source.preparation.lanes[token.lane].planes;
                let cost = planes.len();
                let actual_lane = source.preparation.lanes[token.lane].lane;
                if age == 9 {
                    let capture = PlaneCapture::capture(source, token.lane, 0);
                    capture.audit(source, token.lane, 0)?;
                    self.stats.plane_captures += 1;
                    events.push(Event::PlaneCapture {
                        slot: token.slot,
                        data: capture.clone(),
                    });
                    self.planes.push_back(PlaneToken {
                        slot: token.slot,
                        lane: token.lane,
                        which: 0,
                        next: 0,
                        source: source.clone(),
                        program: token.program,
                        capture,
                    });
                    events.push(Event::PlaneReady {
                        slot: token.slot,
                        lane: actual_lane,
                        which: 0,
                    });
                    plane_writes += 1;
                }
                if age == 10 && cost == 2 {
                    let capture = PlaneCapture::capture(source, token.lane, 1);
                    capture.audit(source, token.lane, 1)?;
                    self.stats.plane_captures += 1;
                    events.push(Event::PlaneCapture {
                        slot: token.slot,
                        data: capture.clone(),
                    });
                    self.planes.push_back(PlaneToken {
                        slot: token.slot,
                        lane: token.lane,
                        which: 1,
                        next: 0,
                        source: source.clone(),
                        program: token.program,
                        capture,
                    });
                    events.push(Event::PlaneReady {
                        slot: token.slot,
                        lane: actual_lane,
                        which: 1,
                    });
                    plane_writes += 1;
                }
                if age < if cost == 2 { 10 } else { 9 } {
                    surviving.push_back(token);
                }
            }
            self.coefficients = surviving;
            if plane_writes > 1 {
                return Err("work FIFO static write collision".into());
            }
            while self
                .coordinates
                .front()
                .is_some_and(|t| clock - t.issue >= 4)
            {
                let t = self.coordinates.pop_front().unwrap();
                events.push(Event::CoordinateReady {
                    slot: t.slot,
                    lane: t.source.preparation.lanes[t.lane].lane,
                });
                self.coordinate_ready.push_back(t);
            }
            if clock >= self.coefficient_next && clock.is_multiple_of(2) {
                if let Some(t) = self.coordinate_ready.front().cloned() {
                    t.capture.audit(&t.source, t.lane)?;
                    let lane = &t.source.preparation.lanes[t.lane];
                    let cost = lane.planes.len();
                    let actual_lane = lane.lane;
                    if self.work_credits + cost <= self.hardware.work_credits {
                        let issues = coefficient_issues(lane, clock)?;
                        self.dsp_issues.extend(issues);
                        self.coordinate_ready.pop_front();
                        self.coordinate_credits -= 1;
                        self.work_credits += cost;
                        self.coefficient_next = clock + 2;
                        self.coefficients.push_back(LaneToken {
                            issue: clock,
                            ..t.clone()
                        });
                        events.push(Event::CoefficientIssue {
                            slot: t.slot,
                            lane: actual_lane,
                        });
                    } else {
                        self.stats.coefficient_stalls += 1;
                    }
                }
            }
            // In-order lane capture. Early-release mode detaches later operands;
            // mask suppresses lane work, never helper derivatives.
            if clock >= self.coordinate_next && clock.is_multiple_of(2) {
                if let Some(slot) = self.order.iter().copied().find(|&s| {
                    let c = self.slots[s].as_ref().unwrap();
                    c.lod_ready && c.next_lane < c.source.preparation.lanes.len()
                }) {
                    if self.coordinate_credits < self.hardware.coordinate_slots {
                        let c = self.slots[slot].as_mut().unwrap();
                        let lane = c.next_lane;
                        c.next_lane += 1;
                        let actual = c.source.preparation.lanes[lane].lane;
                        let capture = LaneCapture::capture(&c.source, lane);
                        capture.audit(&c.source, lane)?;
                        let last = c.next_lane == c.source.preparation.lanes.len();
                        self.coordinates.push_back(LaneToken {
                            slot,
                            lane,
                            issue: clock,
                            source: c.source.clone(),
                            program: c.program,
                            capture: capture.clone(),
                        });
                        self.coordinate_credits += 1;
                        self.coordinate_next = clock + 2;
                        events.push(Event::CoordinateIssue { slot, lane: actual });
                        events.push(Event::LaneCapture {
                            slot,
                            data: capture,
                        });
                        self.stats.lane_captures += 1;
                        if last && self.hardware.release_after_capture {
                            self.release_shared(slot, &mut events);
                        }
                    } else {
                        self.stats.coordinate_stalls += 1;
                    }
                }
            }
            // Fixed 12-cycle difference/abs/max path and proposed 16-cycle
            // LOD/context path. Each admits one quad per eight enabled cycles.
            for slot in 0..self.slots.len() {
                if let Some(c) = &mut self.slots[slot] {
                    if !c.lod_ready && c.lod_start.is_some_and(|t| clock >= t + 16) {
                        c.lod_ready = true;
                        events.push(Event::Lod { slot });
                    }
                }
            }
            let empty: Vec<_> = self
                .order
                .iter()
                .copied()
                .filter(|&slot| {
                    let c = self.slots[slot].as_ref().unwrap();
                    c.lod_ready && c.remaining_planes == 0
                })
                .collect();
            for slot in empty {
                let quad = self.source(slot).quad;
                if self.hardware.release_after_capture {
                    self.release_shared(slot, &mut events);
                }
                self.retire(quad, &mut events);
            }
            if clock >= self.lod_next && clock % 8 == 4 {
                if let Some(slot) = self.order.iter().copied().find(|&s| {
                    let c = self.slots[s].as_ref().unwrap();
                    c.lod_start.is_none() && c.derivative_start.is_some_and(|t| clock >= t + 12)
                }) {
                    self.slots[slot].as_mut().unwrap().lod_start = Some(clock);
                    self.lod_next = clock + 8;
                    events.push(Event::Derivatives { slot });
                }
            }
            if clock >= self.derivative_next && clock.is_multiple_of(8) {
                if let Some(slot) = self
                    .order
                    .iter()
                    .copied()
                    .find(|&s| self.slots[s].as_ref().unwrap().derivative_start.is_none())
                {
                    self.slots[slot].as_mut().unwrap().derivative_start = Some(clock);
                    self.derivative_next = clock + 8;
                }
            }
            if let Some((program, source)) = offered {
                if self.live_ids >> source.quad & 1 == 0 {
                    if let Some(slot) = self.slots.iter().position(Option::is_none) {
                        let remaining_planes = source
                            .preparation
                            .lanes
                            .iter()
                            .map(|l| l.planes.len())
                            .sum();
                        self.live_ids |= 1 << source.quad;
                        self.completions[usize::from(source.quad)] = Some(Completion {
                            program,
                            slot,
                            remaining: remaining_planes,
                        });
                        self.slots[slot] = Some(Context {
                            program,
                            source,
                            derivative_start: None,
                            lod_start: None,
                            lod_ready: false,
                            next_lane: 0,
                            remaining_planes,
                        });
                        self.order.push_back(slot);
                        accepted = true;
                        self.stats.accepted += 1;
                        events.push(Event::Accepted { program, slot });
                    }
                }
            }
        }
        let snapshot = self.snapshot();
        self.stats.peak_contexts = self.stats.peak_contexts.max(snapshot.contexts);
        self.stats.peak_coordinates = self.stats.peak_coordinates.max(snapshot.coordinates);
        self.stats.peak_work = self.stats.peak_work.max(snapshot.work);
        self.stats.peak_live_quads = self
            .stats
            .peak_live_quads
            .max(snapshot.live_ids.count_ones() as usize);
        self.stats.peak_boundary_record_bits = self
            .stats
            .peak_boundary_record_bits
            .max(snapshot.retained_record_bits);
        if snapshot.coordinates > self.hardware.coordinate_slots
            || snapshot.work > self.hardware.work_credits
        {
            return Err("staged credit overflow".into());
        }
        Ok(Step {
            cycle: self.stats.cycles,
            ce,
            ready,
            offered: offered_index,
            accepted,
            events,
            snapshot,
        })
    }
}
fn product(frame: &FrameReport, name: &str) -> Result<Option<usize>, String> {
    let v = frame
        .outputs
        .iter()
        .find(|o| o.name == name)
        .ok_or("coefficient output")?
        .value;
    let mut e = &frame.events[frame.values[v].producer];
    while matches!(e.operation, Operation::Resize) {
        if e.inputs.len() != 1
            || frame.values[e.inputs[0]].format
                != frame.values[e.output.ok_or("coefficient resize output")?].format
        {
            return Err("coefficient branch alias format".into());
        }
        e = &frame.events[frame.values[e.inputs[0]].producer];
    }
    if !matches!(e.operation, Operation::Slice(8)) {
        return Ok(None);
    }
    let p = &frame.events[frame.values[e.inputs[0]].producer];
    if !matches!(p.operation, Operation::Multiply)
        || p.inputs.len() != 2
        || frame.values[p.inputs[0]].format.bits != 9
        || frame.values[p.inputs[1]].format.bits != 8
    {
        return Err("coefficient product provenance".into());
    }
    Ok(Some(p.id))
}
pub fn coefficient_issues(lane: &Lane, start: u64) -> Result<Vec<DspIssue>, String> {
    let mut issues = vec![];
    for (frame, table) in [
        (&lane.rows.frame, vec![("r0.1", 0, 0), ("r1.1", 1, 0)]),
        (
            &lane.columns.frame,
            vec![
                ("w0.1", 2, 4),
                ("w0.3", 0, 5),
                ("w1.1", 1, 5),
                ("w1.3", 2, 5),
            ],
        ),
    ] {
        frame
            .audit()
            .map_err(|e| format!("coefficient frame: {e:?}"))?;
        let mut seen = std::collections::BTreeSet::new();
        for (name, instance, offset) in table {
            if let Some(id) = product(frame, name)? {
                if !seen.insert(id) {
                    return Err("duplicate coefficient product".into());
                }
                issues.push(DspIssue {
                    instance,
                    issue: start + offset,
                    ready: start + offset + 3,
                    work: DspWork::Multiply {
                        a_bits: 9,
                        b_bits: 8,
                    },
                });
            }
        }
        let expected: std::collections::BTreeSet<_> = frame
            .events
            .iter()
            .filter(|e| matches!(e.operation, Operation::Multiply))
            .map(|e| e.id)
            .collect();
        if seen != expected {
            return Err("unassigned coefficient product".into());
        }
    }
    Ok(issues)
}
pub struct Report {
    pub hardware: Hardware,
    pub programs: Vec<Arc<Program>>,
    pub steps: Vec<Step>,
    pub payloads: Vec<i128>,
    pub stats: Stats,
}
impl Report {
    pub fn audit(&self) -> Result<(), String> {
        self.hardware.validate()?;
        if self.programs.len() > self.hardware.max_quads
            || self.steps.len() as u64 > self.hardware.max_cycles
        {
            return Err("staged trace bound".into());
        }
        let mut m = Machine::new(self.hardware)?;
        let mut next = 0;
        let mut payloads = vec![];
        for s in &self.steps {
            let offered = match s.offered {
                Some(i) if i == next => {
                    Some((i, self.programs.get(i).ok_or("program index")?.clone()))
                }
                None => None,
                _ => return Err("staged offer order".into()),
            };
            let got = m.step(offered, s.ce, s.ready)?;
            if got != *s {
                return Err("staged controller replay".into());
            }
            if s.accepted {
                next += 1;
            }
            for e in &s.events {
                if let Event::Packet { payload, .. } = e {
                    payloads.push(*payload);
                }
            }
        }
        let gold: Vec<_> = self
            .programs
            .iter()
            .flat_map(|p| p.preparation.payloads.iter().copied())
            .collect();
        if !m.idle()
            || next != self.programs.len()
            || m.stats != self.stats
            || payloads != self.payloads
            || payloads != gold
        {
            return Err("staged drain/order/payload".into());
        }
        super::super::timed::Hardware::default()
            .inventory()?
            .audit_issues(&m.dsp_issues, None, self.hardware.max_cycles + 10)
            .map_err(|e| format!("fixed coefficient DSP calendar: {e:?}"))?;
        Ok(())
    }
}
pub fn run(
    inputs: &[QuadInput],
    slots: &[Slot],
    hardware: Hardware,
    mut control: impl FnMut(u64) -> (bool, bool),
) -> Result<Report, String> {
    hardware.validate()?;
    if inputs.len() > hardware.max_quads {
        return Err("staged quad budget".into());
    }
    let programs = inputs
        .iter()
        .map(|q| Program::compile(q, slots).map_err(|e| format!("staged numerical: {e:?}")))
        .collect::<Result<Vec<_>, _>>()?;
    let mut m = Machine::new(hardware)?;
    let mut next = 0;
    let mut steps = vec![];
    let mut payloads = vec![];
    while next < programs.len() || !m.idle() {
        let (ce, ready) = control(m.stats.cycles + 1);
        let s = m.step(programs.get(next).map(|p| (next, p.clone())), ce, ready)?;
        if s.accepted {
            next += 1;
        }
        for e in &s.events {
            if let Event::Packet { payload, .. } = e {
                payloads.push(*payload);
            }
        }
        steps.push(s);
    }
    let report = Report {
        hardware,
        programs,
        steps,
        payloads,
        stats: m.stats,
    };
    report.audit()?;
    Ok(report)
}
