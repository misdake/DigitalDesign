//! Finite quad ingress, independent branch queues and ordered status retirement.
//!
//! This controller performs no lighting, sampling or final arithmetic. Those
//! engines receive live operands and return live results through explicit ports.
//! Contexts are immutable, mux-read register tables; this is not a multi-read
//! BSRAM claim. Generation/serial fields are host ownership witnesses only.

use super::{Basic, PixelKey, Ticket};
use crate::framebuffer::ports::{Context as RopContext, Header, Quad};
use crate::lighting::ports::{CompactPixelInput, LightingContext, LightingOutput};
use crate::texture::ports::Filter;
use std::collections::VecDeque;

const SLOTS: usize = 16;
const WHITE: [u8; 3] = [255; 3];
const UNLIT: LightingOutput = LightingOutput { g: 256, h: 0 };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextId {
    pub slot: u8,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct SampleContext {
    pub slot: u8,
    pub size_log2: u8,
    pub filter: Filter,
    pub bias_q8: i16,
}

#[derive(Clone, Copy, Debug)]
pub struct CommonContext {
    pub lighting: LightingContext,
    pub sample: Option<SampleContext>,
    pub alpha: u8,
    pub rop: RopContext,
}

#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub context: ContextId,
    pub header: Header,
    pub basic: [Basic; 4],
    pub light: [CompactPixelInput; 4],
    /// Four helper lanes, S40F18, including uncovered lanes.
    pub uv_q18: [[i64; 2]; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct LightJob {
    pub ticket: Ticket,
    pub context: ContextId,
    pub mask: u8,
    pub pixels: [CompactPixelInput; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct SampleJob {
    pub ticket: Ticket,
    pub context: ContextId,
    pub mask: u8,
    pub uv_q18: [[i64; 2]; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalJob {
    pub key: PixelKey,
    pub tint: [u8; 3],
    pub texture: [u8; 3],
    pub light: LightingOutput,
    pub specular: [u8; 3],
    pub depth: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct RopJob {
    pub ticket: Ticket,
    pub quad: Quad,
    /// Detached snapshot: no context lookup after status retirement.
    pub context: RopContext,
}

#[derive(Clone, Copy, Debug)]
pub struct RopRow {
    pub ticket: Ticket,
    pub header: Header,
    pub context: RopContext,
    pub row: u8,
    pub data: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    Basic,
    Light,
    Sample,
    Output,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
    pub store: Store,
    pub write: bool,
    pub address: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub ingress: usize,
    pub lighting: usize,
    pub sampling: usize,
    pub contexts: usize,
    pub max_wall: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ingress: 2,
            lighting: 2,
            sampling: 2,
            contexts: 4,
            max_wall: 1_000_000,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Signals {
    pub input_ready: bool,
    pub lighting: Option<LightJob>,
    pub sampling: Option<SampleJob>,
    pub final_input: Option<FinalJob>,
    pub final_output_ready: bool,
    pub rop: Option<RopRow>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub ce: bool,
    pub input: Option<Input>,
    pub lighting_ready: bool,
    pub sampling_ready: bool,
    pub light: Option<super::LightWrite>,
    pub sample: Option<super::SampleWrite>,
    pub final_ready: bool,
    pub final_result: Option<(PixelKey, [u8; 3])>,
    pub rop_ready: bool,
    pub finish: bool,
}

#[derive(Clone, Debug)]
pub struct Step {
    /// All offers are pre-edge and transfers require CE.
    pub signals: Signals,
    pub input_accepted: bool,
    pub allocated: Option<Ticket>,
    pub dispatched: Option<Ticket>,
    pub retired: Option<Ticket>,
    pub accesses: Vec<Access>,
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub wall: u64,
    pub input: u64,
    pub dropped: u64,
    pub lighting_jobs: u64,
    pub sampling_jobs: u64,
    pub light_writes: u64,
    pub sample_writes: u64,
    pub final_pixels: u64,
    pub output_quads: u64,
    pub retired: u64,
    pub peak_status: usize,
    pub dispatch_stalls: u64,
    pub probe_wait: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inventory {
    pub ingress_payload_bits: usize,
    pub lighting_queue_payload_bits: usize,
    pub sampling_queue_payload_bits: usize,
    pub status_bits: usize,
    pub basic_rows: (usize, usize),
    pub light_rows: (usize, usize),
    pub sample_rows: (usize, usize),
    pub output_rows: (usize, usize),
}

#[derive(Clone, Copy)]
struct ContextEntry {
    id: ContextId,
    value: CommonContext,
    references: usize,
}

#[derive(Clone, Copy)]
struct Status {
    ticket: Ticket,
    context: ContextId,
    header: Header,
    unlit: bool,
    untextured: bool,
    basic: u8,
    light: u8,
    sample: u8,
    light_issued: bool,
    sample_issued: bool,
}

impl Status {
    fn ready(self) -> bool {
        self.basic & self.light & self.sample == self.header.mask
    }
}

#[derive(Clone, Copy)]
struct BasicWriter {
    ticket: Ticket,
    pixels: [Basic; 4],
    row: u8,
}

#[derive(Clone, Copy)]
enum FinalPhase {
    ColorRead,
    DepthRead,
    DepthWait,
    Offer,
    Result,
    DepthWrite,
}

#[derive(Clone, Copy)]
struct Final {
    ticket: Ticket,
    slot: usize,
    lane: u8,
    phase: FinalPhase,
    job: Option<FinalJob>,
}

#[derive(Clone, Copy)]
enum Return {
    Color(FinalJob),
    Depth(u16),
}

#[derive(Clone, Copy)]
struct Output {
    ticket: Ticket,
    header: Header,
    context: RopContext,
    alpha: u8,
}

/// Fixed 16 status slots, 128x32 basic, 64x18 light, 64x24 sample and
/// 16x32 output rows (two quads). Each store has at most one R and one W per
/// edge. Only one synchronous return and one final arithmetic job are reserved.
/// Queues are explicitly capacity-bounded; VecDeque is host storage only.
pub struct Dispatcher {
    config: Config,
    contexts: [Option<ContextEntry>; SLOTS],
    generation: u64,
    ingress: VecDeque<Input>,
    light_queue: VecDeque<LightJob>,
    sample_queue: VecDeque<SampleJob>,
    status: [Option<Status>; SLOTS],
    head: u64,
    tail: u64,
    basic: [u32; 128],
    light: [u32; 64],
    sample: [u32; 64],
    basic_writer: Option<BasicWriter>,
    final_stage: Option<Final>,
    read_return: Option<Return>,
    output_rows: [[u32; 8]; 2],
    output: [Option<Output>; 2],
    output_head: u64,
    output_tail: u64,
    output_cursor: u8,
    rop_return: Option<RopRow>,
    rop_hold: Option<RopRow>,
    closing: bool,
    faulted: bool,
    pub stats: Stats,
}

impl Dispatcher {
    pub fn new(config: Config) -> Result<Self, String> {
        if [
            config.ingress,
            config.lighting,
            config.sampling,
            config.contexts,
        ]
        .iter()
        .any(|&v| v == 0 || v > SLOTS)
            || config.max_wall == 0
            || config.max_wall > 2_000_000
        {
            return Err("dispatcher capacity/wall bound".into());
        }
        Ok(Self {
            config,
            contexts: [None; SLOTS],
            generation: 0,
            ingress: VecDeque::new(),
            light_queue: VecDeque::new(),
            sample_queue: VecDeque::new(),
            status: [None; SLOTS],
            head: 0,
            tail: 0,
            basic: [0; 128],
            light: [0; 64],
            sample: [0; 64],
            basic_writer: None,
            final_stage: None,
            read_return: None,
            output_rows: [[0; 8]; 2],
            output: [None; 2],
            output_head: 0,
            output_tail: 0,
            output_cursor: 0,
            rop_return: None,
            rop_hold: None,
            closing: false,
            faulted: false,
            stats: Stats::default(),
        })
    }

    pub fn set_context(&mut self, slot: u8, value: CommonContext) -> Result<ContextId, String> {
        if self.faulted || usize::from(slot) >= self.config.contexts {
            return Err("context slot/fault".into());
        }
        if self.contexts[usize::from(slot)].is_some_and(|c| c.references != 0) {
            return Err("context still referenced by ingress/status".into());
        }
        if !value.lighting.material.unlit {
            value
                .lighting
                .validate()
                .map_err(|e| format!("context lighting: {e:?}"))?;
        }
        if value.sample.is_some_and(|s| {
            s.slot > 15 || s.size_log2 > 10 || !(-8192..=8192).contains(&s.bias_q8)
        }) {
            return Err("context sampling fields".into());
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("context generation overflow")?;
        let id = ContextId {
            slot,
            generation: self.generation,
        };
        self.contexts[usize::from(slot)] = Some(ContextEntry {
            id,
            value,
            references: 0,
        });
        Ok(id)
    }

    pub fn context(&self, id: ContextId) -> Result<CommonContext, String> {
        self.entry(id).map(|c| c.value)
    }

    fn entry(&self, id: ContextId) -> Result<ContextEntry, String> {
        self.contexts
            .get(usize::from(id.slot))
            .copied()
            .flatten()
            .filter(|c| c.id == id)
            .ok_or_else(|| "stale/missing context".into())
    }

    pub fn context_references(&self, id: ContextId) -> Result<usize, String> {
        self.entry(id).map(|c| c.references)
    }

    /// Payload and explicit array organizations only. Valid bits, FIFO pointers,
    /// muxes, context registers, return skids and leaf engines are additional;
    /// this is neither fitted Logic nor a BSRAM/SSRAM allocation certificate.
    pub fn inventory(&self) -> Inventory {
        Inventory {
            ingress_payload_bits: self.config.ingress * (9 + 8 + 4 + 4 + 4 * 40 + 4 * 72 + 8 * 40),
            lighting_queue_payload_bits: self.config.lighting * (4 + 4 + 4 + 4 * 72),
            sampling_queue_payload_bits: self.config.sampling * (4 + 4 + 4 + 8 * 40),
            // XY/mask21, context4, bypass2, done masks12, issued2, valid1.
            status_bits: SLOTS * (21 + 4 + 2 + 12 + 2 + 1),
            basic_rows: (128, 32),
            light_rows: (64, 18),
            sample_rows: (64, 24),
            output_rows: (16, 32),
        }
    }

    pub fn live_status(&self) -> usize {
        (self.tail - self.head) as usize
    }

    pub fn ticket(&self, quad: u8) -> Option<Ticket> {
        self.status
            .get(usize::from(quad))
            .copied()
            .flatten()
            .map(|s| s.ticket)
    }

    pub fn context_for(&self, ticket: Ticket) -> Result<ContextId, String> {
        self.status
            .get(usize::from(ticket.quad))
            .copied()
            .flatten()
            .filter(|s| s.ticket == ticket)
            .map(|s| s.context)
            .ok_or_else(|| "context requested for stale ticket".into())
    }

    pub fn idle(&self) -> bool {
        self.ingress.is_empty()
            && self.live_status() == 0
            && self.basic_writer.is_none()
            && self.light_queue.is_empty()
            && self.sample_queue.is_empty()
            && self.final_stage.is_none()
            && self.read_return.is_none()
            && self.rop_hold.is_none()
            && self.rop_return.is_none()
            && self.output_head == self.output_tail
    }

    pub fn faulted(&self) -> bool {
        self.faulted
    }

    pub fn complete(&self) -> bool {
        self.closing && !self.faulted && self.idle()
    }

    pub fn signals(&self) -> Signals {
        let final_input = self
            .final_stage
            .filter(|f| matches!(f.phase, FinalPhase::Offer))
            .and_then(|f| f.job);
        let rop = self.rop_hold;
        Signals {
            input_ready: !self.faulted && !self.closing && self.ingress.len() < self.config.ingress,
            lighting: self.light_queue.front().copied(),
            sampling: self.sample_queue.front().copied(),
            final_input,
            final_output_ready: self
                .final_stage
                .is_some_and(|f| matches!(f.phase, FinalPhase::Result)),
            rop,
        }
    }

    fn status_for(&self, key: PixelKey) -> Result<Status, String> {
        if key.lane > 3 {
            return Err("return lane out of range".into());
        }
        self.status
            .get(usize::from(key.ticket.quad))
            .copied()
            .flatten()
            .filter(|s| s.ticket == key.ticket && s.header.mask & (1 << key.lane) != 0)
            .ok_or_else(|| "stale/uncovered return".into())
    }

    fn validate_tick(&self, tick: &Tick, signals: &Signals) -> Result<(), String> {
        if !tick.ce {
            return Ok(());
        }
        if signals.input_ready {
            if let Some(q) = tick.input {
                let ctx = self.context(q.context)?;
                if q.header.mask > 15
                    || q.header.x > 510
                    || q.header.x & 1 != 0
                    || q.header.y & 1 != 0
                    || q.header.y > 254
                {
                    return Err("quad header/alignment".into());
                }
                if q.header.mask != 0 {
                    if !ctx.lighting.material.unlit {
                        for p in q.light {
                            p.validate()
                                .map_err(|e| format!("compact lighting input: {e:?}"))?;
                        }
                    }
                    if ctx.sample.is_some()
                        && q.uv_q18
                            .iter()
                            .flatten()
                            .any(|&v| !(-(1_i64 << 38)..=(1_i64 << 38)).contains(&v))
                    {
                        return Err("sampling S40F18 input".into());
                    }
                }
            }
        }
        if let Some(w) = tick.light {
            let s = self.status_for(w.key)?;
            if s.unlit
                || !s.light_issued
                || s.light & (1 << w.key.lane) != 0
                || w.value.g > 511
                || w.value.h > 256
            {
                return Err("light unsolicited/duplicate/range".into());
            }
        }
        if let Some(w) = tick.sample {
            let s = self.status_for(w.key)?;
            if s.untextured || !s.sample_issued || s.sample & (1 << w.key.lane) != 0 {
                return Err("sample unsolicited/duplicate".into());
            }
        }
        if let Some((key, _)) = tick.final_result {
            let f = self.final_stage.ok_or("unsolicited final result")?;
            if !signals.final_output_ready || f.ticket != key.ticket || f.lane != key.lane {
                return Err("final owner/phase".into());
            }
        }
        Ok(())
    }

    pub fn tick(&mut self, tick: Tick) -> Result<Step, String> {
        if self.faulted {
            return Err("dispatcher terminal fault".into());
        }
        if self.stats.wall == self.config.max_wall {
            self.faulted = true;
            return Err("dispatcher wall watchdog".into());
        }
        let signals = self.signals();
        if let Err(e) = self.validate_tick(&tick, &signals) {
            self.faulted = true;
            return Err(e);
        }
        self.stats.wall += 1;
        let mut step = Step {
            signals: signals.clone(),
            input_accepted: false,
            allocated: None,
            dispatched: None,
            retired: None,
            accesses: vec![],
            complete: false,
        };
        // A previously issued synchronous read returns on wall time. One
        // reserved destination captures it even if compute CE is low.
        if let Some(returned) = self.read_return.take() {
            let f = self
                .final_stage
                .as_mut()
                .ok_or("return without final owner")?;
            match returned {
                Return::Color(job) => f.job = Some(job),
                Return::Depth(depth) => {
                    f.job.as_mut().ok_or("depth before color")?.depth = depth;
                    f.phase = FinalPhase::Offer;
                }
            }
        }
        if let Some(row) = self.rop_return.take() {
            if self.rop_hold.is_some() {
                return Err("reserved ROP return overflow".into());
            }
            self.rop_hold = Some(row);
        }
        if !tick.ce {
            step.complete = self.closing && self.idle();
            return Ok(step);
        }

        // Pre-edge admission decisions. Never borrow capacity freed this edge.
        let dispatch = self.ingress.front().copied().filter(|q| {
            let ctx = self.context(q.context).expect("retained context");
            self.live_status() < SLOTS
                && self.basic_writer.is_none()
                && (ctx.lighting.material.unlit || self.light_queue.len() < self.config.lighting)
                && (ctx.sample.is_none() || self.sample_queue.len() < self.config.sampling)
        });
        let start_final = self.final_stage.is_none()
            && self.output_tail - self.output_head < 2
            && self.status[(self.head as usize) & 15].is_some_and(Status::ready);
        let previous_final = self.final_stage;
        let rop_read = self.output_head < self.output_tail
            && self.rop_hold.is_none()
            && self.rop_return.is_none();

        if tick.rop_ready {
            if let Some(row) = signals.rop {
                self.rop_hold = None;
                if row.row == 7 {
                    self.output[(self.output_head as usize) & 1] = None;
                    self.output_head += 1;
                    self.output_cursor = 0;
                    self.stats.output_quads += 1;
                } else {
                    self.output_cursor += 1;
                }
            }
        }
        if rop_read {
            let slot = (self.output_head as usize) & 1;
            let desc = self.output[slot].ok_or("published output descriptor lost")?;
            self.rop_return = Some(RopRow {
                ticket: desc.ticket,
                header: desc.header,
                context: desc.context,
                row: self.output_cursor,
                data: self.output_rows[slot][usize::from(self.output_cursor)],
            });
            step.accesses.push(Access {
                store: Store::Output,
                write: false,
                address: slot * 8 + usize::from(self.output_cursor),
            });
        }
        if tick.lighting_ready {
            if let Some(job) = signals.lighting {
                self.light_queue.pop_front();
                self.status[usize::from(job.ticket.quad)]
                    .as_mut()
                    .unwrap()
                    .light_issued = true;
                self.stats.lighting_jobs += 1;
            }
        }
        if tick.sampling_ready {
            if let Some(job) = signals.sampling {
                self.sample_queue.pop_front();
                self.status[usize::from(job.ticket.quad)]
                    .as_mut()
                    .unwrap()
                    .sample_issued = true;
                self.stats.sampling_jobs += 1;
            }
        }
        if let Some(w) = tick.light {
            let address = usize::from(w.key.ticket.quad) * 4 + usize::from(w.key.lane);
            self.light[address] = u32::from(w.value.g) | (u32::from(w.value.h) << 9);
            self.status[usize::from(w.key.ticket.quad)]
                .as_mut()
                .unwrap()
                .light |= 1 << w.key.lane;
            step.accesses.push(Access {
                store: Store::Light,
                write: true,
                address,
            });
            self.stats.light_writes += 1;
        }
        if let Some(w) = tick.sample {
            let address = usize::from(w.key.ticket.quad) * 4 + usize::from(w.key.lane);
            self.sample[address] = u32::from_le_bytes([w.rgb[0], w.rgb[1], w.rgb[2], 0]);
            self.status[usize::from(w.key.ticket.quad)]
                .as_mut()
                .unwrap()
                .sample |= 1 << w.key.lane;
            step.accesses.push(Access {
                store: Store::Sample,
                write: true,
                address,
            });
            self.stats.sample_writes += 1;
        }
        if let Some(mut writer) = self.basic_writer.take() {
            let lane = usize::from(writer.row / 2);
            let s = self.status[usize::from(writer.ticket.quad)].unwrap();
            if s.header.mask & (1 << lane) != 0 {
                let address = usize::from(writer.ticket.quad) * 8 + usize::from(writer.row);
                self.basic[address] = if writer.row & 1 == 0 {
                    let rgb = writer.pixels[lane].tint;
                    u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 0])
                } else {
                    self.status[usize::from(writer.ticket.quad)]
                        .as_mut()
                        .unwrap()
                        .basic |= 1 << lane;
                    u32::from(writer.pixels[lane].depth)
                };
                step.accesses.push(Access {
                    store: Store::Basic,
                    write: true,
                    address,
                });
            }
            writer.row += 1;
            if writer.row < 8 {
                self.basic_writer = Some(writer);
            }
        }
        if let Some(f) = previous_final {
            self.advance_final(f, &tick, &signals, &mut step)?;
        } else if start_final {
            let s = self.status[(self.head as usize) & 15].unwrap();
            let context = self.context(s.context)?;
            let slot = (self.output_tail as usize) & 1;
            // Reserve one output slot before any store read, publish after row7.
            self.output_rows[slot] = [0; 8];
            self.final_stage = Some(Final {
                ticket: s.ticket,
                slot,
                lane: 0,
                phase: FinalPhase::ColorRead,
                job: None,
            });
            self.output[slot] = Some(Output {
                ticket: s.ticket,
                header: s.header,
                context: context.rop,
                alpha: context.alpha,
            });
        } else if self.live_status() != 0 {
            self.stats.probe_wait += 1;
        }
        if let Some(q) = dispatch {
            self.ingress.pop_front();
            let context = self.context(q.context)?;
            let quad = (self.tail & 15) as u8;
            let ticket = Ticket {
                quad,
                serial: self.tail,
            };
            let unlit = context.lighting.material.unlit;
            let untextured = context.sample.is_none();
            self.status[usize::from(quad)] = Some(Status {
                ticket,
                context: q.context,
                header: q.header,
                unlit,
                untextured,
                basic: 0,
                light: if unlit { q.header.mask } else { 0 },
                sample: if untextured { q.header.mask } else { 0 },
                light_issued: false,
                sample_issued: false,
            });
            self.basic_writer = Some(BasicWriter {
                ticket,
                pixels: q.basic,
                row: 0,
            });
            if !unlit {
                self.light_queue.push_back(LightJob {
                    ticket,
                    context: q.context,
                    mask: q.header.mask,
                    pixels: q.light,
                });
            }
            if !untextured {
                self.sample_queue.push_back(SampleJob {
                    ticket,
                    context: q.context,
                    mask: q.header.mask,
                    uv_q18: q.uv_q18,
                });
            }
            self.tail += 1;
            self.stats.peak_status = self.stats.peak_status.max(self.live_status());
            step.allocated = Some(ticket);
            step.dispatched = Some(ticket);
        } else if !self.ingress.is_empty() {
            self.stats.dispatch_stalls += 1;
        }
        if tick.finish {
            self.closing = true;
        }
        if signals.input_ready {
            if let Some(q) = tick.input {
                step.input_accepted = true;
                self.stats.input += 1;
                if q.header.mask == 0 {
                    self.stats.dropped += 1;
                } else {
                    self.contexts[usize::from(q.context.slot)]
                        .as_mut()
                        .unwrap()
                        .references += 1;
                    self.ingress.push_back(q);
                }
            }
        }
        step.complete = self.closing && self.idle();
        Ok(step)
    }

    fn advance_final(
        &mut self,
        mut f: Final,
        tick: &Tick,
        signals: &Signals,
        step: &mut Step,
    ) -> Result<(), String> {
        let status = self.status[usize::from(f.ticket.quad)].ok_or("final status lost")?;
        let key = PixelKey {
            ticket: f.ticket,
            lane: f.lane,
        };
        let covered = status.header.mask & (1 << f.lane) != 0;
        let basic_address = usize::from(f.ticket.quad) * 8 + usize::from(f.lane) * 2;
        let branch_address = usize::from(f.ticket.quad) * 4 + usize::from(f.lane);
        match f.phase {
            FinalPhase::ColorRead if !covered => {
                let address = f.slot * 8 + usize::from(f.lane) * 2;
                self.output_rows[f.slot][usize::from(f.lane) * 2] = 0;
                step.accesses.push(Access {
                    store: Store::Output,
                    write: true,
                    address,
                });
                f.job = None;
                f.phase = FinalPhase::DepthWrite;
            }
            FinalPhase::ColorRead => {
                let tint = self.basic[basic_address].to_le_bytes();
                let light = if status.unlit {
                    UNLIT
                } else {
                    step.accesses.push(Access {
                        store: Store::Light,
                        write: false,
                        address: branch_address,
                    });
                    let packed = self.light[branch_address];
                    LightingOutput {
                        g: (packed & 511) as u16,
                        h: (packed >> 9) as u16,
                    }
                };
                let texture = if status.untextured {
                    WHITE
                } else {
                    step.accesses.push(Access {
                        store: Store::Sample,
                        write: false,
                        address: branch_address,
                    });
                    let v = self.sample[branch_address].to_le_bytes();
                    [v[0], v[1], v[2]]
                };
                step.accesses.push(Access {
                    store: Store::Basic,
                    write: false,
                    address: basic_address,
                });
                self.read_return = Some(Return::Color(FinalJob {
                    key,
                    tint: [tint[0], tint[1], tint[2]],
                    texture,
                    light,
                    specular: self
                        .context(status.context)?
                        .lighting
                        .material
                        .specular_color,
                    depth: 0,
                }));
                f.phase = FinalPhase::DepthRead;
            }
            FinalPhase::DepthRead => {
                self.read_return = Some(Return::Depth(self.basic[basic_address + 1] as u16));
                step.accesses.push(Access {
                    store: Store::Basic,
                    write: false,
                    address: basic_address + 1,
                });
                f.job = self.final_stage.and_then(|s| s.job);
                f.phase = FinalPhase::DepthWait;
            }
            FinalPhase::DepthWait => {
                // The wall-time capture above owns the transition to Offer.
                f = self.final_stage.unwrap();
            }
            FinalPhase::Offer => {
                if signals.final_input.is_some() && tick.final_ready {
                    f.phase = FinalPhase::Result;
                    self.stats.final_pixels += 1;
                }
            }
            FinalPhase::Result => {
                if let Some((_, rgb)) = tick.final_result {
                    let alpha = self.output[f.slot].unwrap().alpha;
                    self.output_rows[f.slot][usize::from(f.lane) * 2] =
                        u32::from_le_bytes([rgb[0], rgb[1], rgb[2], alpha]);
                    step.accesses.push(Access {
                        store: Store::Output,
                        write: true,
                        address: f.slot * 8 + usize::from(f.lane) * 2,
                    });
                    f.phase = FinalPhase::DepthWrite;
                }
            }
            FinalPhase::DepthWrite => {
                self.output_rows[f.slot][usize::from(f.lane) * 2 + 1] =
                    f.job.map_or(0, |j| u32::from(j.depth));
                step.accesses.push(Access {
                    store: Store::Output,
                    write: true,
                    address: f.slot * 8 + usize::from(f.lane) * 2 + 1,
                });
                if f.lane == 3 {
                    self.output_tail += 1;
                    self.status[usize::from(f.ticket.quad)] = None;
                    self.contexts[usize::from(status.context.slot)]
                        .as_mut()
                        .unwrap()
                        .references -= 1;
                    self.head += 1;
                    self.stats.retired += 1;
                    self.final_stage = None;
                    step.retired = Some(f.ticket);
                    return Ok(());
                }
                f.lane += 1;
                f.phase = FinalPhase::ColorRead;
                f.job = None;
            }
        }
        self.final_stage = Some(f);
        Ok(())
    }
}
