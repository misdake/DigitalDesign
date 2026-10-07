//! Published row ingress, split branch inputs, whole-quad join and pipelined Final.
//!
//! This is a bounded Rust controller with explicit asynchronous read/capture
//! edges. SPSC and Final have independent RTL; the combined controller does not.
//! Normal/UV exist only in their branch queues and read heads. The producer owns
//! one incomplete tail described by metadata, never a second complete quad.

use super::{
    dispatch::{CommonContext, ContextId, Input, RopRow},
    final_stage::{self, emu::FinalEmu},
    spsc::{self, emu::Queue, ReadTiming},
    LightWrite, PixelKey, SampleWrite, Ticket,
};
use crate::{framebuffer::ports::Header, lighting::ports::CompactPixelInput};
use std::collections::VecDeque;

pub const GLOBAL_SLOTS: usize = 32;
const SLOTS: usize = GLOBAL_SLOTS;

#[derive(Clone, Copy, Debug)]
pub struct Begin {
    pub context: ContextId,
    pub header: Header,
    pub force_coarsest: bool,
}

/// One source edge: RGB/D alternate, Lighting owns its 2x36 rows per lane,
/// Sampling owns a single UV36 row on each odd edge. Only row zero has Begin.
#[derive(Clone, Copy, Debug)]
pub struct Beat {
    pub begin: Option<Begin>,
    pub basic: u32,
    pub lighting: u64,
    pub sampling: Option<u64>,
}

/// Producer-side packing for pregenerated raster attributes, not DUT storage or
/// precomputed branch answers. The source holds a beat until it is accepted.
pub fn source_beats(input: Input) -> Result<[Beat; 8], String> {
    let mut light = [[0; 2]; 4];
    for (rows, pixel) in light.iter_mut().zip(input.light) {
        *rows = pixel
            .rows()
            .map_err(|e| format!("source normal/NDC: {e:?}"))?;
    }
    if input
        .uv_q16
        .iter()
        .flatten()
        .any(|&x| !(-131072..=131071).contains(&x))
    {
        return Err("source UV outside S(18,16)".into());
    }
    Ok(std::array::from_fn(|row| {
        let lane = row / 2;
        Beat {
            begin: (row == 0).then_some(Begin {
                context: input.context,
                header: input.header,
                force_coarsest: input.force_coarsest,
            }),
            basic: if row & 1 == 0 {
                let c = input.basic[lane].tint;
                u32::from_le_bytes([c[0], c[1], c[2], 0])
            } else {
                u32::from(input.basic[lane].depth)
            },
            lighting: light[lane][row & 1],
            sampling: (row & 1 != 0).then_some(
                (input.uv_q16[lane][0] as u64 & 0x3ffff)
                    | ((input.uv_q16[lane][1] as u64 & 0x3ffff) << 18),
            ),
        }
    }))
}

#[derive(Clone, Copy, Debug)]
pub struct LightPixel {
    pub key: PixelKey,
    pub context: ContextId,
    pub pixel: CompactPixelInput,
}

#[derive(Clone, Copy, Debug)]
pub struct SampleQuad {
    pub ticket: Ticket,
    pub context: ContextId,
    pub mask: u8,
    pub uv_q16: [[i64; 2]; 4],
    pub force_coarsest: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub ce: bool,
    pub input: Option<Beat>,
    pub lighting_ready: bool,
    pub sampling_ready: bool,
    pub light: Option<LightWrite>,
    pub sample: Option<SampleWrite>,
    pub final_issue_ready: bool,
    pub final_result_ready: bool,
    pub rop_ready: bool,
}

#[derive(Clone, Debug)]
pub struct Signals {
    pub input_ready: bool,
    pub lighting: Option<LightPixel>,
    pub sampling: Option<SampleQuad>,
    pub rop: Option<RopRow>,
}

/// Diagnostic occupancy of existing storage; not additional hardware state.
#[derive(Clone, Copy, Debug)]
pub struct CapacitySnapshot {
    pub global: usize,
    pub lighting_input: usize,
    pub sampling_input: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    Basic,
    Light,
    Sample,
    Output,
    LightDone,
    SampleDone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
    pub store: Store,
    pub write: bool,
    pub address: usize,
}

