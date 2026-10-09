use super::{Decoded, MAX_COVERAGE, MAX_RECORDS, MAX_SOURCES, MAX_WALL, WORDS};
use gpu_v2::{
    framebuffer::ports::Header,
    geometry::{record_transport as record, source_record_link as link},
    lighting::ports::PixelInput,
    system::pixel::{Basic, BranchQuad, LiveQuad, QuadInput},
    texture::ports::{Filter, QuadInput as TextureQuad},
};
use std::{fmt::Write, path::Path};

#[derive(Clone, Copy, Debug)]
pub struct Attributes {
    pub near: i32,
    pub far: i32,
    pub slot: u8,
    pub size: u8,
    pub filter: Filter,
    pub default_light: bool,
    pub default_sample: bool,
}
#[derive(Debug, Default)]
pub struct Stats {
    pub wall: u64,
    pub coverage_attempts: u64,
    pub helpers: u64,
    pub covered_captures: u64,
    pub offered: u64,
    pub accepted: u64,
    pub held: u64,
    pub decoded: u64,
    pub reads: u64,
    pub captured: u64,
    pub confirmations: u64,
    pub released: u64,
    pub aborted: u64,
    pub reserved: u64,
    pub published: u64,
    pub peak_published: usize,
    pub last_accept: Option<u64>,
    pub last_confirmation: Option<u64>,
    pub last_release: Option<u64>,
}
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub phase: &'static str,
    pub pending: bool,
    pub captured: bool,
    pub published: usize,
    pub free: usize,
    pub helper_lane: Option<usize>,
}
fn rne(n: i64, d: i64) -> i64 {
    let a = n.unsigned_abs();
    let den = d as u64;
    let q = a / den;
    let rem = a % den;
    let rounded = q + u64::from(2 * rem > den || 2 * rem == den && q & 1 != 0);
    if n < 0 {
        -(rounded as i64)
    } else {
        rounded as i64
    }
}
pub fn ndc(x: u16, y: u16) -> [i32; 2] {
    [
        rne((2 * i64::from(x) + 1 - 32) * 16384, 32) as i32,
        rne((32 - 2 * i64::from(y) - 1) * 16384, 32) as i32,
    ]
}
#[derive(Debug)]
enum QuadPhase {
    Cover { lane: usize, mask: u8 },
    Capture(usize),
    Held,
    Done,
}
struct Cursor {
    data: Decoded,
    tile: [u16; 2],
    xy: [u16; 2],
    edge_row: [i128; 3],
    edge_quad: [i128; 3],
    phase: QuadPhase,
    quad: BranchQuad,
    attributes: Attributes,
}
impl Cursor {
    fn new(data: Decoded, attributes: Attributes) -> Self {
        let tile = [data.bbox[0] / 16, data.bbox[1] / 16];
        let xy = tile.map(|v| v * 16);
        let seed = Self::seed(&data, xy);
        Self {
            data,
            tile,
            xy,
            edge_row: seed,
            edge_quad: seed,
            phase: QuadPhase::Cover { lane: 0, mask: 0 },
            quad: Self::empty(xy, attributes),
            attributes,
        }
    }
    fn seed(data: &Decoded, xy: [u16; 2]) -> [i128; 3] {
        let p = [
            i64::from(xy[0]) * 16 + 8 - data.origin[0],
            i64::from(xy[1]) * 16 + 8 - data.origin[1],
        ];
        data.edges.map(|e| e.coverage(p))
    }
    fn empty(xy: [u16; 2], a: Attributes) -> BranchQuad {
        BranchQuad {
            live: LiveQuad {
                quad: QuadInput {
                    header: Header {
                        x: xy[0],
                        y: xy[1] as u8,
                        mask: 0,
                    },
                    basic: [Basic::default(); 4],
                    default_light: a.default_light,
                    default_sample: a.default_sample,
                },
                light: [PixelInput {
                    normal: [0; 3],
                    ndc: [0; 2],
                }; 4],
            },
            sample: Some(TextureQuad {
                force_coarsest: false,
                quad_id: 0,
                mask: 0,
                uv: [[0.0; 2]; 4],
                slot: a.slot,
                material_size_log2: a.size,
                filter: a.filter,
                lod_bias: 0.0,
            }),
        }
    }
    fn advance(&mut self) {
        let [x, y] = self.xy;
        if x < self.tile[0] * 16 + 14 {
            self.xy[0] += 2;
            for (v, e) in self.edge_quad.iter_mut().zip(self.data.edges) {
                *v += 32 * e.a;
            }
        } else if y < self.tile[1] * 16 + 14 {
            self.xy = [self.tile[0] * 16, y + 2];
            for (v, e) in self.edge_row.iter_mut().zip(self.data.edges) {
                *v += 32 * e.b;
            }
            self.edge_quad = self.edge_row;
        } else {
            if self.tile[0] < self.data.bbox[2] / 16 {
                self.tile[0] += 1;
            } else if self.tile[1] < self.data.bbox[3] / 16 {
                self.tile = [self.data.bbox[0] / 16, self.tile[1] + 1];
            } else {
                self.phase = QuadPhase::Done;
                return;
            }
            self.xy = self.tile.map(|v| v * 16);
            self.edge_row = Self::seed(&self.data, self.xy);
            self.edge_quad = self.edge_row;
        }
        self.quad = Self::empty(self.xy, self.attributes);
        self.phase = QuadPhase::Cover { lane: 0, mask: 0 };
    }
    fn step(&mut self, ce: bool, accepted: bool, stats: &mut Stats) -> Result<(), String> {
        if accepted && (!ce || !matches!(self.phase, QuadPhase::Held)) {
            return Err("quad ACK without enabled held input".into());
        }
        if !ce {
            return Ok(());
        }
        match self.phase {
            QuadPhase::Cover { lane, mut mask } => {
                stats.coverage_attempts += 1;
                if stats.coverage_attempts > MAX_COVERAGE {
                    return Err("coverage attempt watchdog".into());
                }
                let x = self.xy[0] + (lane % 2) as u16;
                let y = self.xy[1] + (lane / 2) as u16;
                let [minx, miny, maxx, maxy] = self.data.bbox;
                let inside = x >= minx && y >= miny && x <= maxx && y <= maxy && x < 32 && y < 32;
                if inside
                    && self.edge_quad.iter().zip(self.data.edges).all(|(&v, e)| {
                        v + 16 * e.a * (lane % 2) as i128 + 16 * e.b * (lane / 2) as i128 >= 0
                    })
                {
                    mask |= 1 << lane;
                }
                if lane == 3 {
                    if mask == 0 {
                        self.advance();
                    } else {
                        self.quad.live.quad.header.mask = mask;
                        self.quad.sample.as_mut().unwrap().mask = mask;
                        self.phase = QuadPhase::Capture(0);
                    }
                } else {
                    self.phase = QuadPhase::Cover {
                        lane: lane + 1,
                        mask,
                    };
                }
            }
            QuadPhase::Capture(lane) => {
                let x = self.xy[0] + (lane % 2) as u16;
                let y = self.xy[1] + (lane / 2) as u16;
                let covered = self.quad.live.quad.header.mask >> lane & 1 != 0;
                let value = match self.data.evaluate(f64::from(x) + 0.5, f64::from(y) + 0.5) {
                    Ok(v) => Some(v),
                    Err(e) if !covered && e == "invalid helper W/attributes" => {
                        self.quad.sample.as_mut().unwrap().force_coarsest = true;
                        None
                    }
                    Err(e) => return Err(e),
                };
                if let Some(v) = &value {
                    let sample = self.quad.sample.as_mut().unwrap();
                    match v.uv() {
                        Ok(uv) => sample.uv[lane] = uv,
                        Err(_) if !covered => sample.force_coarsest = true,
                        Err(e) => return Err(e),
                    }
                }
                stats.helpers += 1;
                if covered {
                    let v = value.as_ref().ok_or("covered record sample missing")?;
                    self.quad.live.quad.basic[lane] = Basic {
                        tint: v.tint(),
                        depth: v.depth(self.attributes.near, self.attributes.far)?,
                    };
                    if !self.attributes.default_light {
                        self.quad.live.light[lane] = PixelInput {
                            normal: v.normal()?,
                            ndc: ndc(x, y),
                        };
                    }
                    stats.covered_captures += 1;
                }
                if lane == 3 {
                    self.phase = QuadPhase::Held;
                    stats.offered += 1;
                } else {
                    self.phase = QuadPhase::Capture(lane + 1);
                }
            }
            QuadPhase::Held => {
                if accepted {
                    stats.accepted += 1;
                    stats.last_accept = Some(stats.wall);
                    self.advance();
                } else {
                    stats.held += 1;
                }
            }
            QuadPhase::Done => {}
        }
        Ok(())
    }
}
// Variant storage is mutually exclusive, not another record/capture pool.
#[allow(clippy::large_enum_variant)]
enum Stage {
    Idle,
    Load {
        key: record::Key,
        words: [u64; WORDS],
        issued: usize,
        captured: usize,
    },
    Active {
        key: record::Key,
        cursor: Cursor,
    },
    Confirm {
        key: record::Key,
        word: u64,
        issued: bool,
    },
    Ack(record::Key),
    Aborting,
}
pub struct Reader {
    controller: record::Controller,
    stage: Stage,
    ready: [Option<record::Key>; 2],
    owners: [Option<record::Key>; 2],
    published: [bool; 2],
    abort_acked: [bool; 2],
    outstanding_read: bool,
    attributes: Attributes,
    pub stats: Stats,
    pub trace: Vec<(u64, record::Event)>,
}
impl Reader {
    pub fn new(context: u64, attributes: Attributes) -> Result<Self, String> {
        if attributes.near <= 0
            || attributes.far <= attributes.near
            || attributes.slot > 15
            || attributes.size > 10
        {
            return Err("reader context bounds".into());
        }
        Ok(Self {
            controller: record::Controller::new(
                context,
                record::Limits {
                    wall_edges: MAX_WALL,
                    sources: MAX_SOURCES,
                    records: MAX_RECORDS,
                },
            )
            .map_err(|e| format!("record init {e:?}"))?,
            stage: Stage::Idle,
            ready: [None; 2],
            owners: [None; 2],
            published: [false; 2],
            abort_acked: [false; 2],
            outstanding_read: false,
            attributes,
            stats: Stats::default(),
            trace: Vec::new(),
        })
    }
    pub fn offer(&self) -> Option<&BranchQuad> {
        match &self.stage {
            Stage::Active { cursor, .. } if matches!(cursor.phase, QuadPhase::Held) => {
                Some(&cursor.quad)
            }
            _ => None,
        }
    }
    pub fn view(&self) -> View {
        let phase = match &self.stage {
            Stage::Idle => "idle",
            Stage::Load { .. } => "load",
            Stage::Confirm { .. } => "confirm",
            Stage::Ack(_) => "ack",
            Stage::Aborting => "abort",
            Stage::Active { cursor, .. } => match cursor.phase {
                QuadPhase::Cover { .. } => "coverage",
                QuadPhase::Capture(_) => "helper",
                QuadPhase::Held => "hold",
                QuadPhase::Done => "exhausted",
            },
        };
        View {
            phase,
            pending: self.outstanding_read && self.controller.response().is_none(),
            captured: self.controller.response().is_some(),
            published: self.published.iter().filter(|&&v| v).count(),
            free: self.controller.free_slots(),
            helper_lane: match &self.stage {
                Stage::Active { cursor, .. } => match cursor.phase {
                    QuadPhase::Capture(lane) => Some(lane),
                    _ => None,
                },
                _ => None,
            },
        }
    }
    pub fn drained(&self) -> bool {
        self.controller.drained()
            && self.ready.iter().all(Option::is_none)
            && matches!(self.stage, Stage::Idle | Stage::Aborting)
    }
    pub fn cancel(&mut self) {
        self.controller.cancel();
        self.stage = Stage::Aborting;
        self.ready = [None; 2];
    }
    /// Sole record clock owner. Producer supplies only source/reserve/write/end.
    pub fn step(
        &mut self,
        ce: bool,
        input: record::Input,
        ready: bool,
        accepted: bool,
    ) -> Result<Vec<record::Event>, String> {
        self.step_with_clock(ce, input, ready, accepted, |controller, action| {
            Ok(link::Out {
                source: Vec::new(),
                transport: controller
                    .step(action)
                    .map_err(|e| format!("record step {e:?}"))?,
            })
        })
        .map(|out| out.transport)
    }
    /// Test-only composition seam. The callback owns exactly one record edge;
    /// action preparation and captured-word observation stay Reader-owned.
    /// A source connection must supply its real events, never a second step.
    pub fn step_with_clock(
        &mut self,
        ce: bool,
        mut input: record::Input,
        ready: bool,
        accepted: bool,
        clock: impl FnOnce(&mut record::Controller, record::Input) -> Result<link::Out, String>,
    ) -> Result<link::Out, String> {
        if input.read.is_some()
            || input.last_quad_ack.is_some()
            || input.abort_ack.is_some()
            || input.source_captured.is_some() && matches!(self.stage, Stage::Aborting)
        {
            return Err("consumer actions are reader-owned".into());
        }
        input.ce = ce;
        input.return_ready = ready;
        input.read = match &self.stage {
            Stage::Load { key, issued, .. } if *issued < WORDS => Some(record::Read {
                key: *key,
                row: *issued,
                consumer: if *issued < 11 {
                    record::Consumer::Coverage
                } else {
                    record::Consumer::Attribute
                },
                last_attribute_capture: false,
            }),
            Stage::Confirm {
                key, issued: false, ..
            } => Some(record::Read {
                key: *key,
                row: 50,
                consumer: record::Consumer::Attribute,
                last_attribute_capture: true,
            }),
            _ => None,
        };
        if let Stage::Ack(key) = self.stage {
            input.last_quad_ack = Some(key);
        }
        if matches!(self.stage, Stage::Aborting) {
            input.abort_ack = (0..2)
                .find(|&i| self.published[i] && !self.abort_acked[i])
                .and_then(|i| self.owners[i]);
        }
        self.stats.wall += 1;
        if self.stats.wall > MAX_WALL {
            return Err("reader wall watchdog".into());
        }
        let out = clock(&mut self.controller, input)?;
        let events = &out.transport;
        for e in events {
            if self.trace.len() >= (8 * MAX_WALL) as usize {
                return Err("record trace watchdog".into());
            }
            self.trace.push((self.stats.wall, e.clone()));
            match e {
                record::Event::Reserved(key) => {
                    self.owners[key.slot] = Some(*key);
                    self.abort_acked[key.slot] = false;
                    self.stats.reserved += 1;
                }
                record::Event::Published(key) => {
                    self.published[key.slot] = true;
                    self.stats.published += 1;
                    let free = self
                        .ready
                        .iter_mut()
                        .find(|k| k.is_none())
                        .ok_or("ready descriptor cap")?;
                    *free = Some(*key);
                    self.stats.peak_published = self
                        .stats
                        .peak_published
                        .max(self.published.iter().filter(|&&v| v).count());
                }
                record::Event::ReadIssued { .. } => {
                    self.outstanding_read = true;
                    self.stats.reads += 1;
                }
                record::Event::ConsumerCaptured(_) | record::Event::ReturnDiscarded(_) => {
                    self.outstanding_read = false;
                    self.stats.captured += 1;
                }
                record::Event::AbortAccepted(key) => {
                    self.abort_acked[key.slot] = true;
                }
                record::Event::PartialWriteDiscarded(key)
                | record::Event::RecordAborted(key)
                | record::Event::RecordReleased(key) => {
                    if self.owners[key.slot] != Some(*key) {
                        return Err("release owner identity".into());
                    }
                    self.owners[key.slot] = None;
                    self.published[key.slot] = false;
                    if matches!(e, record::Event::RecordReleased(_)) {
                        self.stats.released += 1;
                        self.stats.last_release = Some(self.stats.wall);
                    } else {
                        self.stats.aborted += 1;
                    }
                }
                _ => {}
            }
        }
        let old = std::mem::replace(&mut self.stage, Stage::Idle);
        self.stage = match old {
            Stage::Load {
                key,
                mut words,
                mut issued,
                mut captured,
            } => {
                for event in events {
                    match event {
                        record::Event::ReadIssued { key: k, row } => {
                            if *k != key || *row != issued {
                                return Err("read issue sequence/owner".into());
                            }
                            issued += 1;
                        }
                        record::Event::ConsumerCaptured(r) => {
                            let consumer = if captured < 11 {
                                record::Consumer::Coverage
                            } else {
                                record::Consumer::Attribute
                            };
                            if r.key != key
                                || r.row != captured
                                || r.consumer != consumer
                                || r.last_attribute_capture
                            {
                                return Err("captured sequence/owner/route".into());
                            }
                            words[captured] = r.word;
                            captured += 1;
                        }
                        _ => {}
                    }
                }
                if captured == WORDS {
                    self.stats.decoded += 1;
                    Stage::Active {
                        key,
                        cursor: Cursor::new(Decoded::from_words(&words)?, self.attributes),
                    }
                } else {
                    Stage::Load {
                        key,
                        words,
                        issued,
                        captured,
                    }
                }
            }
            Stage::Active { key, mut cursor } => {
                cursor.step(ce, accepted, &mut self.stats)?;
                if matches!(cursor.phase, QuadPhase::Done) {
                    Stage::Confirm {
                        key,
                        word: cursor.data.confirmation,
                        issued: false,
                    }
                } else {
                    Stage::Active { key, cursor }
                }
            }
            Stage::Confirm {
                key,
                word,
                mut issued,
            } => {
                let mut consumed = false;
                for event in events {
                    match event {
                        record::Event::ReadIssued { key: k, row } => {
                            if *k != key || *row != 50 || issued {
                                return Err("confirmation issue identity".into());
                            }
                            issued = true;
                            if self.stats.last_accept.is_some_and(|e| e >= self.stats.wall) {
                                return Err("confirmation not later than capture".into());
                            }
                        }
                        record::Event::ConsumerCaptured(r) => {
                            if r.key != key
                                || r.row != 50
                                || r.word != word
                                || r.consumer != record::Consumer::Attribute
                                || !r.last_attribute_capture
                            {
                                return Err("confirmation returned identity".into());
                            }
                            self.stats.confirmations += 1;
                            self.stats.last_confirmation = Some(self.stats.wall);
                            consumed = true;
                        }
                        _ => {}
                    }
                }
                if consumed {
                    Stage::Ack(key)
                } else {
                    Stage::Confirm { key, word, issued }
                }
            }
            Stage::Ack(key) => {
                if events.contains(&record::Event::RecordReleased(key)) {
                    if self
                        .stats
                        .last_confirmation
                        .is_none_or(|e| e >= self.stats.wall)
                    {
                        return Err("ACK not later than confirmation".into());
                    }
                    Stage::Idle
                } else {
                    Stage::Ack(key)
                }
            }
            s => s,
        };
        if matches!(self.stage, Stage::Idle) && self.ready[0].is_some() {
            let key = self.ready[0].take().unwrap();
            self.ready[0] = self.ready[1].take();
            self.stage = Stage::Load {
                key,
                words: [0; WORDS],
                issued: 0,
                captured: 0,
            };
        }
        Ok(out)
    }
    pub fn save(&self, dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let mut trace = String::new();
        for (wall, e) in &self.trace {
            writeln!(trace, "{wall},{e:?}").unwrap();
        }
        std::fs::write(dir.join(format!("{name}-record.txt")), trace).unwrap();
        std::fs::write(
            dir.join(format!("{name}-record-stats.txt")),
            format!("{:#?}", self.stats),
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::rne;
    #[test]
    fn ndc_signed_ties() {
        assert_eq!(
            [rne(3, 2), rne(5, 2), rne(-3, 2), rne(-5, 2)],
            [2, 2, -2, -2]
        );
        assert_eq!(super::ndc(0, 0), [-15872, 15872]);
        assert_eq!(super::ndc(31, 31), [15872, -15872]);
    }
}
