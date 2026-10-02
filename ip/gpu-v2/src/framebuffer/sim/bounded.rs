//! Bounded synchronous-port control experiment. Pixel arithmetic calls the oracle;
//! this is deliberately not audited counted/timed arithmetic or an RTL simulator.
use super::{oracle, oracle::Pixel};
use crate::framebuffer::ports::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputRow {
    pub header: Header,
    pub row: u8,
    pub data: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Access {
    pub port: char,
    pub bank: usize,
    pub address: usize,
    pub write: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cycle {
    pub cycle: u64,
    pub ce: bool,
    pub input_accepted: bool,
    pub output_read: Option<usize>,
    pub accesses: Vec<Access>,
    pub request: Option<Request>,
    pub response: Response,
    pub committed: Option<Header>,
    pub payload_captured: Option<Header>,
    pub output_published: Option<Header>,
    pub arithmetic_issue: Option<(Header, usize)>,
    pub arithmetic_return: Option<(Header, usize)>,
    pub forwarded: Option<(Header, usize)>,
    pub stall: &'static str,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub cycles: u64,
    pub quads: u64,
    pub pixels: u64,
    pub hits: u64,
    pub misses: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub ce_stalls: u64,
    pub maintenance_stalls: u64,
    pub input_stalls: u64,
    pub serialized_wait: u64,
    pub raw_wait: u64,
    pub forwarded_lanes: u64,
    pub forward_fallbacks: u64,
    pub queue_peak: usize,
}
#[derive(Clone, Copy, Debug, Default)]
struct Line {
    tag: Option<u16>,
    dirty: [bool; 2],
}
#[derive(Clone, Copy, Debug)]
struct Maintenance {
    line: usize,
    target: Option<u16>,
    write: bool,
    plane: usize,
    sector: usize,
    accepted: bool,
    presented: bool,
    beats: u8,
    skid: Option<u64>,
}
#[derive(Clone, Debug)]
struct Rop {
    line: usize,
    phase: u8,
    header: Header,
    pending_row: u32,
    rgba: [[u8; 4]; 4],
    old: [Pixel; 4],
    result: [oracle::ResultPixel; 4],
}
#[derive(Clone, Debug)]
pub struct Model {
    surface: MaterializedSurface,
    context: Context,
    banks: [[u16; 1024]; 4],
    lines: [Line; 8],
    victim: usize,
    output: [[u32; 8]; 2],
    headers: [Option<Header>; 2],
    ready: [bool; 2],
    head: usize,
    tail: usize,
    fill_row: u8,
    rop: Option<Rop>,
    maintenance: Option<Maintenance>,
    flush: bool,
    pub flush_complete: bool,
    pub fault: bool,
    miss_wait: bool,
    pipeline: Option<Pipeline>,
    pub stats: Stats,
}
impl Model {
    pub fn new(surface: MaterializedSurface, context: Context) -> Result<Self, String> {
        surface.validate()?;
        Ok(Self {
            surface,
            context,
            banks: [[0; 1024]; 4],
            lines: [Line::default(); 8],
            victim: 0,
            output: [[0; 8]; 2],
            headers: [None; 2],
            ready: [false; 2],
            head: 0,
            tail: 0,
            fill_row: 0,
            rop: None,
            maintenance: None,
            flush: false,
            flush_complete: false,
            fault: false,
            miss_wait: false,
            pipeline: None,
            stats: Stats::default(),
        })
    }
    pub fn idle(&self) -> bool {
        self.rop.is_none()
            && self.pipeline.as_ref().is_none_or(Pipeline::idle)
            && self.maintenance.is_none()
            && !self.ready.iter().any(|v| *v)
            && self.fill_row == 0
    }
    pub fn drained(&self) -> bool {
        self.fault && self.idle()
    }
    pub fn request_flush(&mut self) -> Result<(), String> {
        if self.fill_row != 0 {
            return Err("flush inside unpublished output".into());
        }
        self.flush = true;
        self.flush_complete = false;
        Ok(())
    }
    pub fn abort(&mut self) {
        self.fault = true;
    }
    pub fn tags(&self) -> [Option<u16>; 8] {
        self.lines.map(|l| l.tag)
    }
    pub fn dirty(&self) -> [[bool; 2]; 8] {
        self.lines.map(|l| l.dirty)
    }
    #[allow(clippy::too_many_arguments)] // Explicit physical port/plane/line/coordinate tuple.
    fn access(
        &mut self,
        t: &mut Cycle,
        port: char,
        plane: usize,
        line: usize,
        x: usize,
        y: usize,
        value: Option<u16>,
    ) -> u16 {
        let (bank, address) = bank_address(plane, line, x, y);
        assert!(
            !t.accesses
                .iter()
                .any(|a| a.bank == bank && (a.port == port || a.address == address)),
            "port overbook or same-address dual-port collision"
        );
        t.accesses.push(Access {
            port,
            bank,
            address,
            write: value.is_some(),
        });
        let old = self.banks[bank][address];
        if let Some(v) = value {
            self.banks[bank][address] = v;
        }
        old
    }
    fn beat(&mut self, t: &mut Cycle, m: Maintenance, data: Option<u64>) -> u64 {
        let word = m.sector * 64 + usize::from(m.beats) * 4;
        let mut out = 0;
        for i in 0..4 {
            let p = word + i;
            out |= u64::from(self.access(
                t,
                'B',
                m.plane,
                m.line,
                p % 16,
                p / 16,
                data.map(|v| (v >> (i * 16)) as u16),
            )) << (i * 16);
        }
        out
    }
    fn begin(&mut self, line: usize, target: Option<u16>) {
        let dirty = self.lines[line].dirty.iter().position(|v| *v);
        if dirty.is_none() {
            self.lines[line].tag = None;
        }
        self.maintenance = Some(Maintenance {
            line,
            target,
            write: dirty.is_some(),
            plane: dirty.unwrap_or(0),
            sector: 0,
            accepted: false,
            presented: false,
            beats: 0,
            skid: None,
        });
    }
    fn after_ack(&mut self, mut m: Maintenance) {
        m.accepted = false;
        m.presented = false;
        m.beats = 0;
        m.skid = None;
        if m.sector < 3 {
            m.sector += 1;
            self.maintenance = Some(m);
            return;
        }
        if m.write {
            self.lines[m.line].dirty[m.plane] = false;
            if let Some(p) = self.lines[m.line].dirty.iter().position(|v| *v) {
                m.plane = p;
                m.sector = 0;
                self.maintenance = Some(m);
            } else if m.target.is_some() {
                self.lines[m.line].tag = None;
                m.write = false;
                m.plane = 0;
                m.sector = 0;
                self.maintenance = Some(m);
            }
        } else if m.plane == 0 {
            m.plane = 1;
            m.sector = 0;
            self.maintenance = Some(m);
        } else {
            self.lines[m.line].tag = m.target;
        }
    }
    pub fn step(
        &mut self,
        ce: bool,
        input: Option<OutputRow>,
        memory: &mut impl MemoryPort,
    ) -> Result<Cycle, String> {
        self.stats.cycles += 1;
        let mut t = Cycle {
            cycle: self.stats.cycles,
            ce,
            ..Cycle::default()
        };
        if self.fault {
            self.rop = None;
            if let Some(p) = &mut self.pipeline {
                p.clear();
            }
            self.ready = [false; 2];
            self.headers = [None; 2];
            self.fill_row = 0;
            if self
                .maintenance
                .is_some_and(|m| !m.accepted && !m.presented)
            {
                self.maintenance = None;
            }
        }
        // Maintenance is wall-clock owned, never gated by compute CE.
        if let Some(mut m) = self.maintenance.take() {
            let tile = if m.write {
                self.lines[m.line].tag.expect("dirty tag")
            } else {
                m.target.expect("refill target")
            };
            let request = Request {
                address_bytes: self.surface.address(m.plane, tile, m.sector * 64),
                write: m.write,
            };
            let offered = if !m.accepted && (!m.write || m.skid.is_some()) {
                Some(request)
            } else {
                None
            };
            m.presented |= offered.is_some();
            // Public transport requires the prefetched first word on admission.
            let response = match memory.cycle(offered, m.skid) {
                Ok(r) => r,
                Err(e) => {
                    self.maintenance = Some(m);
                    self.fault = true;
                    return Err(e);
                }
            };
            t.request = offered;
            t.response = response;
            if response.accepted {
                assert!(!m.accepted);
                m.accepted = true;
            }
            if let Some((index, data)) = response.read {
                assert!(m.accepted && !m.write && index == m.beats);
                self.beat(&mut t, m, Some(data));
                m.beats += 1;
                self.stats.read_bytes += 8;
            }
            if response.write_accepted {
                assert!(m.accepted && m.write && m.skid.is_some());
                m.skid = None;
                m.beats += 1;
                self.stats.write_bytes += 8;
            }
            if let Some(success) = response.complete {
                assert!(m.accepted && (!success || m.beats == 16));
                if !success {
                    self.fault = true;
                }
                if !self.fault {
                    self.after_ack(m);
                }
            } else {
                // One synchronous B-port capture register: present on the next cycle.
                if m.write && m.skid.is_none() && m.beats < 16 {
                    m.skid = Some(self.beat(&mut t, m, None));
                }
                self.maintenance = Some(m);
            }
            t.stall = "maintenance";
            self.stats.maintenance_stalls += 1;
        } else {
            t.response = memory.cycle(None, None)?;
            assert_eq!(t.response, Response::default());
        }
        if self.fault {
            self.rop = None;
            if let Some(p) = &mut self.pipeline {
                p.clear();
            }
            self.ready = [false; 2];
            self.headers = [None; 2];
            self.fill_row = 0;
            return Ok(t);
        }
        if !ce {
            t.stall = "ce";
            self.stats.ce_stalls += 1;
            return Ok(t);
        }
        if let Some(row) = input {
            if !self.flush && !self.ready[self.tail] {
                self.surface.validate_header(row.header)?;
                if row.row != self.fill_row
                    || (row.row & 1 == 1 && row.data >> 16 != 0)
                    || (self.fill_row != 0 && self.headers[self.tail] != Some(row.header))
                {
                    return Err("output row sequence/header/reserved bits".into());
                }
                if self.fill_row == 0 {
                    self.headers[self.tail] = Some(row.header);
                }
                self.output[self.tail][usize::from(row.row)] = row.data;
                self.fill_row += 1;
                t.input_accepted = true;
                if self.fill_row == 8 {
                    t.output_published = Some(row.header);
                    self.ready[self.tail] = true;
                    self.tail ^= 1;
                    self.fill_row = 0;
                }
            } else {
                self.stats.input_stalls += 1;
            }
        }
        self.stats.queue_peak = self
            .stats
            .queue_peak
            .max(self.ready.iter().filter(|v| **v).count() + usize::from(self.fill_row != 0));
        if self.maintenance.is_some() || t.stall == "maintenance" {
            return Ok(t);
        }
        if self.pipeline.is_some() {
            self.step_pipeline(&mut t);
            return Ok(t);
        }
        if let Some(mut r) = self.rop.take() {
            if self.ready[self.head ^ 1] {
                self.stats.serialized_wait += 1;
                let next = self.headers[self.head ^ 1].unwrap();
                if next.x == r.header.x && next.y == r.header.y && next.mask & r.header.mask != 0 {
                    self.stats.raw_wait += 1;
                }
            }
            let p = r.phase;
            // Reads have one enabled-cycle latency; data is held across CE pauses.
            if (1..=8).contains(&p) {
                let row = p - 1;
                let lane = usize::from(row / 2);
                if row & 1 == 0 {
                    r.rgba[lane] = r.pending_row.to_le_bytes();
                } else {
                    r.result[lane] = oracle::pixel(
                        r.old[lane],
                        Fragment {
                            rgba: r.rgba[lane],
                            depth: r.pending_row as u16,
                        },
                        r.header.mask & (1 << lane) != 0,
                        self.context,
                    );
                }
            }
            if p < 8 {
                t.output_read = Some(self.head * 8 + usize::from(p));
                r.pending_row = self.output[self.head][usize::from(p)];
            }
            if p == 0 || p == 1 || p == 9 || p == 10 {
                let plane = usize::from(p == 0 || p == 10);
                for lane in 0..4 {
                    if r.header.mask & (1 << lane) == 0 {
                        continue;
                    }
                    let x = usize::from(r.header.x % 16) + lane % 2;
                    let y = usize::from(r.header.y % 16) + lane / 2;
                    let result = r.result[lane];
                    let value = if p == 9 && result.color_written {
                        Some(result.pixel.color)
                    } else if p == 10 && result.depth_written {
                        Some(result.pixel.depth)
                    } else {
                        None
                    };
                    if p >= 9 && value.is_none() {
                        continue;
                    }
                    let old = self.access(&mut t, 'A', plane, r.line, x, y, value);
                    if p == 0 {
                        r.old[lane].depth = old;
                    } else if p == 1 {
                        r.old[lane].color = old;
                    } else {
                        self.lines[r.line].dirty[plane] = true;
                    }
                }
            }
            if p == 11 {
                self.ready[self.head] = false;
                self.headers[self.head] = None;
                self.head ^= 1;
                self.stats.quads += 1;
                self.stats.pixels += u64::from(r.header.mask.count_ones());
                t.committed = Some(r.header);
            } else {
                r.phase += 1;
                self.rop = Some(r);
            }
        } else if self.ready[self.head] {
            let header = self.headers[self.head].unwrap();
            let tile = self.surface.tile(header);
            if let Some(line) = self.lines.iter().position(|l| l.tag == Some(tile)) {
                if !self.miss_wait {
                    self.stats.hits += 1;
                }
                self.miss_wait = false;
                let pixel = Pixel { color: 0, depth: 0 };
                self.rop = Some(Rop {
                    line,
                    phase: 0,
                    header,
                    pending_row: 0,
                    rgba: [[0; 4]; 4],
                    old: [pixel; 4],
                    result: [oracle::ResultPixel {
                        pixel,
                        color_written: false,
                        depth_written: false,
                    }; 4],
                });
            } else {
                self.stats.misses += 1;
                self.miss_wait = true;
                let line = self
                    .lines
                    .iter()
                    .position(|l| l.tag.is_none())
                    .unwrap_or(self.victim);
                self.victim = (line + 1) % 8;
                self.begin(line, Some(tile));
            }
        } else if self.flush && self.fill_row == 0 {
            if let Some(line) = self.lines.iter().position(|l| l.dirty.iter().any(|d| *d)) {
                self.begin(line, None);
            } else {
                self.flush = false;
                self.flush_complete = true;
            }
        }
        Ok(t)
    }
}

/// Fixed two-enabled-cycle arithmetic-result fixture. Values come from the oracle;
/// the two pipeline registers certify only control lifetime, never DSP timing.
pub const PIPELINE_ARITHMETIC_LATENCY: usize = 2;
#[derive(Clone, Copy, Debug)]
struct Descriptor {
    header: Header,
    line: usize,
    output_slot: usize,
    age: u8,
    dependency_mask: u8,
    dependency_owner: usize,
}
#[derive(Clone, Copy, Debug)]
struct ReturnRow {
    data: u32,
    row: u8,
    descriptor: usize,
}
#[derive(Clone, Copy, Debug)]
struct ArithmeticToken {
    lane: usize,
    descriptor: usize,
    value: oracle::ResultPixel,
}
#[derive(Clone, Debug)]
struct Pipeline {
    descriptors: [Option<Descriptor>; 2],
    phase: u8,
    row: Option<ReturnRow>,
    rgba: [u8; 4],
    old: [Pixel; 4],
    result: [oracle::ResultPixel; 4],
    result_owner: [Option<usize>; 4],
    forwarding: bool,
    force_stall: bool,
    arithmetic: [Option<ArithmeticToken>; PIPELINE_ARITHMETIC_LATENCY],
}
impl Pipeline {
    fn new() -> Self {
        let pixel = Pixel { color: 0, depth: 0 };
        Self {
            descriptors: [None; 2],
            phase: 0,
            row: None,
            rgba: [0; 4],
            old: [pixel; 4],
            result: [oracle::ResultPixel {
                pixel,
                color_written: false,
                depth_written: false,
            }; 4],
            result_owner: [None; 4],
            forwarding: false,
            force_stall: false,
            arithmetic: [None; PIPELINE_ARITHMETIC_LATENCY],
        }
    }
    fn idle(&self) -> bool {
        self.descriptors.iter().all(Option::is_none)
            && self.row.is_none()
            && self.arithmetic.iter().all(Option::is_none)
    }
    fn clear(&mut self) {
        self.descriptors = [None; 2];
        self.row = None;
        self.arithmetic = [None; PIPELINE_ARITHMETIC_LATENCY];
        self.result_owner = [None; 4];
        self.force_stall = false;
    }
}
impl Model {
    /// Same cache/output capacity as the serial baseline, with an explicit fixed
    /// two-cycle arithmetic-return fixture and shared per-lane retained storage.
    pub fn new_pipelined(surface: MaterializedSurface, context: Context) -> Result<Self, String> {
        let mut m = Self::new(surface, context)?;
        m.pipeline = Some(Pipeline::new());
        Ok(m)
    }
    /// Enable bounded lane forwarding; the unforwarded constructor remains the
    /// conservative RAW-stall comparison. Arithmetic delay is unchanged.
    pub fn new_forwarding(surface: MaterializedSurface, context: Context) -> Result<Self, String> {
        let mut m = Self::new_pipelined(surface, context)?;
        m.pipeline.as_mut().unwrap().forwarding = true;
        Ok(m)
    }
    fn step_pipeline(&mut self, t: &mut Cycle) {
        let mut p = self.pipeline.take().unwrap();
        // Combinational aliases of the pre-edge FF outputs, NOT retained copies.
        // A return written on this edge cannot satisfy a same-edge dependency.
        let results_at_edge = p.result;
        let owners_at_edge = p.result_owner;
        let mut restarted_this_edge = false;
        // Old result reads precede next-quad writes to the same result FF bank.
        // In particular, old depth at age 12 is captured before new lane 0 returns.
        for index in 0..2 {
            if let Some(d) = p.descriptors[index] {
                if d.age == 11 || d.age == 12 {
                    let plane = usize::from(d.age == 12);
                    for lane in 0..4 {
                        let result = p.result[lane];
                        let written = if plane == 0 {
                            result.color_written
                        } else {
                            result.depth_written
                        };
                        if d.header.mask & (1 << lane) != 0 && written {
                            self.access(
                                t,
                                'A',
                                plane,
                                d.line,
                                usize::from(d.header.x % 16) + lane % 2,
                                usize::from(d.header.y % 16) + lane / 2,
                                Some(if plane == 0 {
                                    result.pixel.color
                                } else {
                                    result.pixel.depth
                                }),
                            );
                            self.lines[d.line].dirty[plane] = true;
                        }
                    }
                }
                if d.age == 13 {
                    assert!(t.committed.is_none());
                    t.committed = Some(d.header);
                    self.stats.quads += 1;
                    self.stats.pixels += u64::from(d.header.mask.count_ones());
                    p.descriptors[index] = None;
                }
            }
        }
        if let Some(token) = p.arithmetic[0].take() {
            t.arithmetic_return = Some((
                p.descriptors[token.descriptor]
                    .expect("arithmetic return owner")
                    .header,
                token.lane,
            ));
            // Masked lanes do not replace a live result/owner. A covered lane
            // still publishes its final preserved pixel after depth rejection.
            if p.descriptors[token.descriptor].unwrap().header.mask & (1 << token.lane) != 0 {
                p.result[token.lane] = token.value;
                p.result_owner[token.lane] = Some(token.descriptor);
            }
        }
        p.arithmetic.rotate_left(1);
        p.arithmetic[PIPELINE_ARITHMETIC_LATENCY - 1] = None;
        // Consume old lane 3 before the next quad's synchronous depth capture
        // reuses the shared old-value array on the very same edge.
        if let Some(row) = p.row.take() {
            let d = p.descriptors[row.descriptor].expect("return owner remains live");
            let lane = usize::from(row.row / 2);
            let dependency = d.dependency_mask & (1 << lane) != 0;
            let missing =
                row.row & 1 == 1 && dependency && owners_at_edge[lane] != Some(d.dependency_owner);
            if missing {
                // No consumer cache writes occur before age 11. Keep its output
                // slot, discard only its speculative arithmetic and replay after
                // the producer commits. Producer clocks must keep advancing.
                assert!(d.age <= 8 && self.ready[d.output_slot]);
                p.descriptors[row.descriptor] = None;
                for token in &mut p.arithmetic {
                    if token.is_some_and(|v| v.descriptor == row.descriptor) {
                        *token = None;
                    }
                }
                p.force_stall = true;
                restarted_this_edge = true;
                self.stats.forward_fallbacks += 1;
                t.stall = "forward_restart";
            } else if row.row & 1 == 0 {
                p.rgba = row.data.to_le_bytes();
            } else {
                let old = if dependency {
                    self.stats.forwarded_lanes += 1;
                    t.forwarded = Some((d.header, lane));
                    results_at_edge[lane].pixel
                } else {
                    p.old[lane]
                };
                let value = oracle::pixel(
                    old,
                    Fragment {
                        rgba: p.rgba,
                        depth: row.data as u16,
                    },
                    d.header.mask & (1 << lane) != 0,
                    self.context,
                );
                p.arithmetic[PIPELINE_ARITHMETIC_LATENCY - 1] = Some(ArithmeticToken {
                    lane,
                    descriptor: row.descriptor,
                    value,
                });
                t.arithmetic_issue = Some((d.header, lane));
            }
            if row.row == 7 && !missing {
                assert_eq!(d.output_slot, self.head);
                self.ready[self.head] = false;
                self.headers[self.head] = None;
                self.head ^= 1;
                t.payload_captured = Some(d.header);
            }
        }
        let reading_head = p
            .descriptors
            .iter()
            .flatten()
            .any(|d| d.output_slot == self.head && d.age <= 8);
        let hazard = self.ready[self.head]
            && !reading_head
            && p.descriptors.iter().flatten().any(|d| {
                let h = self.headers[self.head].unwrap();
                h.x == d.header.x && h.y == d.header.y && h.mask & d.header.mask != 0
            });
        if hazard && (!p.forwarding || p.force_stall) {
            self.stats.raw_wait += 1;
            if !restarted_this_edge {
                t.stall = "raw";
            }
        }
        // Header/tag predecode is folded into this admission edge. The final row
        // publication can enable a first-row read on the same edge: these are
        // distinct RAM addresses and the first seven rows are already stored.
        let restart_blocked = p.force_stall && !p.idle();
        if p.phase == 0
            && self.ready[self.head]
            && !reading_head
            && (!hazard || p.forwarding)
            && !restart_blocked
            && !restarted_this_edge
        {
            let header = self.headers[self.head].unwrap();
            let tile = self.surface.tile(header);
            if let Some(line) = self.lines.iter().position(|l| l.tag == Some(tile)) {
                let index = p
                    .descriptors
                    .iter()
                    .position(Option::is_none)
                    .expect("two descriptor credit");
                assert!(!p.descriptors.iter().flatten().any(|d| d.age < 8));
                let dependency = p.descriptors.iter().enumerate().find_map(|(owner, d)| {
                    d.filter(|d| d.header.x == header.x && d.header.y == header.y)
                        .map(|d| (owner, d.header.mask & header.mask))
                });
                p.descriptors[index] = Some(Descriptor {
                    header,
                    line,
                    output_slot: self.head,
                    age: 0,
                    dependency_owner: dependency.map_or(0, |v| v.0),
                    dependency_mask: if p.forwarding {
                        dependency.map_or(0, |v| v.1)
                    } else {
                        0
                    },
                });
                if !self.miss_wait && !p.force_stall {
                    self.stats.hits += 1;
                }
                self.miss_wait = false;
                p.force_stall = false;
            } else if p.idle() {
                self.stats.misses += 1;
                self.miss_wait = true;
                let line = self
                    .lines
                    .iter()
                    .position(|l| l.tag.is_none())
                    .unwrap_or(self.victim);
                self.victim = (line + 1) % 8;
                self.begin(line, Some(tile));
            }
        }
        for (index, d) in p.descriptors.iter_mut().enumerate() {
            if let Some(d) = d {
                if d.age < 8 {
                    assert!(t.output_read.is_none());
                    t.output_read = Some(d.output_slot * 8 + usize::from(d.age));
                    p.row = Some(ReturnRow {
                        data: self.output[d.output_slot][usize::from(d.age)],
                        row: d.age,
                        descriptor: index,
                    });
                }
                if d.age == 0 || d.age == 1 {
                    let plane = usize::from(d.age == 0);
                    for lane in 0..4 {
                        if d.header.mask & (1 << lane) == 0 {
                            continue;
                        }
                        let value = self.access(
                            t,
                            'A',
                            plane,
                            d.line,
                            usize::from(d.header.x % 16) + lane % 2,
                            usize::from(d.header.y % 16) + lane / 2,
                            None,
                        );
                        if plane == 0 {
                            p.old[lane].color = value;
                        } else {
                            p.old[lane].depth = value;
                        }
                    }
                }
                d.age += 1;
            }
        }
        if self.flush && !self.ready.iter().any(|v| *v) && self.fill_row == 0 && p.idle() {
            if let Some(line) = self.lines.iter().position(|l| l.dirty.iter().any(|d| *d)) {
                self.begin(line, None);
            } else {
                self.flush = false;
                self.flush_complete = true;
            }
        }
        p.phase = (p.phase + 1) % 8;
        self.pipeline = Some(p);
    }
}

#[cfg(test)]
mod forwarding_fallback_tests {
    use super::*;
    use crate::framebuffer::sim::fixture::{replay, Fixture};

    #[test]
    fn unavailable_or_wrong_owner_restarts_at_each_lane_without_releasing_payload() {
        let surface = MaterializedSurface {
            color_base_bytes: 0,
            depth_base_bytes: 512,
            width: 16,
            height: 16,
        };
        let context = Context {
            depth: DepthFunc::Always,
            depth_write: true,
            blend: Blend::SrcOver,
        };
        let quads: Vec<_> = (0..80)
            .map(|i| Quad {
                header: Header {
                    x: 0,
                    y: 0,
                    mask: 15,
                },
                pixels: std::array::from_fn(|lane| Fragment {
                    rgba: [i as u8 * 3, lane as u8 * 53, 230, 91],
                    depth: 1000 + i as u16 * 7 + lane as u16,
                }),
            })
            .collect();
        let bytes: Vec<_> = (0..1024).map(|i| (i * 37 + 19) as u8).collect();
        let (_, reference, _) = replay(
            surface,
            context,
            &quads,
            Fixture::new(bytes.clone()),
            0,
            30_000,
        )
        .unwrap();
        for row_index in [1, 3, 5, 7] {
            for wrong_owner in [false, true] {
                let mut model = Model::new_forwarding(surface, context).unwrap();
                let mut memory = Fixture::new(bytes.clone());
                let mut cursor = 0;
                let mut flushing = false;
                let mut injected = false;
                let mut captures = 0;
                for _ in 0..30_000 {
                    let pipe = model.pipeline.as_mut().unwrap();
                    if !injected {
                        if let Some(row) = pipe.row {
                            let d = pipe.descriptors[row.descriptor].unwrap();
                            if row.row == row_index && d.dependency_mask != 0 {
                                let lane = usize::from(row.row / 2);
                                pipe.result_owner[lane] = if wrong_owner {
                                    Some(d.dependency_owner ^ 1)
                                } else {
                                    None
                                };
                                injected = true;
                            }
                        }
                    }
                    let old_fallbacks = model.stats.forward_fallbacks;
                    let input = quads.get(cursor / 8).map(|q| OutputRow {
                        header: q.header,
                        row: (cursor % 8) as u8,
                        data: q.rows()[cursor % 8],
                    });
                    let t = model.step(true, input, &mut memory).unwrap();
                    if t.input_accepted {
                        cursor += 1;
                    }
                    if t.payload_captured.is_some() {
                        captures += 1;
                    }
                    if model.stats.forward_fallbacks != old_fallbacks {
                        assert!(t.payload_captured.is_none() && t.output_read.is_none());
                        assert!(model.ready[model.head]);
                        assert_eq!(model.headers[model.head], Some(quads[1].header));
                    }
                    if cursor == quads.len() * 8 && !flushing {
                        model.request_flush().unwrap();
                        flushing = true;
                    }
                    if model.flush_complete {
                        break;
                    }
                }
                assert!(injected && model.flush_complete);
                assert_eq!(model.stats.forward_fallbacks, 1);
                assert_eq!(captures, 80);
                assert_eq!(model.stats.quads, 80);
                assert_eq!(memory.bytes, reference.bytes);
            }
        }
    }
}