pub struct Step {
    pub signals: Signals,
    pub input_accepted: bool,
    pub published: Option<Ticket>,
    pub lighting_accepted: bool,
    pub sampling_accepted: bool,
    pub final_stage: final_stage::Step,
    pub final_accepted: Option<PixelKey>,
    pub accesses: Vec<Access>,
    pub retired: Option<Ticket>,
    pub released_draws: Vec<ContextId>,
}

#[derive(Clone, Copy)]
struct Draw {
    id: ContextId,
    value: CommonContext,
    closed: bool,
    references: usize,
}

#[derive(Clone, Copy)]
struct Status {
    ticket: Ticket,
    context: ContextId,
    header: Header,
    unlit: bool,
    untextured: bool,
    force_coarsest: bool,
}

#[derive(Clone, Copy)]
struct Producer {
    begin: Begin,
    ticket: Option<Ticket>,
    row: usize,
    unlit: bool,
    untextured: bool,
}

#[derive(Clone, Copy)]
struct Owner {
    key: PixelKey,
    depth: u16,
}

pub struct Pipeline {
    banks: [Option<Draw>; 2],
    generation: u64,
    boundaries: VecDeque<(u64, ContextId)>,
    status: [Option<Status>; SLOTS],
    // Published tail: advances only with the last basic/branch input row.
    // The sole private Producer owns an additional, unpublished slot.
    insert: u64,
    consume: u64,
    producer: Option<Producer>,
    lighting: Queue,
    sampling: Queue,
    output: Queue,
    light_meta: VecDeque<Ticket>,
    sample_meta: VecDeque<Ticket>,
    light_normal: Option<u64>,
    sample_head: [u64; 3],
    sample_rows: usize,
    basic: [u32; SLOTS * 8],
    light: [u32; SLOTS * 4],
    sample: [u32; SLOTS * 4],
    // Each branch alone writes its completion array. Allocator does not clear it.
    light_done: [bool; SLOTS],
    sample_done: [bool; SLOTS],
    // Allocator-owned FF state. Bypass does not toggle the branch expectation.
    // Done remains branch-owned RAM; allocator never reads or clears it.
    light_expected: [bool; SLOTS],
    sample_expected: [bool; SLOTS],
    // Host-only duplicate/order witnesses, not per-lane hardware ready flags.
    light_seen: [u8; SLOTS],
    sample_seen: [u8; SLOTS],
    light_sent: [u8; SLOTS],
    sample_sent: [bool; SLOTS],
    initializing: usize,
    final_stage: FinalEmu,
    final_owners: VecDeque<Owner>,
    join: u64,
    join_lane: u8,
    color: Option<final_stage::Input>,
    write_quad: u64,
    write_row: usize,
    depth: Option<u16>,
    wall: u64,
    max_wall: u64,
    faulted: bool,
}

impl Pipeline {
    pub fn new(max_wall: u64) -> Result<Self, String> {
        let queue = |entries, rows, width| {
            Queue::new(spsc::Config {
                entries,
                rows,
                width,
                read_timing: ReadTiming::Capture,
                max_wall,
            })
        };
        Ok(Self {
            banks: [None; 2],
            generation: 0,
            boundaries: VecDeque::new(),
            status: [None; SLOTS],
            insert: 0,
            consume: 0,
            producer: None,
            lighting: queue(2, 8, 36)?,
            sampling: Queue::new(spsc::Config {
                entries: 8,
                rows: 4,
                width: 36,
                read_timing: ReadTiming::Registered,
                max_wall,
            })?,
            output: queue(SLOTS, 8, 32)?,
            light_meta: VecDeque::new(),
            sample_meta: VecDeque::new(),
            light_normal: None,
            sample_head: [0; 3],
            sample_rows: 0,
            basic: [0; SLOTS * 8],
            light: [0; SLOTS * 4],
            sample: [0; SLOTS * 4],
            light_done: [false; SLOTS],
            sample_done: [false; SLOTS],
            light_expected: [true; SLOTS],
            sample_expected: [true; SLOTS],
            light_seen: [0; SLOTS],
            sample_seen: [0; SLOTS],
            light_sent: [0; SLOTS],
            sample_sent: [false; SLOTS],
            initializing: 0,
            final_stage: FinalEmu::new(max_wall)?,
            final_owners: VecDeque::new(),
            join: 0,
            join_lane: 0,
            color: None,
            write_quad: 0,
            write_row: 0,
            depth: None,
            wall: 0,
            max_wall,
            faulted: false,
        })
    }

