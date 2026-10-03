use super::*;
use crate::framebuffer::sim::bounded::{self, OutputRow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    Basic,
    Light,
    Sample,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
    pub store: Store,
    pub write: bool,
    pub address: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Running,
    Closing,
    Flushing,
    Complete,
    FaultDraining,
    Faulted,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Admitted(Ticket),
    Dropped,
    Joined(Ticket),
    BasicDone(PixelKey),
    LightDone(PixelKey),
    SampleDone(PixelKey),
    ReadIssued { key: PixelKey, depth: bool },
    Captured { key: PixelKey, depth: bool },
    Consumed { key: PixelKey, depth: bool },
    OutputAccepted { ticket: Ticket, row: u8 },
    Retired(Ticket),
    FlushRequested,
    Complete,
    Rejected(String),
    Discarded { quads: usize },
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub wall_cycles: u64,
    pub admitted: u64,
    pub dropped: u64,
    pub retired: u64,
    pub peak_live: usize,
    pub light_writes: u64,
    pub sample_writes: u64,
    pub basic_reads: u64,
    pub light_reads: u64,
    pub sample_reads: u64,
    pub input_stalls: u64,
    pub output_stalls: u64,
    pub ce_returns: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub phase: Phase,
    pub live: usize,
    pub ingress: bool,
    pub read_pending: bool,
    pub return_valid: bool,
    pub output_valid: bool,
}
#[derive(Clone, Debug)]
pub struct Cycle {
    pub wall: u64,
    pub ce: bool,
    pub quad_accepted: bool,
    pub ticket: Option<Ticket>,
    pub light_accepted: bool,
    pub sample_accepted: bool,
    pub events: Vec<Event>,
    pub accesses: Vec<Access>,
    pub framebuffer: bounded::Cycle,
    pub snapshot: Snapshot,
}
#[derive(Clone, Copy)]
struct HeaderState {
    ticket: Ticket,
    header: Header,
    default_light: bool,
    default_sample: bool,
    basic_done: u8,
    light_done: u8,
    sample_done: u8,
}
impl HeaderState {
    fn ready(self) -> bool {
        self.basic_done & self.light_done & self.sample_done == self.header.mask
    }
}
struct Ingress {
    ticket: Ticket,
    basic: [Basic; 4],
    row: u8,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum FinalPhase {
    ColorRead,
    ColorWait,
    ColorOutput,
    DepthRead,
    DepthWait,
    DepthOutput,
}
#[derive(Clone, Copy)]
struct Final {
    key: PixelKey,
    phase: FinalPhase,
}
#[derive(Clone, Copy)]
enum Data {
    Color {
        tint: [u8; 3],
        texture: [u8; 3],
        light: LightingOutput,
    },
    Depth(u16),
}
#[derive(Clone, Copy)]
struct Return {
    key: PixelKey,
    data: Data,
    issued: u64,
}
impl Return {
    fn depth(self) -> bool {
        matches!(self.data, Data::Depth(_))
    }
}

/// One basic ingress writer, one final reader, one reserved synchronous return
/// and one held output row. Payload stores are fixed arrays with explicit ports.
/// Ownership witnesses are diagnostic, not fitted hardware tag/resource claims.
pub struct Model {
    context: Context,
    framebuffer: bounded::Model,
    phase: Phase,
    max_cycles: u64,
    head: u64,
    tail: u64,
    headers: [Option<HeaderState>; 16],
    basic: [u32; 128],
    light: [u32; 64],
    sample: [u32; 64],
    basic_owner: [Option<Ticket>; 128],
    light_owner: [Option<Ticket>; 64],
    sample_owner: [Option<Ticket>; 64],
    ingress: Option<Ingress>,
    final_work: Option<Final>,
    pending: Option<Return>,
    returned: Option<Return>,
    output: Option<OutputRow>,
    pub stats: Stats,
}
impl Model {
    pub fn new(context: Context, max_cycles: u64) -> Result<Self, String> {
        if max_cycles == 0 {
            return Err("pixel cycle bound".into());
        }
        Ok(Self {
            framebuffer: bounded::Model::new_forwarding(context.surface, context.rop)?,
            context,
            phase: Phase::Running,
            max_cycles,
            head: 0,
            tail: 0,
            headers: [None; 16],
            // Stale, deliberately different payloads. Default branches never
            // write or read these values, even after a slot wraps.
            basic: [0xa5a5_a5a5; 128],
            light: [0x2aaaa; 64],
            sample: [0x5a5a5a; 64],
            basic_owner: [None; 128],
            light_owner: [None; 64],
            sample_owner: [None; 64],
            ingress: None,
            final_work: None,
            pending: None,
            returned: None,
            output: None,
            stats: Stats::default(),
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            phase: self.phase,
            live: (self.tail - self.head) as usize,
            ingress: self.ingress.is_some(),
            read_pending: self.pending.is_some(),
            return_valid: self.returned.is_some(),
            output_valid: self.output.is_some(),
        }
    }
    pub fn complete(&self) -> bool {
        self.phase == Phase::Complete
    }
    pub fn drained(&self) -> bool {
        self.phase == Phase::Faulted
    }
    pub fn abort(&mut self) {
        if !matches!(self.phase, Phase::Faulted | Phase::Complete) {
            self.phase = Phase::FaultDraining;
            self.framebuffer.abort();
        }
    }
    fn header(&self, key: PixelKey) -> Result<HeaderState, String> {
        if key.ticket.quad > 15 || key.lane > 3 {
            return Err("pixel result key bounds".into());
        }
        let h = self.headers[usize::from(key.ticket.quad)]
            .filter(|h| h.ticket == key.ticket)
            .ok_or("pixel result stale/unallocated ticket")?;
        if h.header.mask >> key.lane & 1 == 0 {
            return Err("pixel result uncovered lane".into());
        }
        Ok(h)
    }
    fn validate_tick(&self, tick: Tick) -> Result<(), String> {
        if !tick.ce || !matches!(self.phase, Phase::Running | Phase::Closing) {
            return Ok(());
        }
        if let Some(q) = tick
            .quad
            .filter(|_| self.phase == Phase::Running && !tick.finish)
        {
            if q.header.mask != 0 {
                // Validate the host-sized coordinate before the framebuffer
                // helper adds one. Malformed u16::MAX must reject, not overflow.
                if q.header.x >= self.context.surface.width {
                    return Err("quad bounds/alignment/mask".into());
                }
                self.context.surface.validate_header(q.header)?;
            }
        }
        if let Some(w) = tick.light {
            let h = self.header(w.key)?;
            if h.default_light || h.light_done >> w.key.lane & 1 != 0 {
                return Err("pixel light duplicate/default write".into());
            }
            if w.value.g > 511 || w.value.h > 256 {
                return Err("pixel light range".into());
            }
        }
        if let Some(w) = tick.sample {
            let h = self.header(w.key)?;
            if h.default_sample || h.sample_done >> w.key.lane & 1 != 0 {
                return Err("pixel sample duplicate/default write".into());
            }
        }
        Ok(())
    }
    fn access(trace: &mut Vec<Access>, store: Store, write: bool, address: usize) {
        assert!(
            !trace.iter().any(|a| a.store == store && a.write == write),
            "store port overbook"
        );
        assert!(
            !trace
                .iter()
                .any(|a| a.store == store && usize::from(a.address) == address),
            "same-row read/write"
        );
        trace.push(Access {
            store,
            write,
            address: address as u8,
        });
    }
    fn issue(
        &mut self,
        work: Final,
        events: &mut Vec<Event>,
        accesses: &mut Vec<Access>,
    ) -> Result<(), String> {
        if self.pending.is_some() || self.returned.is_some() || self.output.is_some() {
            return Err("pixel read without reserved return position".into());
        }
        let h = self.header(work.key)?;
        let p = usize::from(work.key.ticket.quad) * 4 + usize::from(work.key.lane);
        let depth = work.phase == FinalPhase::DepthRead;
        let row = p * 2 + usize::from(depth);
        if self.basic_owner[row] != Some(h.ticket) {
            return Err("pixel basic read stale owner".into());
        }
        Self::access(accesses, Store::Basic, false, row);
        self.stats.basic_reads += 1;
        let data = if depth {
            Data::Depth(self.basic[row] as u16)
        } else {
            let light = if h.default_light {
                LightingOutput { g: 256, h: 0 }
            } else {
                if self.light_owner[p] != Some(h.ticket) {
                    return Err("pixel light read stale owner".into());
                }
                Self::access(accesses, Store::Light, false, p);
                self.stats.light_reads += 1;
                LightingOutput {
                    g: (self.light[p] & 511) as u16,
                    h: (self.light[p] >> 9) as u16,
                }
            };
            let texture = if h.default_sample {
                [255; 3]
            } else {
                if self.sample_owner[p] != Some(h.ticket) {
                    return Err("pixel sample read stale owner".into());
                }
                Self::access(accesses, Store::Sample, false, p);
                self.stats.sample_reads += 1;
                let bytes = self.sample[p].to_le_bytes();
                [bytes[0], bytes[1], bytes[2]]
            };
            let bytes = self.basic[row].to_le_bytes();
            Data::Color {
                tint: [bytes[0], bytes[1], bytes[2]],
                texture,
                light,
            }
        };
        self.pending = Some(Return {
            key: work.key,
            data,
            issued: self.stats.wall_cycles,
        });
        self.final_work.as_mut().unwrap().phase = if depth {
            FinalPhase::DepthWait
        } else {
            FinalPhase::ColorWait
        };
        events.push(Event::ReadIssued {
            key: work.key,
            depth,
        });
        Ok(())
    }
    /// Every call advances the existing ROP/MC once. CE gates injection/final;
    /// local synchronous return capture has its reserved slot even under CE=0.
    /// Invalid injection latches a terminal fault but keeps clocking accepted MC
    /// work. A wall watchdog requires the caller to drain transport externally.
    pub fn step(&mut self, tick: Tick, memory: &mut impl MemoryPort) -> Result<Cycle, String> {
        if self.stats.wall_cycles >= self.max_cycles {
            self.abort();
            return Err("pixel wall-cycle watchdog; drain transport externally".into());
        }
        self.stats.wall_cycles += 1;
        let mut events = vec![];
        let mut accesses = vec![];
        if let Err(error) = self.validate_tick(tick) {
            self.abort();
            events.push(Event::Rejected(error));
        }
        let running = matches!(self.phase, Phase::Running | Phase::Closing);
        let ingress_free = self.ingress.is_none();
        let final_was_empty = self.final_work.is_none();
        let head_ready = self.headers[(self.head & 15) as usize].is_some_and(HeaderState::ready);
        // Save the old valid. A newly captured return cannot be consumed here.
        let old_return = self.returned;
        if let Some(ret) = self.pending.take() {
            assert_eq!(ret.issued + 1, self.stats.wall_cycles);
            assert!(self.returned.is_none(), "reserved store return overwritten");
            self.returned = Some(ret);
            self.stats.ce_returns += u64::from(!tick.ce);
            events.push(Event::Captured {
                key: ret.key,
                depth: ret.depth(),
            });
        }
        let offered = self
            .output
            .filter(|_| running && tick.ce && tick.final_ready);
        let framebuffer_ce = tick.ce
            && !matches!(
                self.phase,
                Phase::Complete | Phase::FaultDraining | Phase::Faulted
            );
        let framebuffer = match self.framebuffer.step(framebuffer_ce, offered, memory) {
            Ok(cycle) => cycle,
            Err(error) => {
                self.abort();
                return Err(error);
            }
        };
        if self.framebuffer.fault {
            self.abort();
        }
        let mut quad_accepted = false;
        let mut ticket = None;
        let mut light_accepted = false;
        let mut sample_accepted = false;
        if self.phase == Phase::FaultDraining {
            if self.framebuffer.drained() && self.pending.is_none() {
                events.push(Event::Discarded {
                    quads: (self.tail - self.head) as usize,
                });
                self.headers = [None; 16];
                self.head = self.tail;
                self.ingress = None;
                self.final_work = None;
                self.returned = None;
                self.output = None;
                self.phase = Phase::Faulted;
            }
        } else if running && tick.ce {
            if tick.finish {
                self.phase = Phase::Closing;
            }
            if framebuffer.input_accepted {
                let row = self.output.take().unwrap();
                let work = self.final_work.as_mut().unwrap();
                events.push(Event::OutputAccepted {
                    ticket: work.key.ticket,
                    row: row.row,
                });
                if row.row == 7 {
                    assert_eq!(framebuffer.output_published, Some(row.header));
                    assert_eq!(work.key.ticket.serial, self.head);
                    self.headers[usize::from(work.key.ticket.quad)] = None;
                    events.push(Event::Retired(work.key.ticket));
                    self.head += 1;
                    self.stats.retired += 1;
                    self.final_work = None;
                } else if row.row & 1 == 0 {
                    work.phase = FinalPhase::DepthRead;
                } else {
                    work.key.lane += 1;
                    work.phase = FinalPhase::ColorRead;
                }
            } else if offered.is_some() {
                self.stats.output_stalls += 1;
            }
            if let Some(ret) = old_return.filter(|_| tick.final_ready) {
                let work = self.final_work.as_mut().unwrap();
                assert_eq!(work.key, ret.key);
                let h = self.headers[usize::from(ret.key.ticket.quad)].unwrap();
                let data = match ret.data {
                    Data::Depth(d) => u32::from(d),
                    Data::Color {
                        tint,
                        texture,
                        light,
                    } => {
                        let rgb = final_rgb(tint, texture, light, self.context.specular)?;
                        u32::from_le_bytes([rgb[0], rgb[1], rgb[2], self.context.alpha])
                    }
                };
                self.output = Some(OutputRow {
                    header: h.header,
                    row: ret.key.lane * 2 + u8::from(ret.depth()),
                    data,
                });
                work.phase = if ret.depth() {
                    FinalPhase::DepthOutput
                } else {
                    FinalPhase::ColorOutput
                };
                self.returned = None;
                events.push(Event::Consumed {
                    key: ret.key,
                    depth: ret.depth(),
                });
            }
            // Only old readiness allows a join; writes below cannot bypass it.
            if final_was_empty && head_ready {
                let h = self.headers[(self.head & 15) as usize].unwrap();
                self.final_work = Some(Final {
                    key: PixelKey {
                        ticket: h.ticket,
                        lane: 0,
                    },
                    phase: FinalPhase::ColorRead,
                });
                events.push(Event::Joined(h.ticket));
            }
            if tick.final_ready {
                if let Some(work) = self
                    .final_work
                    .filter(|w| matches!(w.phase, FinalPhase::ColorRead | FinalPhase::DepthRead))
                {
                    let h = self.headers[usize::from(work.key.ticket.quad)].unwrap();
                    if h.header.mask >> work.key.lane & 1 == 0 {
                        let depth = work.phase == FinalPhase::DepthRead;
                        self.output = Some(OutputRow {
                            header: h.header,
                            row: work.key.lane * 2 + u8::from(depth),
                            data: 0,
                        });
                        self.final_work.as_mut().unwrap().phase = if depth {
                            FinalPhase::DepthOutput
                        } else {
                            FinalPhase::ColorOutput
                        };
                    } else {
                        self.issue(work, &mut events, &mut accesses)?;
                    }
                }
            }
            if let Some(w) = tick.light {
                let p = usize::from(w.key.ticket.quad) * 4 + usize::from(w.key.lane);
                Self::access(&mut accesses, Store::Light, true, p);
                self.light[p] = u32::from(w.value.g) | u32::from(w.value.h) << 9;
                self.light_owner[p] = Some(w.key.ticket);
                self.headers[usize::from(w.key.ticket.quad)]
                    .as_mut()
                    .unwrap()
                    .light_done |= 1 << w.key.lane;
                self.stats.light_writes += 1;
                light_accepted = true;
                events.push(Event::LightDone(w.key));
            }
            if let Some(w) = tick.sample {
                let p = usize::from(w.key.ticket.quad) * 4 + usize::from(w.key.lane);
                Self::access(&mut accesses, Store::Sample, true, p);
                self.sample[p] = u32::from_le_bytes([w.rgb[0], w.rgb[1], w.rgb[2], 0]);
                self.sample_owner[p] = Some(w.key.ticket);
                self.headers[usize::from(w.key.ticket.quad)]
                    .as_mut()
                    .unwrap()
                    .sample_done |= 1 << w.key.lane;
                self.stats.sample_writes += 1;
                sample_accepted = true;
                events.push(Event::SampleDone(w.key));
            }
            if let Some(mut ingress) = self.ingress.take() {
                let h = self.headers[usize::from(ingress.ticket.quad)]
                    .as_mut()
                    .unwrap();
                while ingress.row < 8 && h.header.mask >> (ingress.row / 2) & 1 == 0 {
                    ingress.row += 2;
                }
                if ingress.row < 8 {
                    let lane = ingress.row / 2;
                    let b = ingress.basic[usize::from(lane)];
                    let row = usize::from(ingress.ticket.quad) * 8 + usize::from(ingress.row);
                    Self::access(&mut accesses, Store::Basic, true, row);
                    self.basic[row] = if ingress.row & 1 == 0 {
                        u32::from_le_bytes([b.tint[0], b.tint[1], b.tint[2], 0])
                    } else {
                        u32::from(b.depth)
                    };
                    self.basic_owner[row] = Some(ingress.ticket);
                    if ingress.row & 1 == 1 {
                        h.basic_done |= 1 << lane;
                        events.push(Event::BasicDone(PixelKey {
                            ticket: ingress.ticket,
                            lane,
                        }));
                    }
                    ingress.row += 1;
                }
                while ingress.row < 8 && h.header.mask >> (ingress.row / 2) & 1 == 0 {
                    ingress.row += 2;
                }
                if ingress.row < 8 {
                    self.ingress = Some(ingress);
                }
            }
            if let Some(q) = tick.quad.filter(|_| self.phase == Phase::Running) {
                if q.header.mask == 0 {
                    quad_accepted = true;
                    self.stats.dropped += 1;
                    events.push(Event::Dropped);
                } else if ingress_free && self.tail - self.head < 16 {
                    let t = Ticket {
                        quad: (self.tail & 15) as u8,
                        serial: self.tail,
                    };
                    assert!(self.headers[usize::from(t.quad)].is_none());
                    self.headers[usize::from(t.quad)] = Some(HeaderState {
                        ticket: t,
                        header: q.header,
                        default_light: q.default_light,
                        default_sample: q.default_sample,
                        basic_done: 0,
                        light_done: if q.default_light { q.header.mask } else { 0 },
                        sample_done: if q.default_sample { q.header.mask } else { 0 },
                    });
                    self.ingress = Some(Ingress {
                        ticket: t,
                        basic: q.basic,
                        row: 0,
                    });
                    self.tail += 1;
                    self.stats.admitted += 1;
                    quad_accepted = true;
                    ticket = Some(t);
                    events.push(Event::Admitted(t));
                } else {
                    self.stats.input_stalls += 1;
                }
            }
        }
        if self.phase == Phase::Closing
            && self.head == self.tail
            && self.ingress.is_none()
            && self.final_work.is_none()
        {
            assert!(self.pending.is_none() && self.returned.is_none() && self.output.is_none());
            self.framebuffer.request_flush()?;
            self.phase = Phase::Flushing;
            events.push(Event::FlushRequested);
        }
        if self.phase == Phase::Flushing
            && self.framebuffer.flush_complete
            && self.framebuffer.idle()
        {
            self.phase = Phase::Complete;
            events.push(Event::Complete);
        }
        let snapshot = self.snapshot();
        assert!(snapshot.live <= 16);
        self.stats.peak_live = self.stats.peak_live.max(snapshot.live);
        Ok(Cycle {
            wall: self.stats.wall_cycles,
            ce: tick.ce,
            quad_accepted,
            ticket,
            light_accepted,
            sample_accepted,
            events,
            accesses,
            framebuffer,
            snapshot,
        })
    }
}
