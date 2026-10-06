//! Independent synchronous-port cache control with actual registered ROP.
//! The serial control calendar preserves the existing bounded transport ABI.
//! Memory is wall-clock owned; only compute/ingress/leaf use CE. No numerical
//! oracle, counted answer or precomputed delayed-result queue is executed here.
use super::arithmetic::{LeafInput, LeafTick, Pipeline, Pixel, ResultPixel};
use super::ports::*;
pub use super::sim::bounded::{Access, Cycle, OutputRow, Stats};

/// Finite logical state owned by this emulator, not Rust host sizes or a fit.
/// The ROP phase reaches 16, so its declaration needs five bits. The actual
/// source-depth and issue/return masks are charged separately from old pixels.
pub const CACHE_DATA_BITS: usize = 4 * 1024 * 16;
pub const OUTPUT_DATA_BITS: usize = 2 * 8 * 32;
pub const TAG_BITS: usize = 8 * (16 + 1 + 2);
pub const MAINTENANCE_BITS: usize = 3 + 17 + 1 + 1 + 2 + 1 + 1 + 5 + 65;
pub const ROP_STATE_BITS: usize = 3 + 5 + 21 + 32 + 128 + 128 + 136 + 64 + 4 + 4;
pub const OUTPUT_CONTROL_BITS: usize = 42 + 2 + 2 + 1 + 1 + 4;
pub const CONFIG_BITS: usize = 64 + 10 + 9 + 3 + 1 + 1;
pub const CONTROL_BITS: usize = 3 + 1 + 1 + 1 + 1 + 1 + 1;
/// Eight typed leaf positions, including lane identity and valid ownership.
pub const LEAF_REGISTER_BITS: usize = 797;
pub const TOTAL_LOGICAL_BITS: usize = CACHE_DATA_BITS
    + OUTPUT_DATA_BITS
    + TAG_BITS
    + MAINTENANCE_BITS
    + ROP_STATE_BITS
    + OUTPUT_CONTROL_BITS
    + CONFIG_BITS
    + CONTROL_BITS
    + LEAF_REGISTER_BITS;

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
    result: [ResultPixel; 4],
    source_depth: [u16; 4],
    issued: u8,
    returned: u8,
}
#[derive(Clone, Debug)]
pub struct FramebufferEmu {
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
    leaf: Pipeline,
    max_wall: u64,
    pub stats: Stats,
}
impl FramebufferEmu {
    pub fn new(
        surface: MaterializedSurface,
        context: Context,
        max_wall: u64,
    ) -> Result<Self, String> {
        if max_wall == 0 || max_wall > 2_000_000 {
            return Err("framebuffer wall bound".into());
        }
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
            leaf: Pipeline::new(max_wall)?,
            max_wall,
            stats: Stats::default(),
        })
    }
    pub fn idle(&self) -> bool {
        self.rop.is_none()
            && self.leaf.idle()
            && self.maintenance.is_none()
            && !self.ready.iter().any(|v| *v)
            && self.fill_row == 0
    }
    /// Resident tiles survive a state change; old queued/partial/active work
    /// must have retired before the immutable draw context can be replaced.
    pub fn set_context(&mut self, context: Context) -> Result<(), String> {
        if self.fault || !self.idle() || self.flush {
            return Err("framebuffer context still owned".into());
        }
        self.context = context;
        Ok(())
    }
    pub fn drained(&self) -> bool {
        self.fault && self.idle()
    }
    pub fn request_flush(&mut self) -> Result<(), String> {
        if self.fault || self.fill_row != 0 {
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
        let result = self.advance(ce, input, memory);
        if result.is_err() {
            // The memory view may already have advanced; caller must not retry
            // this edge. Accepted transport can be drained with subsequent
            // step(None) edges, or through the test adapter's abort owner.
            self.fault = true;
        }
        result
    }
    fn advance(
        &mut self,
        ce: bool,
        input: Option<OutputRow>,
        memory: &mut impl MemoryPort,
    ) -> Result<Cycle, String> {
        if self.stats.cycles >= self.max_wall {
            return Err("framebuffer wall watchdog".into());
        }
        self.stats.cycles += 1;
        let old_ready = self.ready[self.head];
        let mut t = Cycle {
            cycle: self.stats.cycles,
            ce,
            ..Cycle::default()
        };
        if self.fault {
            self.rop = None;
            self.leaf.reset();
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
        let leaf_input = if ce && !self.fault && self.maintenance.is_none() {
            self.rop
                .as_ref()
                .filter(|r| (9..=12).contains(&r.phase))
                .map(|r| {
                    let lane = usize::from(r.phase - 9);
                    LeafInput {
                        key: lane as u8,
                        old: r.old[lane],
                        source: Fragment {
                            rgba: r.rgba[lane],
                            depth: r.source_depth[lane],
                        },
                        covered: r.header.mask & (1 << lane) != 0,
                        depth: self.context.depth,
                        depth_write: self.context.depth_write,
                        blend: self.context.blend,
                    }
                })
        } else {
            None
        };
        let leaf = self.leaf.tick(LeafTick {
            ce: ce && !self.fault,
            input: leaf_input,
        })?;
        if leaf.accepted {
            let input = leaf_input.ok_or("leaf accepted without input")?;
            let r = self.rop.as_mut().ok_or("leaf issue owner")?;
            if r.issued & (1 << input.key) != 0 {
                return Err("duplicate leaf issue".into());
            }
            r.issued |= 1 << input.key;
            t.arithmetic_issue = Some((r.header, usize::from(input.key)));
        }
        if leaf.returned {
            let output = leaf.output.ok_or("leaf lost result")?;
            let r = self.rop.as_mut().ok_or("leaf return owner")?;
            let bit = 1 << output.key;
            if r.issued & bit == 0 || r.returned & bit != 0 {
                return Err("unsolicited or duplicate leaf return".into());
            }
            r.result[usize::from(output.key)] = output.result;
            r.returned |= bit;
            t.arithmetic_return = Some((r.header, usize::from(output.key)));
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
                if m.accepted || offered.is_none() {
                    self.maintenance = Some(m);
                    return Err("memory accepted without held request".into());
                }
                m.accepted = true;
            }
            if let Some((index, data)) = response.read {
                if !m.accepted || m.write || index != m.beats || m.beats >= 16 {
                    self.maintenance = Some(m);
                    return Err("memory read beat ownership/index".into());
                }
                self.beat(&mut t, m, Some(data));
                m.beats += 1;
                self.stats.read_bytes += 8;
            }
            if response.write_accepted {
                if !m.accepted || !m.write || m.skid.is_none() || m.beats >= 16 {
                    self.maintenance = Some(m);
                    return Err("memory write beat without offered data".into());
                }
                m.skid = None;
                m.beats += 1;
                self.stats.write_bytes += 8;
            }
            if let Some(success) = response.complete {
                if !m.accepted || (success && m.beats != 16) {
                    self.maintenance = Some(m);
                    return Err("memory terminal ACK ownership/beat count".into());
                }
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
            if t.response != Response::default() {
                return Err("unsolicited framebuffer memory response".into());
            }
        }
        if self.fault {
            self.rop = None;
            self.leaf.reset();
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
                    r.source_depth[lane] = r.pending_row as u16;
                }
            }
            if p < 8 {
                t.output_read = Some(self.head * 8 + usize::from(p));
                r.pending_row = self.output[self.head][usize::from(p)];
            }
            if p == 0 || p == 1 || p == 14 || p == 15 {
                let plane = usize::from(p == 0 || p == 15);
                for lane in 0..4 {
                    if r.header.mask & (1 << lane) == 0 {
                        continue;
                    }
                    let x = usize::from(r.header.x % 16) + lane % 2;
                    let y = usize::from(r.header.y % 16) + lane / 2;
                    let result = r.result[lane];
                    let value = if p == 14 && result.color_written {
                        Some(result.pixel.color)
                    } else if p == 15 && result.depth_written {
                        Some(result.pixel.depth)
                    } else {
                        None
                    };
                    if p >= 14 && value.is_none() {
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
            if p == 16 {
                self.ready[self.head] = false;
                self.headers[self.head] = None;
                self.head ^= 1;
                self.stats.quads += 1;
                self.stats.pixels += u64::from(r.header.mask.count_ones());
                t.committed = Some(r.header);
            } else {
                if p != 13 || r.returned == 15 {
                    r.phase += 1;
                }
                self.rop = Some(r);
            }
        } else if old_ready {
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
                    source_depth: [0; 4],
                    issued: 0,
                    returned: 0,
                    old: [pixel; 4],
                    result: [ResultPixel {
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