    /// Atomic immutable publication of one completely supplied context bank.
    /// No free bank returns None; the third draw must retry after a boundary retires.
    pub fn open_draw(&mut self, value: CommonContext) -> Result<Option<ContextId>, String> {
        if self.faulted {
            return Err("pixel foundation terminal fault".into());
        }
        if !value.lighting.material.unlit {
            value
                .lighting
                .validate()
                .map_err(|e| format!("draw lighting: {e:?}"))?;
        }
        if value.sample.is_some_and(|s| {
            s.slot > 15 || s.size_log2 > 10 || !(-8192..=8192).contains(&s.bias_q8)
        }) {
            return Err("draw sampling configuration".into());
        }
        let Some(slot) = self.banks.iter().position(Option::is_none) else {
            return Ok(None);
        };
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("draw witness overflow")?;
        let id = ContextId {
            slot: slot as u8,
            generation: self.generation,
        };
        self.banks[slot] = Some(Draw {
            id,
            value,
            closed: false,
            references: 0,
        });
        Ok(Some(id))
    }

    pub fn context(&self, id: ContextId) -> Result<CommonContext, String> {
        Ok(self.draw(id)?.value)
    }

    fn draw(&self, id: ContextId) -> Result<Draw, String> {
        self.banks
            .get(usize::from(id.slot))
            .copied()
            .flatten()
            .filter(|d| d.id == id)
            .ok_or_else(|| "stale/unpublished draw bank".into())
    }

    pub fn close_draw(&mut self, id: ContextId) -> Result<(), String> {
        let draw = self.draw(id)?;
        if self.faulted || draw.closed || self.producer.is_some_and(|p| p.begin.context == id) {
            return Err("draw close during partial quad or after close/fault".into());
        }
        self.banks[usize::from(id.slot)].as_mut().unwrap().closed = true;
        self.boundaries.push_back((self.insert, id));
        Ok(())
    }

    pub fn live_status(&self) -> usize {
        (self.insert - self.consume) as usize
            + usize::from(self.producer.is_some_and(|p| p.ticket.is_some()))
    }
    pub fn capacity_snapshot(&self) -> CapacitySnapshot {
        CapacitySnapshot {
            global: self.live_status(),
            lighting_input: self.lighting.occupied_entries(),
            sampling_input: self.sampling.occupied_entries(),
        }
    }
    pub fn faulted(&self) -> bool {
        self.faulted
    }
    pub fn ticket(&self, quad: u8) -> Option<Ticket> {
        self.status
            .get(usize::from(quad))
            .copied()
            .flatten()
            .map(|s| s.ticket)
    }
    pub fn context_for(&self, ticket: Ticket) -> Result<ContextId, String> {
        Ok(self.owned(ticket)?.context)
    }
    fn owned(&self, ticket: Ticket) -> Result<Status, String> {
        self.status
            .get(usize::from(ticket.quad))
            .copied()
            .flatten()
            .filter(|s| s.ticket == ticket)
            .ok_or_else(|| "stale/unowned quad result".into())
    }
    fn ready(&self, s: Status) -> bool {
        (s.unlit
            || self.light_done[usize::from(s.ticket.quad)]
                == self.light_expected[usize::from(s.ticket.quad)])
            && (s.untextured
                || self.sample_done[usize::from(s.ticket.quad)]
                    == self.sample_expected[usize::from(s.ticket.quad)])
    }
    pub fn idle(&self) -> bool {
        !self.faulted
            && self.live_status() == 0
            && self.producer.is_none()
            && self.lighting.idle()
            && self.sampling.idle()
            && self.output.idle()
            && self.final_stage.idle()
            && self.final_owners.is_empty()
            && self.banks.iter().all(Option::is_none)
    }

