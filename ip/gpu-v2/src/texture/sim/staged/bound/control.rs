//! Finite control for the shared, input-independent stage bindings.
use super::*;
use std::collections::VecDeque;
#[derive(Clone, Copy, Debug)]
pub struct Hardware {
    pub storage: Storage,
    pub contexts: usize,
    pub release_after_capture: bool,
    pub coordinate_credits: usize,
    pub work_credits: usize,
    pub packet_credits: usize,
    pub max_cycles: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Storage {
    #[default]
    Dedicated,
    Packed,
}
impl Default for Hardware {
    fn default() -> Self {
        Self {
            storage: Storage::Dedicated,
            contexts: 8,
            release_after_capture: true,
            coordinate_credits: 6,
            work_credits: 16,
            packet_credits: 16,
            max_cycles: 2_000_000,
        }
    }
}
impl Hardware {
    pub fn validate(self) -> Result<(), String> {
        if !(1..=8).contains(&self.contexts)
            || !(1..=16).contains(&self.coordinate_credits)
            || !(2..=32).contains(&self.work_credits)
            || !(1..=32).contains(&self.packet_credits)
            || self.max_cycles == 0
            || self.max_cycles > 2_000_000
        {
            return Err("bound capacities".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub cycles: u64,
    pub enabled: u64,
    pub accepted: u64,
    pub packets: u64,
    pub released: u64,
    pub peak_contexts: usize,
    pub peak_live: usize,
    pub peak_coordinates: usize,
    pub peak_work: usize,
    pub peak_packet: usize,
    pub blocked: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Accept {
        program: usize,
        slot: usize,
    },
    Issue {
        stage: &'static str,
        program: usize,
        lane: usize,
        plane: usize,
        packet: usize,
    },
    SharedRelease {
        program: usize,
        slot: usize,
    },
    Packet {
        program: usize,
        payload: i128,
    },
    Release {
        program: usize,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub cycle: u64,
    pub ce: bool,
    pub ready: bool,
    pub packet_issue_ready: bool,
    pub offered: Option<usize>,
    pub accepted: bool,
    pub events: Vec<Event>,
}
struct Context {
    program: usize,
    source: Arc<Program>,
    d: Option<u64>,
    lod: Option<u64>,
    next: usize,
}
#[derive(Clone)]
struct Token {
    program: usize,
    source: Arc<Program>,
    lane: usize,
    plane: usize,
    packet: usize,
    issue: u64,
}
struct Completion {
    program: usize,
    slot: usize,
    left: usize,
}
pub struct Machine {
    pub hardware: Hardware,
    pub binding: Arc<Binding>,
    pub stats: Stats,
    slots: Vec<Option<Context>>,
    order: VecDeque<usize>,
    live: u16,
    completion: Vec<Option<Completion>>,
    coordinates: VecDeque<Token>,
    coord_ready: VecDeque<Token>,
    coefficients: VecDeque<Token>,
    coeff_ready: VecDeque<Token>,
    members: VecDeque<Token>,
    work: VecDeque<Token>,
    packets: VecDeque<Token>,
    output: VecDeque<Token>,
    coordinate_count: usize,
    work_count: usize,
    packet_count: usize,
    pub dsp_issues: Vec<physical::DspIssue>,
}
impl Machine {
    pub(crate) fn pending_packets(&self) -> usize {
        self.packet_count
    }
    pub fn new(binding: Arc<Binding>, hardware: Hardware) -> Result<Self, String> {
        hardware.validate()?;
        Ok(Self {
            hardware,
            binding,
            stats: Stats::default(),
            slots: (0..hardware.contexts).map(|_| None).collect(),
            order: VecDeque::new(),
            live: 0,
            completion: (0..16).map(|_| None).collect(),
            coordinates: VecDeque::new(),
            coord_ready: VecDeque::new(),
            coefficients: VecDeque::new(),
            coeff_ready: VecDeque::new(),
            members: VecDeque::new(),
            work: VecDeque::new(),
            packets: VecDeque::new(),
            output: VecDeque::new(),
            coordinate_count: 0,
            work_count: 0,
            packet_count: 0,
            dsp_issues: vec![],
        })
    }
    pub fn idle(&self) -> bool {
        self.live == 0
    }
    fn shared_release(&mut self, slot: usize, events: &mut Vec<Event>) {
        let c = self.slots[slot].take().unwrap();
        self.order.retain(|&s| s != slot);
        events.push(Event::SharedRelease {
            program: c.program,
            slot,
        });
    }
    fn release(&mut self, quad: u8, events: &mut Vec<Event>) {
        let c = self.completion[usize::from(quad)].take().unwrap();
        self.live &= !(1 << quad);
        if !self.hardware.release_after_capture {
            self.shared_release(c.slot, events);
        }
        self.stats.released += 1;
        events.push(Event::Release { program: c.program });
    }
    pub fn step(
        &mut self,
        offer: Option<(usize, Arc<Program>)>,
        ce: bool,
        ready: bool,
    ) -> Result<Step, String> {
        self.step_packet_port(offer, ce, ready, true)
    }
    pub(crate) fn step_packet_port(
        &mut self,
        offer: Option<(usize, Arc<Program>)>,
        ce: bool,
        ready: bool,
        packet_issue_ready: bool,
    ) -> Result<Step, String> {
        if self.stats.cycles >= self.hardware.max_cycles {
            return Err("bound watchdog".into());
        }
        self.stats.cycles += 1;
        let mut events = vec![];
        let mut accepted = false;
        let offered = offer.as_ref().map(|p| p.0);
        if ce {
            let t = self.stats.enabled;
            self.stats.enabled += 1;
            // Outputs become stable after the primitive-ready edge. A separate
            // holding-register edge captures them; the next stage consumes the
            // ready queue on a later edge. Shared D/LOD contexts have the same
            // registered cuts. No same-edge consumer bypass is assumed.
            if ready {
                if let Some(p) = self.output.pop_front() {
                    let word =
                        p.source.preparation.lanes[p.lane].packets[p.plane][p.packet].raw("packet");
                    events.push(Event::Packet {
                        program: p.program,
                        payload: word,
                    });
                    self.stats.packets += 1;
                    self.packet_count -= 1;
                    let id = p.source.input.quad_id;
                    let c = self.completion[usize::from(id)].as_mut().unwrap();
                    if c.program != p.program || c.left == 0 {
                        return Err("bound ID reuse".into());
                    }
                    c.left -= 1;
                    if c.left == 0 {
                        self.release(id, &mut events);
                    }
                }
            } else if !self.output.is_empty() {
                self.stats.blocked += 1;
            }
            while self
                .packets
                .front()
                .is_some_and(|p| t - p.issue > self.binding.packet.span())
            {
                self.output.push_back(self.packets.pop_front().unwrap());
            }
            if t.is_multiple_of(self.binding.packet.ii())
                && self.packet_count < self.hardware.packet_credits
                && packet_issue_ready
            {
                if let Some(mut w) = self.work.front().cloned() {
                    let count = w.source.preparation.lanes[w.lane].packets[w.plane].len();
                    w.issue = t;
                    events.push(Event::Issue {
                        stage: "packet",
                        program: w.program,
                        lane: w.lane,
                        plane: w.plane,
                        packet: w.packet,
                    });
                    self.packets.push_back(w.clone());
                    self.packet_count += 1;
                    self.work.front_mut().unwrap().packet += 1;
                    if w.packet + 1 == count {
                        self.work.pop_front();
                        self.work_count -= 1;
                    }
                }
            }
            while self
                .members
                .front()
                .is_some_and(|p| t - p.issue > self.binding.plane.span())
            {
                self.work.push_back(self.members.pop_front().unwrap());
            }
            if t.is_multiple_of(self.binding.plane.ii()) {
                if let Some(mut c) = self.coeff_ready.front().cloned() {
                    let count = c.source.preparation.lanes[c.lane].memberships.len();
                    c.issue = t;
                    events.push(Event::Issue {
                        stage: "membership",
                        program: c.program,
                        lane: c.lane,
                        plane: c.plane,
                        packet: 0,
                    });
                    self.members.push_back(c.clone());
                    self.coeff_ready.front_mut().unwrap().plane += 1;
                    if c.plane + 1 == count {
                        self.coeff_ready.pop_front();
                    }
                }
            }
            while self
                .coefficients
                .front()
                .is_some_and(|p| t - p.issue > self.binding.coefficient.span())
            {
                self.coeff_ready
                    .push_back(self.coefficients.pop_front().unwrap());
            }
            if t.is_multiple_of(self.binding.coefficient.ii()) {
                if let Some(mut c) = self.coord_ready.front().cloned() {
                    let cost = c.source.preparation.lanes[c.lane].memberships.len();
                    if self.work_count + cost <= self.hardware.work_credits {
                        for (e, time) in c.source.preparation.lanes[c.lane]
                            .coefficient
                            .frame
                            .events
                            .iter()
                            .zip(&self.binding.coefficient.times)
                        {
                            if e.operation == Operation::Multiply {
                                self.dsp_issues.push(physical::DspIssue {
                                    instance: self.binding.coefficient.calendar.nodes[e.id]
                                        .lane
                                        .unwrap(),
                                    issue: t + time.issue,
                                    ready: t + time.ready,
                                    work: physical::DspWork::Multiply {
                                        a_bits: 9,
                                        b_bits: 8,
                                    },
                                });
                            }
                        }
                        c.issue = t;
                        self.coord_ready.pop_front();
                        self.coordinate_count -= 1;
                        self.work_count += cost;
                        events.push(Event::Issue {
                            stage: "coefficient",
                            program: c.program,
                            lane: c.lane,
                            plane: 0,
                            packet: 0,
                        });
                        self.coefficients.push_back(c);
                    }
                }
            }
            while self
                .coordinates
                .front()
                .is_some_and(|p| t - p.issue > self.binding.coordinate.span())
            {
                self.coord_ready
                    .push_back(self.coordinates.pop_front().unwrap());
            }
            if t.is_multiple_of(self.binding.coordinate.ii())
                && self.coordinate_count < self.hardware.coordinate_credits
            {
                if let Some(slot) = self.order.iter().copied().find(|&s| {
                    let c = self.slots[s].as_ref().unwrap();
                    c.lod.is_some_and(|u| t - u > self.binding.lod.span() + 1)
                        && c.next < c.source.preparation.lanes.len()
                }) {
                    let c = self.slots[slot].as_mut().unwrap();
                    let lane = c.next;
                    c.next += 1;
                    self.coordinates.push_back(Token {
                        program: c.program,
                        source: c.source.clone(),
                        lane,
                        plane: 0,
                        packet: 0,
                        issue: t,
                    });
                    self.coordinate_count += 1;
                    events.push(Event::Issue {
                        stage: "coordinate",
                        program: c.program,
                        lane,
                        plane: 0,
                        packet: 0,
                    });
                    if c.next == c.source.preparation.lanes.len()
                        && self.hardware.release_after_capture
                    {
                        self.shared_release(slot, &mut events);
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
            for s in empty {
                let q = self.slots[s].as_ref().unwrap().source.input.quad_id;
                if self.hardware.release_after_capture {
                    self.shared_release(s, &mut events);
                }
                self.release(q, &mut events);
            }
            if t.is_multiple_of(self.binding.lod.ii()) {
                if let Some(s) = self.order.iter().copied().find(|&s| {
                    let c = self.slots[s].as_ref().unwrap();
                    c.lod.is_none() && c.d.is_some_and(|u| t - u > self.binding.derivative.span())
                }) {
                    let c = self.slots[s].as_mut().unwrap();
                    c.lod = Some(t);
                    events.push(Event::Issue {
                        stage: "lod",
                        program: c.program,
                        lane: 0,
                        plane: 0,
                        packet: 0,
                    });
                }
            }
            if t.is_multiple_of(self.binding.derivative.ii()) {
                if let Some(s) = self
                    .order
                    .iter()
                    .copied()
                    .find(|&s| self.slots[s].as_ref().unwrap().d.is_none())
                {
                    let c = self.slots[s].as_mut().unwrap();
                    c.d = Some(t);
                    events.push(Event::Issue {
                        stage: "derivative",
                        program: c.program,
                        lane: 0,
                        plane: 0,
                        packet: 0,
                    });
                }
            }
            if let Some((program, source)) = offer {
                let q = source.input.quad_id;
                if self.live >> q & 1 == 0 {
                    if let Some(slot) = self.slots.iter().position(Option::is_none) {
                        if !Arc::ptr_eq(&source.binding, &self.binding) {
                            return Err("binding source identity".into());
                        }
                        self.live |= 1 << q;
                        self.completion[usize::from(q)] = Some(Completion {
                            program,
                            slot,
                            left: source.preparation.payloads.len(),
                        });
                        self.slots[slot] = Some(Context {
                            program,
                            source,
                            d: None,
                            lod: None,
                            next: 0,
                        });
                        self.order.push_back(slot);
                        accepted = true;
                        self.stats.accepted += 1;
                        events.push(Event::Accept { program, slot });
                    }
                }
            }
        }
        self.stats.peak_contexts = self
            .stats
            .peak_contexts
            .max(self.slots.iter().filter(|s| s.is_some()).count());
        self.stats.peak_live = self.stats.peak_live.max(self.live.count_ones() as usize);
        self.stats.peak_coordinates = self.stats.peak_coordinates.max(self.coordinate_count);
        self.stats.peak_work = self.stats.peak_work.max(self.work_count);
        self.stats.peak_packet = self.stats.peak_packet.max(self.packet_count);
        if self.coordinate_count > self.hardware.coordinate_credits
            || self.work_count > self.hardware.work_credits
            || self.packet_count > self.hardware.packet_credits
            || self.coeff_ready.len() > 2
        {
            return Err("bound credit overflow".into());
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
