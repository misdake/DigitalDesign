//! Independent conserving UNORM9 coefficient pipeline, with runtime operands.
//! Three 9x8 multiply sites have three real product registers each. The static
//! II=2 calendar and 315-bit FF placement are declarations, not fitted RTL.
//! Registers are read before reuse writes. No numerical template is evaluated.

pub const COHORT_CAPACITY: usize = 6;
pub const READY_CAPACITY: usize = 2;
pub const NUMERIC_BITS: usize = 315;
pub const CAPTURE_AGE: u8 = 11;
pub const CONSUMER_AGE: u8 = 12;
const VALID_MASK: u16 = (1 << 11) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub multiply_sites: usize,
    pub multiply_latency: usize,
    pub initiation_interval: usize,
    pub numeric_ff_bits: usize,
    pub phase_valid_bits: usize,
    pub metadata_bits: usize,
    pub ready_bits: usize,
    pub queue_control_bits: usize,
    pub terminal_fault_bits: usize,
    pub hard_product_bits: usize,
    pub soft_bits: usize,
    pub ram_ports: usize,
}
/// The 13 queue bits are an explicit future suballocation of preparation control.
/// The standalone fault bit is additional; integration can use Runtime's owner.
pub const ALLOCATION: Allocation = Allocation {
    multiply_sites: 3,
    multiply_latency: 3,
    initiation_interval: 2,
    numeric_ff_bits: NUMERIC_BITS,
    phase_valid_bits: 8 + 11,
    metadata_bits: 99 * COHORT_CAPACITY,
    ready_bits: 171 * READY_CAPACITY,
    queue_control_bits: 3 + 3 + 3 + 1 + 1 + 2,
    terminal_fault_bits: 1,
    hard_product_bits: 3 * 3 * 17,
    soft_bits: 315 + 19 + 594 + 342 + 13 + 1,
    ram_ports: 0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metadata {
    /// Each plane: x0, x1, y0, y1, all unsigned ten-bit codes.
    pub coordinates: [[u16; 4]; 2],
    pub levels: [u8; 2],
    pub slot: u8,
    /// quad_id << 2 | lane, six bits.
    pub key: u8,
    pub last_fine: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    /// Unsigned UNORM9 codes, sum exactly511.
    pub parents: [u16; 2],
    /// Each plane: fu, fv, unsigned binary fractions with denominator256.
    pub fractions: [[u8; 2]; 2],
    pub nearest: bool,
    pub metadata: Metadata,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    /// Row-major: top-left, top-right, bottom-left, bottom-right.
    pub weights: [[u16; 4]; 2],
    pub metadata: Metadata,
}
#[derive(Clone, Copy, Debug)]
pub struct Tick {
    pub ce: bool,
    pub input: Option<Input>,
    pub output_ready: bool,
    /// Actual pre-edge free count from the existing external work16 owner.
    pub work_available: u8,
}
impl Default for Tick {
    fn default() -> Self {
        Self {
            ce: true,
            input: None,
            output_ready: true,
            work_available: 16,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    Multiply(u8),
    Subtract(u8),
    Select(u8),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Field {
    Nearest,
    FineParent,
    FineURaw,
    FineU,
    FineVRaw,
    FineV,
    FineBottom,
    FineTop,
    FineTopRight,
    FineTopLeft,
    FineBottomRight,
    FineBottomLeft,
    CoarseParent,
    CoarseURaw,
    CoarseU,
    CoarseVRaw,
    CoarseV,
    CoarseBottom,
    CoarseTop,
    CoarseTopRight,
    CoarseTopLeft,
    CoarseBottomRight,
    CoarseBottomLeft,
}
#[derive(Clone, Copy, Debug)]
pub struct FieldLayout {
    pub field: Field,
    pub width: u8,
    /// Logical ready/last-use ages, inclusive. Raw fine fractions are external
    /// combinational inputs at age0; they are not written to a second register.
    pub birth: u8,
    pub last_read: u8,
    pub lows: [usize; 4],
}
const fn field(field: Field, width: u8, birth: u8, last_read: u8, lows: [usize; 4]) -> FieldLayout {
    FieldLayout {
        field,
        width,
        birth,
        last_read,
        lows,
    }
}
use Field::*;
/// Exact four-iteration placement, indexed by acceptance phase /2 (period8).
/// Product low bytes remain only in hard registers; the FF bank retains high9.
pub const LAYOUT: [FieldLayout; 23] = [
    field(Nearest, 1, 0, 1, [314; 4]),
    field(FineParent, 9, 0, 4, [36, 45, 54, 63]),
    field(FineURaw, 8, 0, 0, [234, 242, 250, 258]),
    field(FineU, 8, 1, 5, [234, 242, 250, 258]),
    field(FineVRaw, 8, 0, 0, [298; 4]),
    field(FineV, 8, 1, 1, [9, 18, 27, 0]),
    field(FineBottom, 9, 4, 7, [72, 81, 72, 81]),
    field(FineTop, 9, 5, 8, [90, 99, 90, 99]),
    field(FineTopRight, 9, 8, 10, [162, 171, 162, 171]),
    field(FineTopLeft, 9, 9, 10, [54, 63, 36, 45]),
    field(FineBottomRight, 9, 7, 10, [108, 117, 108, 117]),
    field(FineBottomLeft, 9, 8, 10, [180, 189, 180, 189]),
    field(CoarseParent, 9, 0, 5, [0, 9, 18, 27]),
    field(CoarseURaw, 8, 0, 1, [242, 250, 258, 234]),
    field(CoarseU, 8, 2, 6, [266, 274, 282, 290]),
    field(CoarseVRaw, 8, 0, 1, [266, 274, 282, 290]),
    field(CoarseV, 8, 2, 2, [306; 4]),
    field(CoarseBottom, 9, 5, 8, [126, 135, 126, 135]),
    field(CoarseTop, 9, 6, 9, [144, 153, 144, 153]),
    field(CoarseTopRight, 9, 9, 10, [216; 4]),
    field(CoarseTopLeft, 9, 10, 10, [18, 27, 0, 9]),
    field(CoarseBottomRight, 9, 8, 10, [198, 207, 198, 207]),
    field(CoarseBottomLeft, 9, 9, 10, [225; 4]),
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessKind {
    Read,
    Write,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
    pub kind: AccessKind,
    pub field: Field,
    pub key: u8,
    pub age: u8,
    pub low: usize,
    pub width: u8,
    pub value: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Accepted {
        key: u8,
        work_reserved: u8,
    },
    Issued {
        site: Site,
        key: u8,
        age: u8,
        operands: [u16; 2],
    },
    Returned {
        site: Site,
        key: u8,
        age: u8,
        value: u32,
    },
    Access(Access),
    /// Samples last-use age10 into the existing output row. Its registered
    /// valid is published at age11; it cannot be consumed on either edge.
    OutputRegister {
        key: u8,
    },
    Captured(Output),
    Consumed(Output),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub wall: u64,
    /// Effective local arithmetic advances, excluding CE/ready-full holds.
    pub enabled: u64,
    pub phase: u8,
    pub valid: u16,
    pub cohorts: u8,
    pub queued: u8,
    /// Derived from the last issue-valid bit; no extra pending row/valid bit.
    pub pending_capture: bool,
    pub cohort_pointers: [u8; 2],
    pub ready_pointers: [u8; 2],
    /// Read-only checker view, not retained event/history storage.
    pub numeric_words: [u64; 5],
    pub product_registers: [[u32; 3]; 3],
}
#[derive(Clone, Debug)]
pub struct Step {
    pub input_ready: bool,
    pub accepted: bool,
    pub output: Option<Output>,
    pub consumed: bool,
    /// A reservation pulse only. No work credit is retained or returned here.
    pub work_reserved: u8,
    pub events: Vec<Event>,
    pub snapshot: Snapshot,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    WatchdogBound,
    Watchdog,
    Terminal,
    Input,
    WorkCredit,
    Storage,
    Calendar,
}

#[derive(Clone, Copy, Default)]
struct Ready {
    weights: u128,
    metadata: u128,
}
impl Ready {
    fn output(self) -> Output {
        Output {
            weights: std::array::from_fn(|p| {
                std::array::from_fn(|t| ((self.weights >> (9 * (p * 4 + t))) & 511) as u16)
            }),
            metadata: unpack_metadata(self.metadata),
        }
    }
}
#[derive(Clone, Copy)]
struct Job {
    key: u8,
    age: u8,
    iteration: usize,
    metadata: u128,
}
#[derive(Clone, Copy)]
struct Write {
    job: Job,
    field: Field,
    value: u16,
}

pub struct CoefficientEmu {
    // Host container widths are not logical allocations: only bits0..314 exist.
    bits: [u64; 5],
    products: [[u32; 3]; 3],
    phase: u8,
    valid: u16,
    metadata: [u128; COHORT_CAPACITY],
    cohort_read: u8,
    cohort_write: u8,
    cohorts: u8,
    ready: [Ready; READY_CAPACITY],
    ready_read: u8,
    ready_write: u8,
    queued: u8,
    faulted: bool,
    // Diagnostic/watchdog clocks never select operands, placement or owners.
    wall: u64,
    enabled: u64,
    max_wall: u64,
}
impl CoefficientEmu {
    pub fn new(max_wall: u64) -> Result<Self, Fault> {
        if max_wall == 0 || max_wall > 2_000_000 {
            return Err(Fault::WatchdogBound);
        }
        Ok(Self {
            bits: [0; 5],
            products: [[0; 3]; 3],
            phase: 1,
            valid: 0,
            metadata: [0; 6],
            cohort_read: 0,
            cohort_write: 0,
            cohorts: 0,
            ready: [Ready::default(); 2],
            ready_read: 0,
            ready_write: 0,
            queued: 0,
            faulted: false,
            wall: 0,
            enabled: 0,
            max_wall,
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            wall: self.wall,
            enabled: self.enabled,
            phase: self.phase,
            valid: self.valid,
            cohorts: self.cohorts,
            queued: self.queued,
            pending_capture: self.valid & (1 << 10) != 0,
            cohort_pointers: [self.cohort_read, self.cohort_write],
            ready_pointers: [self.ready_read, self.ready_write],
            numeric_words: self.bits,
            product_registers: self.products,
        }
    }
    pub fn idle(&self) -> bool {
        self.valid == 0 && self.queued == 0
    }
    pub fn faulted(&self) -> bool {
        self.faulted
    }
    /// Continuous fanout of the existing old FF ready head. No state or port
    /// is added; callers decide the whole-row handshake before this edge.
    pub fn output(&self) -> Option<Output> {
        (self.queued != 0).then(|| self.ready[usize::from(self.ready_read)].output())
    }
    pub fn tick(&mut self, tick: Tick) -> Result<Step, Fault> {
        if self.faulted {
            return Err(Fault::Terminal);
        }
        if self.wall == self.max_wall {
            self.faulted = true;
            return Err(Fault::Watchdog);
        }
        self.wall += 1;
        let result = self.advance(tick);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }
    fn job(&self, age: u8) -> Option<Job> {
        if age == 0 || self.valid & (1 << (age - 1)) == 0 {
            return None;
        }
        let older = (self.valid >> age).count_ones() as usize;
        let metadata = self.metadata[(usize::from(self.cohort_read) + older) % 6];
        let phase = self.phase.trailing_zeros() as u8;
        Some(Job {
            key: ((metadata >> 92) & 63) as u8,
            age,
            iteration: usize::from((phase + 8 - age % 8) % 8 / 2),
            metadata,
        })
    }
    fn read(&self, job: Job, field: Field, events: &mut Vec<Event>) -> u16 {
        let p = LAYOUT[field as usize];
        let low = p.lows[job.iteration];
        let mut value = 0;
        for bit in 0..usize::from(p.width) {
            value |= (((self.bits[(low + bit) / 64] >> ((low + bit) % 64)) & 1) as u16) << bit;
        }
        events.push(Event::Access(Access {
            kind: AccessKind::Read,
            field,
            key: job.key,
            age: job.age,
            low,
            width: p.width,
            value,
        }));
        value
    }
    fn operand(&self, job: Job, field: Field, events: &mut Vec<Event>) -> u16 {
        let p = LAYOUT[field as usize];
        // Returning hard-register bits are wire operands this edge; the FF copy
        // serves later reads. No post-edge FF read or product recomputation.
        let site = match field {
            FineBottom | CoarseBottom => Some(0),
            FineTopRight | FineBottomRight => Some(1),
            CoarseTopRight | CoarseBottomRight => Some(2),
            _ => None,
        };
        if let Some(site) = site.filter(|_| job.age == p.birth) {
            (self.products[site][2] >> 8) as u16
        } else {
            self.read(job, field, events)
        }
    }
    fn writes(&mut self, writes: &[Write], events: &mut Vec<Event>) -> Result<(), Fault> {
        let mut written = [false; NUMERIC_BITS]; // combinational/checker only
        for w in writes {
            let p = LAYOUT[w.field as usize];
            if u32::from(w.value) >= 1 << p.width {
                return Err(Fault::Storage);
            }
            let low = p.lows[w.job.iteration];
            for bit in 0..usize::from(p.width) {
                let address = low + bit;
                if written[address] {
                    return Err(Fault::Storage);
                }
                written[address] = true;
                let mask = 1 << (address % 64);
                self.bits[address / 64] = (self.bits[address / 64] & !mask)
                    | (u64::from((w.value >> bit) & 1) << (address % 64));
            }
            events.push(Event::Access(Access {
                kind: AccessKind::Write,
                field: w.field,
                key: w.job.key,
                age: w.job.age,
                low,
                width: p.width,
                value: w.value,
            }));
        }
        Ok(())
    }
    fn advance(&mut self, tick: Tick) -> Result<Step, Fault> {
        let output = (self.queued != 0).then(|| self.ready[usize::from(self.ready_read)].output());
        // Old ready-full holds the numerical clock even if a consumer returns
        // capacity now. Its independent consumption allows progress next edge.
        let advance = tick.ce && self.queued < 2;
        let cost = tick.input.map_or(0, |v| {
            v.parents.into_iter().filter(|&p| p != 0).count() as u8
        });
        let input_ready =
            advance && self.cohorts < 6 && self.phase & 0x55 != 0 && tick.work_available >= cost;
        let incoming = tick.input.filter(|_| input_ready);
        if let Some(input) = incoming {
            validate(input)?;
            if tick.work_available > 16 {
                return Err(Fault::WorkCredit);
            }
        }
        let accepted = incoming.is_some();
        let consumed = tick.ce && tick.output_ready && output.is_some();
        let mut events = Vec::new();
        if consumed {
            self.ready_read ^= 1;
            self.queued -= 1;
            events.push(Event::Consumed(output.unwrap()));
        }
        if advance {
            let mut writes = Vec::with_capacity(13); // ephemeral combinational destinations
            let mut multiply = [None; 3];
            let mut sites = [false; 8];
            // Registered returns are read before any FF or pipeline reuse.
            for (age, site, field) in [
                (4, 0, FineBottom),
                (5, 0, CoarseBottom),
                (7, 1, FineBottomRight),
                (8, 1, FineTopRight),
                (8, 2, CoarseBottomRight),
                (9, 2, CoarseTopRight),
            ] {
                if let Some(job) = self.job(age) {
                    let value = self.products[site][2];
                    events.push(Event::Returned {
                        site: Site::Multiply(site as u8),
                        key: job.key,
                        age,
                        value,
                    });
                    writes.push(Write {
                        job,
                        field,
                        value: (value >> 8) as u16,
                    });
                }
            }
            for (age, site, field) in [
                (1, Site::Select(0), FineU),
                (1, Site::Select(1), FineV),
                (2, Site::Select(0), CoarseU),
                (2, Site::Select(1), CoarseV),
                (5, Site::Subtract(0), FineTop),
                (6, Site::Subtract(0), CoarseTop),
                (8, Site::Subtract(1), FineBottomLeft),
                (9, Site::Subtract(1), FineTopLeft),
                (9, Site::Subtract(2), CoarseBottomLeft),
                (10, Site::Subtract(2), CoarseTopLeft),
            ] {
                if let Some(job) = self.job(age) {
                    let value = u32::from(self.read(job, field, &mut events));
                    events.push(Event::Returned {
                        site,
                        key: job.key,
                        age,
                        value,
                    });
                }
            }
            // Output register samples age10 before this edge's reuse writes.
            // The existing issue-valid bit publishes this same row at age11.
            if let Some(job) = self.job(10) {
                let mut weights = 0;
                for (index, field) in [
                    FineTopLeft,
                    FineTopRight,
                    FineBottomLeft,
                    FineBottomRight,
                    CoarseTopLeft,
                    CoarseTopRight,
                    CoarseBottomLeft,
                    CoarseBottomRight,
                ]
                .into_iter()
                .enumerate()
                {
                    weights |= u128::from(self.read(job, field, &mut events)) << (9 * index);
                }
                self.ready[usize::from(self.ready_write)] = Ready {
                    weights,
                    metadata: job.metadata,
                };
                events.push(Event::OutputRegister { key: job.key });
            }
            // One-cycle selected fractions are the actual captured values,
            // not a retained whole input or a computed result vector.
            if let Some(input) = incoming {
                let metadata = pack_metadata(input.metadata);
                let job = Job {
                    key: input.metadata.key,
                    age: 0,
                    iteration: self.phase.trailing_zeros() as usize / 2,
                    metadata,
                };
                for (field, value) in [
                    (Nearest, u16::from(input.nearest)),
                    (FineParent, input.parents[0]),
                    (CoarseParent, input.parents[1]),
                    (CoarseURaw, u16::from(input.fractions[1][0])),
                    (CoarseVRaw, u16::from(input.fractions[1][1])),
                ] {
                    writes.push(Write { job, field, value });
                }
                for (site, field, fraction) in [
                    (0, FineU, input.fractions[0][0]),
                    (1, FineV, input.fractions[0][1]),
                ] {
                    let operands = [u16::from(input.nearest), u16::from(fraction)];
                    issue_site(Site::Select(site), job, operands, &mut sites, &mut events)?;
                    writes.push(Write {
                        job,
                        field,
                        value: if input.nearest { 0 } else { operands[1] },
                    });
                }
            }
            if let Some(job) = self.job(1) {
                let nearest = self.read(job, Nearest, &mut events);
                for (site, raw, selected) in [(0, CoarseURaw, CoarseU), (1, CoarseVRaw, CoarseV)] {
                    let fraction = self.read(job, raw, &mut events);
                    issue_site(
                        Site::Select(site),
                        job,
                        [nearest, fraction],
                        &mut sites,
                        &mut events,
                    )?;
                    writes.push(Write {
                        job,
                        field: selected,
                        value: if nearest != 0 { 0 } else { fraction },
                    });
                }
            }
            for (age, site, left, right) in [
                (1, 0, FineParent, FineV),
                (2, 0, CoarseParent, CoarseV),
                (4, 1, FineBottom, FineU),
                (5, 1, FineTop, FineU),
                (5, 2, CoarseBottom, CoarseU),
                (6, 2, CoarseTop, CoarseU),
            ] {
                if let Some(job) = self.job(age) {
                    let operands = [
                        self.operand(job, left, &mut events),
                        self.operand(job, right, &mut events),
                    ];
                    issue_site(Site::Multiply(site), job, operands, &mut sites, &mut events)?;
                    // Only real issue edges multiply. The value passes through
                    // all three hard registers before any consumer sees it.
                    multiply[usize::from(site)] =
                        Some(u32::from(operands[0]) * u32::from(operands[1]));
                }
            }
            for (age, site, left, right, destination) in [
                (4, 0, FineParent, FineBottom, FineTop),
                (5, 0, CoarseParent, CoarseBottom, CoarseTop),
                (7, 1, FineBottom, FineBottomRight, FineBottomLeft),
                (8, 1, FineTop, FineTopRight, FineTopLeft),
                (8, 2, CoarseBottom, CoarseBottomRight, CoarseBottomLeft),
                (9, 2, CoarseTop, CoarseTopRight, CoarseTopLeft),
            ] {
                if let Some(job) = self.job(age) {
                    let operands = [
                        self.operand(job, left, &mut events),
                        self.operand(job, right, &mut events),
                    ];
                    issue_site(Site::Subtract(site), job, operands, &mut sites, &mut events)?;
                    let value = operands[0].checked_sub(operands[1]).ok_or(Fault::Storage)?;
                    writes.push(Write {
                        job,
                        field: destination,
                        value,
                    });
                }
            }
            self.writes(&writes, &mut events)?;
            for (pipe, value) in self.products.iter_mut().zip(multiply) {
                *pipe = [value.unwrap_or(0), pipe[0], pipe[1]];
            }
            if self.valid & (1 << 10) != 0 {
                let value = self.ready[usize::from(self.ready_write)].output();
                events.push(Event::Captured(value));
                self.ready_write ^= 1;
                self.queued += 1;
                self.cohort_read = (self.cohort_read + 1) % 6;
                self.cohorts -= 1;
            }
            if let Some(input) = incoming {
                self.metadata[usize::from(self.cohort_write)] = pack_metadata(input.metadata);
                self.cohort_write = (self.cohort_write + 1) % 6;
                self.cohorts += 1;
                events.push(Event::Accepted {
                    key: input.metadata.key,
                    work_reserved: cost,
                });
            }
            self.valid = ((self.valid << 1) | u16::from(accepted)) & VALID_MASK;
            self.phase = self.phase.rotate_left(1);
            self.enabled += 1;
        }
        if self.cohorts != self.valid.count_ones() as u8
            || self.cohorts > 6
            || self.queued + u8::from(self.valid & (1 << 10) != 0) > 2
            || self.bits[4] >> 59 != 0
            || self.metadata.iter().any(|m| m >> 99 != 0)
            || self.products.iter().flatten().any(|v| v >> 17 != 0)
        {
            return Err(Fault::Storage);
        }
        Ok(Step {
            input_ready,
            accepted,
            output,
            consumed,
            work_reserved: if accepted { cost } else { 0 },
            events,
            snapshot: self.snapshot(),
        })
    }
}
fn issue_site(
    site: Site,
    job: Job,
    operands: [u16; 2],
    sites: &mut [bool; 8],
    events: &mut Vec<Event>,
) -> Result<(), Fault> {
    let index = match site {
        Site::Multiply(i) => i,
        Site::Subtract(i) => i + 3,
        Site::Select(i) => i + 6,
    } as usize;
    if sites[index] {
        return Err(Fault::Calendar);
    }
    sites[index] = true;
    events.push(Event::Issued {
        site,
        key: job.key,
        age: job.age,
        operands,
    });
    Ok(())
}
fn validate(input: Input) -> Result<(), Fault> {
    let m = input.metadata;
    if input.parents.iter().any(|p| *p > 511)
        || u32::from(input.parents[0]) + u32::from(input.parents[1]) != 511
        || m.last_fine != (input.parents[1] == 0)
        || input.nearest && input.parents != [511, 0]
        || m.coordinates.iter().flatten().any(|p| *p > 1023)
        || m.levels.iter().any(|n| *n > 10)
        || m.slot > 15
        || m.key > 63
    {
        return Err(Fault::Input);
    }
    Ok(())
}
fn pack_metadata(m: Metadata) -> u128 {
    let mut word = 0;
    for (i, coordinate) in m.coordinates.into_iter().flatten().enumerate() {
        word |= u128::from(coordinate) << (10 * i);
    }
    word | (u128::from(m.levels[0]) << 80)
        | (u128::from(m.levels[1]) << 84)
        | (u128::from(m.slot) << 88)
        | (u128::from(m.key) << 92)
        | (u128::from(m.last_fine) << 98)
}
fn unpack_metadata(word: u128) -> Metadata {
    Metadata {
        coordinates: std::array::from_fn(|p| {
            std::array::from_fn(|i| ((word >> (10 * (p * 4 + i))) & 1023) as u16)
        }),
        levels: [((word >> 80) & 15) as u8, ((word >> 84) & 15) as u8],
        slot: ((word >> 88) & 15) as u8,
        key: ((word >> 92) & 63) as u8,
        last_fine: word >> 98 != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Input {
        Input {
            parents: [511, 0],
            fractions: [[128; 2]; 2],
            nearest: false,
            metadata: Metadata {
                coordinates: [[0, 1, 0, 1]; 2],
                levels: [5, 4],
                slot: 2,
                key: 17,
                last_fine: true,
            },
        }
    }
    fn drain(r: &mut CoefficientEmu) -> Output {
        let mut output = None;
        for _ in 0..100 {
            let step = r.tick(Tick::default()).unwrap();
            if step.consumed {
                output = step.output;
            }
            if r.idle() {
                break;
            }
        }
        assert!(r.idle());
        output.unwrap()
    }
    #[test]
    fn corrupted_retained_fraction_changes_output_instead_of_replaying_input_golden() {
        let mut r = CoefficientEmu::new(100).unwrap();
        assert!(
            r.tick(Tick {
                input: Some(sample()),
                ..Tick::default()
            })
            .unwrap()
            .accepted
        );
        // The accepted fineU=128 occupies bit241. Change the actual retained
        // FF before any column multiply. No test callback can repair that bit.
        r.bits[241 / 64] ^= 1 << (241 % 64);
        assert_eq!(drain(&mut r).weights, [[256, 0, 255, 0], [0; 4]]);
    }
    #[test]
    fn corrupted_issued_product_travels_through_all_three_registers() {
        let mut r = CoefficientEmu::new(100).unwrap();
        r.tick(Tick {
            input: Some(sample()),
            ..Tick::default()
        })
        .unwrap();
        r.tick(Tick::default()).unwrap(); // actual fine parent*fv issue, age1
        assert_eq!(r.products[0], [65_408, 0, 0]);
        r.products[0][0] = 0;
        assert_eq!(drain(&mut r).weights, [[256, 255, 0, 0], [0; 4]]);
    }
}