    fn input_ready(&self, begin: Option<Begin>, ce: bool) -> bool {
        if !ce || self.faulted || self.initializing < SLOTS {
            return false;
        }
        if self.producer.is_some() {
            return true;
        }
        let Some(begin) = begin else {
            return false;
        };
        let Ok(draw) = self.draw(begin.context) else {
            return false;
        };
        if draw.closed {
            return false;
        }
        begin.header.mask == 0
            || (self.live_status() < SLOTS
                && (draw.value.lighting.material.unlit || self.lighting.signals(true).input_ready)
                && (draw.value.sample.is_none() || self.sampling.signals(true).input_ready))
    }

    pub fn signals(&self, begin: Option<Begin>, ce: bool) -> Result<Signals, String> {
        let mut lighting = None;
        if let (Some(normal), Some(word), Some(&ticket)) = (
            self.light_normal,
            self.lighting.signals(ce).output,
            self.light_meta.front(),
        ) {
            let s = self.owned(ticket)?;
            let lane = (word.row / 2) as u8;
            if word.row & 1 != 0 && s.header.mask & (1 << lane) != 0 {
                lighting = Some(LightPixel {
                    key: PixelKey { ticket, lane },
                    context: s.context,
                    pixel: CompactPixelInput::from_rows([normal, word.data])
                        .map_err(|e| format!("Lighting head: {e:?}"))?,
                });
            }
        }
        let sampling = if self.sample_rows == 3 {
            match (self.sampling.signals(ce).output, self.sample_meta.front()) {
                (Some(word), Some(&ticket)) => {
                    let s = self.owned(ticket)?;
                    let rows = [
                        self.sample_head[0],
                        self.sample_head[1],
                        self.sample_head[2],
                        word.data,
                    ];
                    let signed = |x: u64| ((x as i64) << 46) >> 46;
                    Some(SampleQuad {
                        ticket,
                        context: s.context,
                        mask: s.header.mask,
                        uv_q16: rows.map(|r| [signed(r & 0x3ffff), signed(r >> 18)]),
                        force_coarsest: s.force_coarsest,
                    })
                }
                _ => None,
            }
        } else {
            None
        };
        let rop = self
            .output
            .signals(ce)
            .output
            .map(|word| {
                let s = self.status[(self.consume as usize) & (SLOTS - 1)]
                    .ok_or("ROP output lost status")?;
                Ok::<_, String>(RopRow {
                    ticket: s.ticket,
                    header: s.header,
                    context: self.context(s.context)?.rop,
                    row: word.row as u8,
                    data: word.data as u32,
                })
            })
            .transpose()?;
        Ok(Signals {
            input_ready: self.input_ready(begin, ce),
            lighting,
            sampling,
            rop,
        })
    }

    pub fn tick(&mut self, tick: Tick) -> Result<Step, String> {
        if self.faulted {
            return Err("pixel foundation terminal fault; drain accepted MC externally".into());
        }
        if self.wall >= self.max_wall {
            self.faulted = true;
            return Err("pixel foundation wall watchdog".into());
        }
        let result = self.advance(tick);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }

