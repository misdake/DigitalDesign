//! Test-only single physical clock owner. No Service or average-latency engine.
use digital_design_hardware_gowin::sdram_memory_controller::{
    emu::{idle_inputs, Combination},
    ports::OracleImage,
};
use gpu_v2::{
    memory::ports::{MemoryPort, Request, Response, BURST_BEATS, BURST_BYTES},
    texture::ports::{RefillEvent, RefillPort},
};
use std::{cell::RefCell, fmt::Write, path::Path, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Client {
    Ro,
    FbRead,
    FbWrite,
    Display,
    Instruction,
    Data,
}
#[derive(Clone, Debug)]
pub struct Trace {
    pub physical: u64,
    pub delivered: Option<u64>,
    pub client: Client,
    pub id: u64,
    pub event: &'static str,
    pub index: u8,
    pub data: Option<u64>,
    pub last: bool,
    pub discarded: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub init_edges: u64,
    pub frame_edges: u64,
    pub ro_submitted: u64,
    pub ro_started: u64,
    pub ro_beats: u64,
    pub ro_terminals: u64,
    pub ro_delivered_beats: u64,
    pub ro_delivered_complete: u64,
    pub ro_discarded_beats: u64,
    pub ro_discarded_parents: u64,
    pub ro_cancelled_unpresented: u64,
    pub fb_reads: u64,
    pub fb_writes: u64,
    pub fb_read_beats: u64,
    pub fb_write_beats: u64,
    pub fb_terminals: u64,
    pub bg_submitted: [u64; 3],
    pub bg_skipped: [u64; 3],
    pub bg_completed: [u64; 3],
    pub max_ro_parents: usize,
    pub max_fb_outstanding: usize,
    pub max_bg_pending: [usize; 3],
    pub max_ro_records: usize,
    pub driver_failures: u64,
}
struct RoParent {
    id: u64,
    address: u64,
    eligible: u64,
    presented: bool,
    accepted: bool,
    next: u8,
    terminal: bool,
}
/// Exactly one physical-edge record, with at most one payload beat.
struct RoRecord {
    physical: u64,
    id: u64,
    started: bool,
    beat: Option<(u8, u64, bool)>,
    terminal: bool,
    error: bool,
}
struct FbActive {
    id: u64,
    request: Request,
    writes: u8,
    reads: u8,
}
struct Background {
    id: u64,
    address: u64,
    accepted: bool,
    reads: u8,
}
struct Hub {
    combination: Combination,
    max_edges: u64,
    texture_range: (u64, u64),
    background_base: u64,
    background_enabled: bool,
    background: [Option<Background>; 3],
    ro: Option<RoParent>,
    record: Option<RoRecord>,
    ro_polled: u64,
    discard_ro: bool,
    driver_fail_next: bool,
    physical_error: bool,
    tick_poison: Option<String>,
    fb: Option<FbActive>,
    held_request: Option<Request>,
    held_write: Option<u64>,
    next_id: u64,
    stats: Stats,
    trace: Vec<Trace>,
}
pub struct Shared {
    hub: Rc<RefCell<Hub>>,
}
pub struct RoView(Rc<RefCell<Hub>>);
pub struct FbView(Rc<RefCell<Hub>>);

impl Shared {
    pub fn new(
        image: OracleImage,
        texture_range: (u64, u64),
        background_base: u64,
        loaded: bool,
        max_edges: u64,
    ) -> Result<Self, String> {
        if max_edges == 0 || max_edges > 400_000 || texture_range.0 >= texture_range.1 {
            return Err("shared fixture bounds".into());
        }
        let background_end = background_base
            .checked_add(96)
            .ok_or("background range overflow")?;
        if !texture_range.0.is_multiple_of(128)
            || !texture_range.1.is_multiple_of(128)
            || !background_base.is_multiple_of(32)
            || texture_range.0 < background_end && texture_range.1 > background_base
        {
            return Err("shared immutable range alignment/overlap".into());
        }
        let mut combination = Combination::with_options(image, 21600, false, false)?;
        if !combination.bridge.pins.contains(
            texture_range.0,
            (texture_range.1 - texture_range.0) as usize,
        ) || !combination.bridge.pins.contains(background_base, 96)
        {
            return Err("shared fixture ranges".into());
        }
        for _ in 0..20_000 {
            if combination.bridge.output(false).initialized {
                break;
            }
            combination.tick(&idle_inputs())?;
        }
        if !combination.bridge.output(false).initialized {
            return Err("shared initialization watchdog".into());
        }
        let stats = Stats {
            init_edges: combination.cycle,
            ..Stats::default()
        };
        Ok(Self {
            hub: Rc::new(RefCell::new(Hub {
                combination,
                max_edges,
                texture_range,
                background_base,
                background_enabled: loaded,
                background: std::array::from_fn(|_| None),
                ro: None,
                record: None,
                ro_polled: 0,
                discard_ro: false,
                driver_fail_next: false,
                physical_error: false,
                tick_poison: None,
                fb: None,
                held_request: None,
                held_write: None,
                next_id: 0,
                stats,
                trace: Vec::new(),
            })),
        })
    }
    pub fn views(&self) -> (RoView, FbView) {
        (RoView(self.hub.clone()), FbView(self.hub.clone()))
    }
    pub fn stats(&self) -> Stats {
        self.hub.borrow().stats.clone()
    }
    pub fn trace(&self) -> Vec<Trace> {
        self.hub.borrow().trace.clone()
    }
    pub fn image(&self) -> Vec<u8> {
        self.hub.borrow().combination.bridge.pins.bytes().to_vec()
    }
    pub fn clocks(&self) -> (u64, u64) {
        let h = self.hub.borrow();
        (h.combination.cycle, h.combination.bridge.core_cycle)
    }
    pub fn ro_active(&self) -> bool {
        self.hub
            .borrow()
            .ro
            .as_ref()
            .is_some_and(|r| r.accepted && !r.terminal)
    }
    pub fn fb_write_active(&self) -> bool {
        self.hub
            .borrow()
            .fb
            .as_ref()
            .is_some_and(|f| f.request.write)
    }
    pub fn gpu_idle(&self) -> bool {
        let h = self.hub.borrow();
        h.ro.is_none()
            && h.record.is_none()
            && h.fb.is_none()
            && h.held_request.is_none()
            && h.held_write.is_none()
            && h.tick_poison.is_none()
    }
    pub fn idle(&self) -> bool {
        self.gpu_idle() && self.hub.borrow().background.iter().all(Option::is_none)
    }
    pub fn physical_error(&self) -> bool {
        self.hub.borrow().physical_error
    }
    pub fn tick_poisoned(&self) -> bool {
        self.hub.borrow().tick_poison.is_some()
    }
    pub fn stop_background(&self) {
        self.hub.borrow_mut().background_enabled = false;
    }
    /// Explicit driver fault, not a fabricated MC response or Refill Complete.
    pub fn fail_next_ro_poll(&self) {
        self.hub.borrow_mut().driver_fail_next = true;
    }
    pub fn abort_sampling(&self) {
        let mut h = self.hub.borrow_mut();
        h.discard_ro = true;
        h.background_enabled = false;
        if h.ro.as_ref().is_some_and(|r| !r.presented) {
            h.ro = None;
            h.stats.ro_cancelled_unpresented += 1;
            h.stats.ro_discarded_parents += 1;
        }
    }
    pub fn save(&self, directory: &Path, name: &str) {
        let h = self.hub.borrow();
        std::fs::create_dir_all(directory).unwrap();
        let mut csv = String::from(
            "physical_edge,delivered_edge,client,id,event,index,data,last,discarded\n",
        );
        for t in &h.trace {
            writeln!(
                csv,
                "{},{},{:?},{},{},{},{},{},{}",
                t.physical,
                t.delivered.map_or(String::new(), |v| v.to_string()),
                t.client,
                t.id,
                t.event,
                t.index,
                t.data.map_or(String::new(), |v| format!("{v:016x}")),
                t.last,
                t.discarded
            )
            .unwrap();
        }
        std::fs::write(directory.join(format!("{name}-mc.csv")), csv).unwrap();
        std::fs::write(
            directory.join(format!("{name}-stats.txt")),
            format!("{:#?}\nclocks={:?}\n", h.stats, self.clocks()),
        )
        .unwrap();
    }
}
impl Hub {
    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    fn log(&mut self, t: Trace) {
        assert!(
            self.trace.len() < (8 * self.max_edges) as usize,
            "trace watchdog"
        );
        self.trace.push(t);
    }
    fn consume_ro(&mut self, discard: bool) -> Result<Vec<RefillEvent>, String> {
        let edge = self.stats.frame_edges + 1;
        if self.ro_polled == edge {
            return Err("RO polled twice without physical edge".into());
        }
        self.ro_polled = edge;
        let mut result = Vec::new();
        if let Some(r) = self.record.take() {
            if r.physical + 1 != edge || self.ro.as_ref().map(|p| p.id) != Some(r.id) {
                return Err("RO return edge/owner".into());
            }
            for (event, index, data, last) in [
                r.started.then_some(("started", 0, None, false)),
                r.beat.map(|(i, d, last)| ("beat", i, Some(d), last)),
                r.terminal
                    .then_some((if r.error { "error" } else { "complete" }, 0, None, true)),
            ]
            .into_iter()
            .flatten()
            {
                self.log(Trace {
                    physical: r.physical,
                    delivered: Some(edge),
                    client: Client::Ro,
                    id: r.id,
                    event,
                    index,
                    data,
                    last,
                    discarded: discard || r.error,
                });
            }
            if r.started && !discard && !r.error {
                result.push(RefillEvent::Started { id: r.id });
            }
            if let Some((index, data, last)) = r.beat {
                if discard || r.error {
                    self.stats.ro_discarded_beats += 1;
                } else {
                    self.stats.ro_delivered_beats += 1;
                    result.push(RefillEvent::Beat {
                        id: r.id,
                        index: index as usize,
                        data,
                        last,
                    });
                }
            }
            if r.terminal {
                self.ro = None;
                if discard || r.error {
                    self.stats.ro_discarded_parents += 1;
                } else {
                    self.stats.ro_delivered_complete += 1;
                    result.push(RefillEvent::Complete { id: r.id });
                }
            }
            if r.error && !discard {
                return Err("physical RO terminal error; external discard/drain required".into());
            }
        }
        Ok(result)
    }
    fn cycle(&mut self, request: Option<Request>, write: Option<u64>) -> Result<Response, String> {
        if let Some(error) = &self.tick_poison {
            return Err(format!("shared edge poisoned; no retry: {error}"));
        }
        if self.stats.frame_edges >= self.max_edges {
            return Err("shared frame watchdog".into());
        }
        let edge = self.stats.frame_edges + 1;
        if self.ro_polled != edge || self.record.is_some() {
            return Err("RO return destination not consumed/reserved before MC edge".into());
        }
        if self.held_request.is_some() && self.held_request != request {
            return Err("blocked FB descriptor changed".into());
        }
        if self.held_write.is_some() && self.held_write != write {
            return Err("blocked FB write beat changed".into());
        }
        if self.fb.is_some() && request.is_some() {
            return Err("FB already outstanding".into());
        }
        if let Some(r) = request {
            r.validate()?;
            if !self
                .combination
                .bridge
                .pins
                .contains(r.address_bytes, BURST_BYTES)
            {
                return Err("FB request outside shared image".into());
            }
            let end = r.address_bytes + BURST_BYTES as u64;
            if r.address_bytes < self.texture_range.1 && end > self.texture_range.0
                || r.address_bytes < self.background_base + 96 && end > self.background_base
            {
                return Err("FB request overlaps immutable fixture range".into());
            }
        }
        // Everything above is read-only preflight. Below this boundary owner,
        // counters and pending returns can change before the native tick. Any
        // error is terminal even if the physical clock count has not advanced.
        let result = self.advance_edge(request, write);
        if let Err(error) = &result {
            if self.tick_poison.is_none() {
                self.tick_poison = Some(format!("protocol failure after Hub mutation: {error}"));
            }
        }
        result
    }
    fn advance_edge(
        &mut self,
        request: Option<Request>,
        write: Option<u64>,
    ) -> Result<Response, String> {
        let edge = self.stats.frame_edges + 1;
        let mut input = idle_inputs();
        input.display_response_ready = true;
        input.instruction_response_ready = true;
        input.data_response_ready = true;
        if let Some(r) = self
            .ro
            .as_mut()
            .filter(|r| !r.accepted && r.eligible <= edge)
        {
            input.gpu_ro_request_valid = true;
            input.gpu_ro_address = r.address / 2;
            input.gpu_ro_line_count_minus_1 = 3;
            r.presented = true;
        }
        let writing = self
            .fb
            .as_ref()
            .map(|a| a.request.write)
            .or(request.map(|r| r.write))
            .unwrap_or(false);
        if let Some(r) = request.filter(|r| !r.write || write.is_some()) {
            if r.write {
                input.gpu_fb_w_request_valid = true;
                input.gpu_fb_w_write = true;
                input.gpu_fb_w_address = r.address_bytes / 2;
                input.gpu_fb_w_line_count_minus_1 = 3;
            } else {
                input.gpu_fb_r_request_valid = true;
                input.gpu_fb_r_address = r.address_bytes / 2;
                input.gpu_fb_r_line_count_minus_1 = 3;
            }
        }
        input.gpu_fb_w_write_data = write.unwrap_or(0);
        for (i, period) in [128, 256, 512].into_iter().enumerate() {
            if self.background_enabled && (edge == 1 || edge.is_multiple_of(period)) {
                if self.background[i].is_some() {
                    self.stats.bg_skipped[i] += 1;
                } else {
                    let id = self.id();
                    self.background[i] = Some(Background {
                        id,
                        address: self.background_base + 32 * i as u64,
                        accepted: false,
                        reads: 0,
                    });
                    self.stats.bg_submitted[i] += 1;
                    self.stats.max_bg_pending[i] = 1;
                }
            }
            if let Some(b) = self.background[i].as_ref().filter(|b| !b.accepted) {
                match i {
                    0 => {
                        input.display_request_valid = true;
                        input.display_address = b.address / 2;
                    }
                    1 => {
                        input.instruction_request_valid = true;
                        input.instruction_address = b.address / 2;
                    }
                    2 => {
                        input.data_request_valid = true;
                        input.data_line = true;
                        input.data_address = b.address / 2;
                    }
                    _ => unreachable!(),
                }
            }
        }
        let o = self.combination.output(&input);
        let accepted = if writing {
            input.gpu_fb_w_request_valid && o.gpu_fb_w_request_ready
        } else {
            input.gpu_fb_r_request_valid && o.gpu_fb_r_request_ready
        };
        let mut response = Response {
            accepted,
            ..Response::default()
        };
        let mut records = Vec::new();
        if accepted {
            let id = self.id();
            let request = request.unwrap();
            self.fb = Some(FbActive {
                id,
                request,
                writes: 0,
                reads: 0,
            });
            self.stats.max_fb_outstanding = 1;
            if writing {
                self.stats.fb_writes += 1;
            } else {
                self.stats.fb_reads += 1;
            }
            records.push((
                if writing {
                    Client::FbWrite
                } else {
                    Client::FbRead
                },
                id,
                "started",
                0,
                None,
                false,
            ));
        }
        self.held_request = request.filter(|_| !accepted);
        if let Some(a) = &mut self.fb {
            let client = if a.request.write {
                Client::FbWrite
            } else {
                Client::FbRead
            };
            let terminal = if a.request.write {
                if o.gpu_fb_w_write_data_ready {
                    if write.is_none() || a.writes >= BURST_BEATS as u8 {
                        return Err("continuous FB write source underrun/overrun".into());
                    }
                    response.write_accepted = true;
                    records.push((client, a.id, "beat", a.writes, write, a.writes == 15));
                    a.writes += 1;
                    self.stats.fb_write_beats += 1;
                }
                o.gpu_fb_w_response_valid
            } else {
                if o.gpu_fb_r_response_valid && !o.gpu_fb_r_error {
                    if a.reads >= BURST_BEATS as u8 {
                        return Err("FB read overrun".into());
                    }
                    response.read = Some((a.reads, o.gpu_fb_r_read_data));
                    records.push((
                        client,
                        a.id,
                        "beat",
                        a.reads,
                        Some(o.gpu_fb_r_read_data),
                        a.reads == 15,
                    ));
                    a.reads += 1;
                    self.stats.fb_read_beats += 1;
                }
                o.gpu_fb_r_response_valid && (o.gpu_fb_r_response_last || o.gpu_fb_r_error)
            };
            if terminal {
                let error = if a.request.write {
                    o.gpu_fb_w_error
                } else {
                    o.gpu_fb_r_error
                };
                if !error && (if a.request.write { a.writes } else { a.reads }) != BURST_BEATS as u8
                {
                    return Err("FB terminal before sixteen beats".into());
                }
                self.physical_error |= error;
                response.complete = Some(!error);
                self.stats.fb_terminals += 1;
                records.push((
                    client,
                    a.id,
                    if error { "error" } else { "complete" },
                    0,
                    None,
                    true,
                ));
            }
        }
        self.held_write = write.filter(|_| {
            writing
                && !response.write_accepted
                && response.complete.is_none()
                && self
                    .fb
                    .as_ref()
                    .is_none_or(|a| a.writes < BURST_BEATS as u8)
        });
        if response.complete.is_some() {
            self.fb = None;
            self.held_write = None;
        }
        if let Some(r) = &mut self.ro {
            let started = input.gpu_ro_request_valid && o.gpu_ro_request_ready;
            if started {
                r.accepted = true;
                self.stats.ro_started += 1;
            }
            let mut beat = None;
            let terminal = o.gpu_ro_response_valid && (o.gpu_ro_response_last || o.gpu_ro_error);
            if o.gpu_ro_response_valid {
                if !r.accepted || r.terminal {
                    return Err("RO response without active physical owner".into());
                }
                if !o.gpu_ro_error {
                    if r.next >= 16 || o.gpu_ro_response_last != (r.next == 15) {
                        return Err("RO beat count/last".into());
                    }
                    beat = Some((r.next, o.gpu_ro_read_data, o.gpu_ro_response_last));
                    r.next += 1;
                    self.stats.ro_beats += 1;
                }
            }
            if terminal {
                r.terminal = true;
                self.stats.ro_terminals += 1;
            }
            if started || beat.is_some() || terminal {
                self.record = Some(RoRecord {
                    physical: edge,
                    id: r.id,
                    started,
                    beat,
                    terminal,
                    error: o.gpu_ro_error,
                });
                self.stats.max_ro_records = 1;
                if started {
                    records.push((Client::Ro, r.id, "started", 0, None, false));
                }
                if let Some((i, d, last)) = beat {
                    records.push((Client::Ro, r.id, "beat", i, Some(d), last));
                }
                if terminal {
                    records.push((
                        Client::Ro,
                        r.id,
                        if o.gpu_ro_error { "error" } else { "complete" },
                        0,
                        None,
                        true,
                    ));
                }
                self.physical_error |= o.gpu_ro_error;
            }
        } else if o.gpu_ro_response_valid || o.gpu_ro_request_ready {
            return Err("RO response/grant without parent".into());
        }
        for (i, (ready, valid, data, error, last)) in [
            (
                o.display_request_ready,
                o.display_response_valid,
                o.display_read_data,
                o.display_error,
                o.display_response_last,
            ),
            (
                o.instruction_request_ready,
                o.instruction_response_valid,
                o.instruction_read_data,
                o.instruction_error,
                false,
            ),
            (
                o.data_request_ready,
                o.data_response_valid,
                o.data_read_data,
                o.data_error,
                false,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let client = [Client::Display, Client::Instruction, Client::Data][i];
            if let Some(b) = &mut self.background[i] {
                if ready {
                    b.accepted = true;
                    records.push((client, b.id, "started", 0, None, false));
                }
                if valid {
                    if error || !b.accepted || b.reads >= 4 || i == 0 && last != (b.reads == 3) {
                        self.physical_error |= error;
                        return Err("background response protocol/error".into());
                    }
                    records.push((client, b.id, "beat", b.reads, Some(data), b.reads == 3));
                    b.reads += 1;
                    if b.reads == 4 {
                        records.push((client, b.id, "complete", 0, None, true));
                        self.background[i] = None;
                        self.stats.bg_completed[i] += 1;
                    }
                }
            } else if valid || ready {
                return Err("background response without owner".into());
            }
        }
        let old = self.combination.cycle;
        if let Err(error) = self.combination.tick(&input) {
            self.tick_poison = Some(error.clone());
            return Err(format!("physical tick failed; no retry: {error}"));
        }
        assert_eq!(self.combination.cycle, old + 1);
        assert_eq!(
            self.combination.bridge.core_cycle,
            2 * self.combination.cycle
        );
        self.stats.frame_edges += 1;
        for (client, id, event, index, data, last) in records {
            self.log(Trace {
                physical: edge,
                delivered: None,
                client,
                id,
                event,
                index,
                data,
                last,
                discarded: false,
            });
        }
        Ok(response)
    }
}
impl RoView {
    /// Caller supplies the return sink after Runtime has entered terminal fault.
    pub fn discard_edge(&mut self) -> Result<(), String> {
        self.0.borrow_mut().consume_ro(true).map(|_| ())
    }
}
impl RefillPort for RoView {
    fn submit_read(&mut self, address: u64, bytes: usize) -> Result<u64, String> {
        let mut h = self.0.borrow_mut();
        if h.discard_ro || h.physical_error || h.tick_poison.is_some() || h.ro.is_some() {
            return Err("RO parent credit/fault".into());
        }
        if bytes != 128
            || !address.is_multiple_of(128)
            || address < h.texture_range.0
            || address
                .checked_add(128)
                .is_none_or(|end| end > h.texture_range.1)
        {
            return Err("RO logical128B range/alignment".into());
        }
        if h.ro_polled != h.stats.frame_edges + 1 {
            return Err("RO submit before this edge poll".into());
        }
        let id = h.id();
        h.ro = Some(RoParent {
            id,
            address,
            eligible: h.stats.frame_edges + 2,
            presented: false,
            accepted: false,
            next: 0,
            terminal: false,
        });
        h.stats.ro_submitted += 1;
        h.stats.max_ro_parents = 1;
        Ok(id)
    }
    fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
        let mut h = self.0.borrow_mut();
        if h.driver_fail_next {
            h.driver_fail_next = false;
            h.stats.driver_failures += 1;
            return Err("driver RO poll failure before physical tick".into());
        }
        if h.discard_ro {
            return Err("faulted RO requires external discard sink".into());
        }
        h.consume_ro(false)
    }
}
impl MemoryPort for FbView {
    fn cycle(&mut self, request: Option<Request>, write: Option<u64>) -> Result<Response, String> {
        self.0.borrow_mut().cycle(request, write)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_parent_and_one_edge_sink_reject_double_admission_or_tick() {
        let s = Shared::new(
            OracleImage::filled::<0x57>(0, 0x20000).unwrap(),
            (0x10000, 0x10100),
            0x18000,
            false,
            200,
        )
        .unwrap();
        let (mut ro, mut fb) = s.views();
        ro.step().unwrap();
        let id = ro.submit_read(0x10000, 128).unwrap();
        assert!(ro
            .submit_read(0x10080, 128)
            .unwrap_err()
            .contains("parent credit"));
        assert_eq!(s.stats().ro_submitted, 1);
        fb.cycle(None, None).unwrap();
        assert_eq!(s.stats().ro_started, 0, "no submit-edge physical admission");
        let clocks = s.clocks();
        assert!(fb.cycle(None, None).unwrap_err().contains("destination"));
        assert_eq!(
            s.clocks(),
            clocks,
            "missing passive poll cannot cause second tick"
        );
        let mut complete = false;
        let mut beats = 0;
        for _ in 0..199 {
            for event in ro.step().unwrap() {
                match event {
                    RefillEvent::Beat {
                        id: owner,
                        index,
                        data,
                        last,
                    } => {
                        assert_eq!(owner, id);
                        assert_eq!(index, beats);
                        assert_eq!(data, 0x5757_5757_5757_5757);
                        assert_eq!(last, beats == 15);
                        beats += 1;
                    }
                    RefillEvent::Complete { id: owner } => {
                        assert_eq!(owner, id);
                        complete = true;
                    }
                    RefillEvent::Started { id: owner } => assert_eq!(owner, id),
                }
            }
            fb.cycle(None, None).unwrap();
            if complete {
                break;
            }
        }
        assert!(complete && s.idle());
        assert_eq!(beats, 16);
        assert_eq!(s.clocks().1, 2 * s.clocks().0);
    }

    #[test]
    fn source_underrun_after_ro_presentation_poison_prevents_retry() {
        let s = Shared::new(
            OracleImage::filled::<0x57>(0, 0x20000).unwrap(),
            (0x10000, 0x10080),
            0x18000,
            false,
            100,
        )
        .unwrap();
        let (mut ro, mut fb) = s.views();
        ro.step().unwrap();
        assert!(
            fb.cycle(
                Some(Request {
                    address_bytes: 512,
                    write: true
                }),
                Some(0x9876)
            )
            .unwrap()
            .accepted
        );
        ro.step().unwrap();
        ro.submit_read(0x10000, 128).unwrap();
        assert!(fb.cycle(None, Some(0x9876)).unwrap().write_accepted);
        ro.step().unwrap();
        assert!(!s.hub.borrow().ro.as_ref().unwrap().presented);
        let before = s.clocks();
        // Actual accepted-write supply violation, no fabricated MC response and
        // no vendor/controller state injection. The prior beat was consumed, so
        // this passes read-only blocked-beat preflight and fails inside evaluation.
        let error = fb.cycle(None, None).unwrap_err();
        assert!(error.contains("source underrun"), "{error}");
        assert_eq!(s.clocks(), before, "MC tick was not reached");
        assert!(
            s.hub.borrow().ro.as_ref().unwrap().presented,
            "Hub was partly changed"
        );
        assert!(s.tick_poisoned() && !s.physical_error());
        assert!(!s.gpu_idle() && !s.idle());
        let writes = s.stats().fb_write_beats;
        assert!(fb
            .cycle(None, Some(0x4321))
            .unwrap_err()
            .contains("no retry"));
        assert_eq!(s.clocks(), before);
        assert_eq!(s.stats().fb_write_beats, writes);
        assert_eq!(s.stats().ro_delivered_complete, 0);
    }

    #[test]
    fn partial_native_tick_failure_poison_prevents_retry() {
        let s = Shared::new(
            OracleImage::filled::<0x57>(0, 0x20000).unwrap(),
            (0x10000, 0x10080),
            0x18000,
            false,
            100,
        )
        .unwrap();
        let (mut ro, mut fb) = s.views();
        ro.step().unwrap();
        assert!(
            fb.cycle(
                Some(Request {
                    address_bytes: 512,
                    write: true
                }),
                Some(0x9876)
            )
            .unwrap()
            .accepted
        );
        ro.step().unwrap();
        // Deliberate controller-state corruption, labelled test-only. Native
        // state15 consumes continuous write data; there is no source reservation.
        // This calls the actual failing engine, not a fabricated error response.
        s.hub.borrow_mut().combination.bridge.controller.state = 15;
        let before = s.clocks();
        let error = fb.cycle(None, Some(0x9876)).unwrap_err();
        assert!(error.contains("physical tick failed"), "{error}");
        assert!(s.tick_poisoned());
        assert!(
            !s.gpu_idle() && !s.idle(),
            "failed physical state is not certified drained"
        );
        let after_failure = s.clocks();
        assert_eq!(before.0, after_failure.0);
        let retry = fb.cycle(None, None).unwrap_err();
        assert!(retry.contains("no retry"));
        assert_eq!(s.clocks(), after_failure);
    }
}