    fn advance(&mut self, tick: Tick) -> Result<Step, String> {
        self.wall += 1;
        let signals = self.signals(tick.input.and_then(|b| b.begin), tick.ce)?;
        let accepted = tick.input.is_some() && signals.input_ready;
        let lighting_accepted = tick.ce && tick.lighting_ready && signals.lighting.is_some();
        let sampling_accepted = tick.ce && tick.sampling_ready && signals.sampling.is_some();
        let mut accesses = Vec::new();
        if tick.ce {
            if let Some(w) = tick.light {
                self.owned(w.key.ticket)?;
                if w.key.lane > 3
                    || self.light_sent[usize::from(w.key.ticket.quad)] & (1 << w.key.lane) == 0
                {
                    return Err("Lighting result precedes actual issue".into());
                }
            }
            if let Some(w) = tick.sample {
                self.owned(w.key.ticket)?;
                if !self.sample_sent[usize::from(w.key.ticket.quad)] {
                    return Err("Sampling result precedes actual issue".into());
                }
            }
        }
        // All join, branch and output decisions observe old completion/publication.
        let join_status = (self.join < self.insert)
            .then(|| self.status[(self.join as usize) & (SLOTS - 1)])
            .flatten();
        let light_done_collision = tick.ce
            && join_status.is_some_and(|s| {
                tick.light.is_some_and(|w| {
                    w.key.ticket == s.ticket
                        && w.key.lane < 4
                        && s.header.mask >> (w.key.lane + 1) == 0
                })
            });
        let sample_done_collision = tick.ce
            && join_status.is_some_and(|s| {
                tick.sample.is_some_and(|w| {
                    w.key.ticket == s.ticket
                        && w.key.lane < 4
                        && s.header.mask >> (w.key.lane + 1) == 0
                })
            });
        let join_ready = !light_done_collision
            && !sample_done_collision
            && join_status.is_some_and(|s| self.ready(s));
        if tick.ce {
            if let Some(s) = join_status {
                if !s.unlit && !light_done_collision {
                    accesses.push(Access {
                        store: Store::LightDone,
                        write: false,
                        address: usize::from(s.ticket.quad),
                    });
                }
                if !s.untextured && !sample_done_collision {
                    accesses.push(Access {
                        store: Store::SampleDone,
                        write: false,
                        address: usize::from(s.ticket.quad),
                    });
                }
            }
        }
        let output_status = (self.write_quad < self.insert)
            .then(|| self.status[(self.write_quad as usize) & (SLOTS - 1)])
            .flatten();
        // Only join probes the done RAMs. Output follows work already admitted
        // by join; it does not invent a second done-table read port.
        let output_ready = output_status.is_some()
            && (self.write_quad < self.join
                || (self.write_quad == self.join && (self.color.is_some() || self.join_lane != 0)));
        let output_space = self.output.signals(tick.ce).input_ready;
        let covered_output =
            output_status.is_some_and(|s| s.header.mask & (1 << (self.write_row / 2)) != 0);
        let take_result = output_ready
            && output_space
            && self.write_row & 1 == 0
            && covered_output
            && tick.final_result_ready;
        let final_input = self.color.filter(|_| tick.final_issue_ready);
        let final_step = self.final_stage.tick(final_stage::Tick {
            reset: false,
            ce: tick.ce,
            input: final_input,
            output_ready: take_result,
        })?;
        let mut output_word = None;
        if tick.ce && output_ready && output_space {
            let s = output_status.unwrap();
            if !covered_output {
                output_word = Some(0);
            } else if self.write_row & 1 != 0 {
                output_word = Some(u32::from(
                    self.depth.take().ok_or("Final depth owner lost")?,
                ));
            } else if final_step.consumed {
                let result = final_step.output.ok_or("Final result missing")?;
                let owner = self
                    .final_owners
                    .pop_front()
                    .ok_or("Final result without destination")?;
                if owner.key.ticket != s.ticket
                    || usize::from(owner.key.lane) != self.write_row / 2
                    || result.key != (s.ticket.quad & 15) * 4 + owner.key.lane
                {
                    return Err("Final output owner/order mismatch".into());
                }
                self.depth = Some(owner.depth);
                output_word = Some(u32::from_le_bytes([
                    result.rgb[0],
                    result.rgb[1],
                    result.rgb[2],
                    self.context(s.context)?.alpha,
                ]));
            }
        }
        let mut final_accepted = None;
        if final_step.accepted {
            let s = join_status.ok_or("Final acceptance lost quad")?;
            let key = PixelKey {
                ticket: s.ticket,
                lane: self.join_lane,
            };
            final_accepted = Some(key);
            let address = usize::from(s.ticket.quad) * 8 + usize::from(self.join_lane) * 2 + 1;
            accesses.push(Access {
                store: Store::Basic,
                write: false,
                address,
            });
            self.final_owners.push_back(Owner {
                key,
                depth: self.basic[address] as u16,
            });
            if self.final_owners.len() > final_stage::RESULT_CAPACITY {
                return Err("Final destination overflow".into());
            }
            self.color = None;
            self.join_lane += 1;
            if self.join_lane == 4 {
                self.join_lane = 0;
                self.join += 1;
            }
        } else if tick.ce && join_ready && self.color.is_none() {
            let s = join_status.unwrap();
            let lane = self.join_lane;
            if s.header.mask & (1 << lane) == 0 {
                self.join_lane += 1;
                if self.join_lane == 4 {
                    self.join_lane = 0;
                    self.join += 1;
                }
            } else {
                let pixel = usize::from(s.ticket.quad) * 4 + usize::from(lane);
                let address = pixel * 2;
                let tint = self.basic[address].to_le_bytes();
                accesses.push(Access {
                    store: Store::Basic,
                    write: false,
                    address,
                });
                let light = if s.unlit {
                    256
                } else {
                    accesses.push(Access {
                        store: Store::Light,
                        write: false,
                        address: pixel,
                    });
                    self.light[pixel]
                };
                let texture = if s.untextured {
                    [255; 4]
                } else {
                    accesses.push(Access {
                        store: Store::Sample,
                        write: false,
                        address: pixel,
                    });
                    self.sample[pixel].to_le_bytes()
                };
                self.color = Some(final_stage::Input {
                    // Ordered owner FIFO retains the full global destination;
                    // the unchanged leaf carries only the low six bits.
                    key: (s.ticket.quad & 15) * 4 + lane,
                    tint: [tint[0], tint[1], tint[2]],
                    texture: [texture[0], texture[1], texture[2]],
                    g: (light & 511) as u16,
                    h: (light >> 9) as u16,
                    specular: self.context(s.context)?.lighting.material.specular_color,
                });
            }
        }

        let light_head = self.lighting.signals(tick.ce).output;
        let light_pop = light_head
            .is_some_and(|w| w.row & 1 == 0 || signals.lighting.is_none() || tick.lighting_ready);
        let sample_head = self.sampling.signals(tick.ce).output;
        let sample_pop = sample_head.is_some() && (self.sample_rows < 3 || tick.sampling_ready);
        let mut light_word = None;
        let mut sample_word = None;
        let mut published = None;
        if accepted {
            let beat = tick.input.unwrap();
            let mut p = if let Some(p) = self.producer {
                p
            } else {
                let begin = beat.begin.ok_or("first row lacks descriptor")?;
                let draw = self.draw(begin.context)?;
                if begin.header.mask > 15
                    || begin.header.x > 510
                    || begin.header.x & 1 != 0
                    || begin.header.y > 254
                    || begin.header.y & 1 != 0
                {
                    return Err("source quad header/alignment".into());
                }
                let ticket = (begin.header.mask != 0).then_some(Ticket {
                    quad: (self.insert as usize & (SLOTS - 1)) as u8,
                    serial: self.insert,
                });
                if let Some(ticket) = ticket {
                    let slot = usize::from(ticket.quad);
                    if self.status[slot].is_some() {
                        return Err("allocator overwrote live slot".into());
                    }
                    if !draw.value.lighting.material.unlit {
                        self.light_expected[slot] = !self.light_expected[slot];
                    }
                    if draw.value.sample.is_some() {
                        self.sample_expected[slot] = !self.sample_expected[slot];
                    }
                    self.status[slot] = Some(Status {
                        ticket,
                        context: begin.context,
                        header: begin.header,
                        unlit: draw.value.lighting.material.unlit,
                        untextured: draw.value.sample.is_none(),
                        force_coarsest: begin.force_coarsest,
                    });
                    self.light_seen[slot] = 0;
                    self.sample_seen[slot] = 0;
                    self.light_sent[slot] = 0;
                    self.sample_sent[slot] = false;
                    self.banks[usize::from(begin.context.slot)]
                        .as_mut()
                        .unwrap()
                        .references += 1;
                }
                Producer {
                    begin,
                    ticket,
                    row: 0,
                    unlit: draw.value.lighting.material.unlit,
                    untextured: draw.value.sample.is_none(),
                }
            };
            if p.row != 0 && beat.begin.is_some() {
                return Err("descriptor replaced incomplete quad".into());
            }
            if let Some(ticket) = p.ticket {
                let address = usize::from(ticket.quad) * 8 + p.row;
                self.basic[address] = beat.basic;
                accesses.push(Access {
                    store: Store::Basic,
                    write: true,
                    address,
                });
                if !p.unlit {
                    light_word = Some(beat.lighting);
                }
                if !p.untextured && p.row & 1 != 0 {
                    sample_word = Some(beat.sampling.ok_or("missing source UV row")?);
                }
                if p.row == 7 {
                    self.insert += 1;
                    if !p.unlit {
                        self.light_meta.push_back(ticket);
                    }
                    if !p.untextured {
                        self.sample_meta.push_back(ticket);
                    }
                    published = Some(ticket);
                }
            }
            p.row += 1;
            self.producer = (p.row != 8).then_some(p);
        }
        let light_step = self.lighting.tick(spsc::Tick {
            reset: false,
            ce: tick.ce,
            input: light_word,
            output_ready: light_pop,
        })?;
        let sample_step = self.sampling.tick(spsc::Tick {
            reset: false,
            ce: tick.ce,
            input: sample_word,
            output_ready: sample_pop,
        })?;
        let output_step = self.output.tick(spsc::Tick {
            reset: false,
            ce: tick.ce,
            input: output_word.map(u64::from),
            output_ready: tick.rop_ready,
        })?;
        if light_step.accepted != light_word.is_some()
            || sample_step.accepted != sample_word.is_some()
            || output_step.accepted != output_word.is_some()
        {
            return Err("published store transfer disagreement".into());
        }
        if light_step.consumed {
            let word = light_step.signals.output.unwrap();
            if word.row & 1 == 0 {
                self.light_normal = Some(word.data);
            } else {
                self.light_normal = None;
            }
            if word.last {
                self.light_meta
                    .pop_front()
                    .ok_or("Lighting descriptor underflow")?;
            }
        }
        if sample_step.consumed {
            let word = sample_step.signals.output.unwrap();
            if word.last {
                self.sample_rows = 0;
                self.sample_meta
                    .pop_front()
                    .ok_or("Sampling descriptor underflow")?;
            } else {
                self.sample_head[self.sample_rows] = word.data;
                self.sample_rows += 1;
            }
        }
        if lighting_accepted {
            let key = signals.lighting.unwrap().key;
            self.light_sent[usize::from(key.ticket.quad)] |= 1 << key.lane;
        }
        if sampling_accepted {
            self.sample_sent[usize::from(signals.sampling.unwrap().ticket.quad)] = true;
        }
        if output_step.accepted {
            accesses.push(Access {
                store: Store::Output,
                write: true,
                address: (self.write_quad as usize & (SLOTS - 1)) * 8 + self.write_row,
            });
            self.write_row += 1;
            if self.write_row == 8 {
                self.write_row = 0;
                self.write_quad += 1;
            }
        }
        let mut retired = None;
        if let Some(address) = output_step.read_address {
            accesses.push(Access {
                store: Store::Output,
                write: false,
                address,
            });
        }
        if output_step.consumed {
            let s = self.status[(self.consume as usize) & (SLOTS - 1)]
                .ok_or("ROP consumed unowned status")?;
            let word = output_step.signals.output.unwrap();
            if word.last {
                self.status[usize::from(s.ticket.quad)] = None;
                self.banks[usize::from(s.context.slot)]
                    .as_mut()
                    .unwrap()
                    .references -= 1;
                self.consume += 1;
                retired = Some(s.ticket);
            }
        }
        if tick.ce {
            if let Some(w) = tick.light {
                self.write_light(w, &mut accesses)?;
            }
            if let Some(w) = tick.sample {
                self.write_sample(w, &mut accesses)?;
            }
            if self.initializing < SLOTS {
                // Each branch initializes its own done RAM through its one
                // writer, one address per edge; allocator is still disabled.
                self.light_done[self.initializing] = true;
                self.sample_done[self.initializing] = true;
                accesses.push(Access {
                    store: Store::LightDone,
                    write: true,
                    address: self.initializing,
                });
                accesses.push(Access {
                    store: Store::SampleDone,
                    write: true,
                    address: self.initializing,
                });
                self.initializing += 1;
            }
        }
        let mut released_draws = Vec::new();
        if tick.ce {
            while let Some(&(end, id)) = self.boundaries.front() {
                if end > self.consume {
                    break;
                }
                let draw = self.draw(id)?;
                if draw.references != 0 {
                    return Err("draw boundary passed live readers".into());
                }
                self.banks[usize::from(id.slot)] = None;
                self.boundaries.pop_front();
                released_draws.push(id);
            }
        }
        Ok(Step {
            signals,
            input_accepted: accepted,
            published,
            lighting_accepted,
            sampling_accepted,
            final_stage: final_step,
            final_accepted,
            accesses,
            retired,
            released_draws,
        })
    }

    fn write_light(&mut self, w: LightWrite, accesses: &mut Vec<Access>) -> Result<(), String> {
        let s = self.owned(w.key.ticket)?;
        let slot = usize::from(w.key.ticket.quad);
        if s.ticket.serial >= self.insert
            || s.unlit
            || w.key.lane > 3
            || s.header.mask & (1 << w.key.lane) == 0
            || self.light_seen[slot] & (1 << w.key.lane) != 0
            || w.value.g > 511
            || w.value.h > 256
        {
            return Err("Lighting result ownership/duplicate/range".into());
        }
        let address = slot * 4 + usize::from(w.key.lane);
        self.light[address] = u32::from(w.value.g) | (u32::from(w.value.h) << 9);
        self.light_seen[slot] |= 1 << w.key.lane;
        if self.light_seen[slot] != s.header.mask & ((1u8 << (w.key.lane + 1)) - 1) {
            return Err("Lighting outputs must remain ordered within a quad".into());
        }
        // Only this branch writes done, once, on its last covered lane. The
        // prefix bitmap above is a host witness; hardware needs lane/last only.
        if s.header.mask >> (w.key.lane + 1) == 0 {
            self.light_done[slot] = self.light_expected[slot];
            accesses.push(Access {
                store: Store::LightDone,
                write: true,
                address: slot,
            });
        }
        accesses.push(Access {
            store: Store::Light,
            write: true,
            address,
        });
        Ok(())
    }
    fn write_sample(&mut self, w: SampleWrite, accesses: &mut Vec<Access>) -> Result<(), String> {
        let s = self.owned(w.key.ticket)?;
        let slot = usize::from(w.key.ticket.quad);
        if s.ticket.serial >= self.insert
            || s.untextured
            || w.key.lane > 3
            || s.header.mask & (1 << w.key.lane) == 0
            || self.sample_seen[slot] & (1 << w.key.lane) != 0
        {
            return Err("Sampling result ownership/duplicate".into());
        }
        let address = slot * 4 + usize::from(w.key.lane);
        self.sample[address] = u32::from_le_bytes([w.rgb[0], w.rgb[1], w.rgb[2], 0]);
        self.sample_seen[slot] |= 1 << w.key.lane;
        if self.sample_seen[slot] != s.header.mask & ((1u8 << (w.key.lane + 1)) - 1) {
            return Err("Sampling outputs must remain ordered within a quad".into());
        }
        if s.header.mask >> (w.key.lane + 1) == 0 {
            self.sample_done[slot] = self.sample_expected[slot];
            accesses.push(Access {
                store: Store::SampleDone,
                write: true,
                address: slot,
            });
        }
        accesses.push(Access {
            store: Store::Sample,
            write: true,
            address,
        });
        Ok(())
    }
}
