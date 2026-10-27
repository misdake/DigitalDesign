//! Single-outstanding, non-backpressurable SDRAM loader for VLIW input pages.
//!
//! This component does not interpret mesh headers or traverse mesh addresses.
//! Sources are 22-bit halfword addresses; destinations are 64-bit word indices
//! in the selected 512-word input memory. The caller grants an inactive page
//! before accepting a descriptor and keeps it writable through completion.
//! A nonempty configuration segment additionally requires whole-GPU drain.
//! There is no compute CE and no response-ready signal. An aborted/failed page
//! is never published, even though earlier writes are not rolled back.
//!
//! Reset is a global transport reset: the arbiter/controller must reset too,
//! or have no outstanding response. Use `abort` for a local cancellation;
//! an already presented request stays valid and its terminal response drains.

pub const RTL: &str = include_str!("vliw_dma.v");
pub const SDRAM_HALFWORDS: u32 = 1 << 22;
pub const LOCAL_WORDS: u16 = 512;
pub const ERROR_DESCRIPTOR: u8 = 1;
pub const ERROR_ABORT: u8 = 2;
pub const ERROR_MEMORY: u8 = 4;
pub const ERROR_PROTOCOL: u8 = 8;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Segment {
    pub source_halfword: u32,
    pub destination_word: u16,
    pub bytes: u16,
}

impl Segment {
    fn valid(self, alignment_halfwords: u32) -> bool {
        self.bytes == 0
            || (self.bytes <= 1024
                && self.bytes & 7 == 0
                && self.source_halfword < SDRAM_HALFWORDS
                && self.source_halfword & (alignment_halfwords - 1) == 0
                && self.source_halfword + u32::from(self.bytes.div_ceil(32)) * 16
                    <= SDRAM_HALFWORDS
                && self.destination_word < LOCAL_WORDS
                && self.destination_word + self.bytes / 8 <= LOCAL_WORDS)
    }
    fn overlaps(self, other: Self) -> bool {
        self.bytes != 0
            && other.bytes != 0
            && self.destination_word < other.destination_word + other.bytes / 8
            && other.destination_word < self.destination_word + self.bytes / 8
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Descriptor {
    pub page: bool,
    pub tag: u8,
    pub config: Segment,
    pub metadata_source_halfword: u32,
    pub metadata_destination_word: u16,
    pub payload: Segment,
}

impl Descriptor {
    pub fn metadata(self) -> Segment {
        Segment {
            source_halfword: self.metadata_source_halfword,
            destination_word: self.metadata_destination_word,
            bytes: 32,
        }
    }
    pub fn valid(self) -> bool {
        let m = self.metadata();
        self.config.valid(16)
            && m.valid(16)
            && self.payload.valid(64)
            && !self.config.overlaps(m)
            && !self.config.overlaps(self.payload)
            && !m.overlaps(self.payload)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Request {
    pub address_halfword: u32,
    /// 0/1/3 denote 32/64/128 bytes; 2 is never emitted.
    pub line_count_minus_1: u8,
}

impl Request {
    pub fn beats(self) -> u8 {
        4 * (self.line_count_minus_1 + 1)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Response {
    pub data: u64,
    pub last: bool,
    /// Like the real arbiter, error terminates ownership even without last.
    pub error: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub abort: bool,
    pub descriptor: Option<Descriptor>,
    pub page_writable: bool,
    pub gpu_drained: bool,
    pub request_ready: bool,
    pub response: Option<Response>,
    pub completion_ready: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Completion {
    pub page: bool,
    pub tag: u8,
    pub error: u8,
}

impl Completion {
    pub fn publish(self) -> bool {
        self.error == 0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Output {
    pub descriptor_ready: bool,
    pub request: Option<Request>,
    pub write: Option<(u16, u64)>,
    pub completion: Option<Completion>,
    pub busy: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Phase {
    #[default]
    Idle,
    Select,
    Request,
    Response,
    Complete,
}

#[derive(Clone, Debug, Default)]
pub struct Loader {
    phase: Phase,
    descriptor: Descriptor,
    segment: u8,
    source: u32,
    destination: u16,
    left: u8,
    burst_beats: u8,
    copy_beats: u8,
    beat: u8,
    error: u8,
}

impl Loader {
    fn request(&self) -> Request {
        let code = if self.left >= 16 && self.source & 63 == 0 {
            3
        } else if self.left >= 8 && self.source & 31 == 0 {
            1
        } else {
            0
        };
        Request {
            address_halfword: self.source,
            line_count_minus_1: code,
        }
    }
    fn malformed(&self, r: Response) -> bool {
        !r.error && (r.last != (self.beat == self.burst_beats - 1) || self.beat >= self.burst_beats)
    }
    pub fn output(&self, i: &Input) -> Output {
        if i.reset {
            return Output::default();
        }
        Output {
            descriptor_ready: self.phase == Phase::Idle
                && i.page_writable
                && (i.descriptor.is_none_or(|d| d.config.bytes == 0) || i.gpu_drained),
            request: (self.phase == Phase::Request).then(|| self.request()),
            write: i
                .response
                .filter(|r| {
                    self.phase == Phase::Response
                        && self.error == 0
                        && !i.abort
                        && !r.error
                        && !self.malformed(*r)
                        && self.beat < self.copy_beats
                })
                .map(|r| (self.destination + u16::from(self.beat), r.data)),
            completion: (self.phase == Phase::Complete).then_some(Completion {
                page: self.descriptor.page,
                tag: self.descriptor.tag,
                error: self.error,
            }),
            busy: self.phase != Phase::Idle,
        }
    }
    /// Return pre-edge observations, then advance one memory/wall-clock edge.
    pub fn step(&mut self, i: Input) -> Output {
        let out = self.output(&i);
        if i.reset {
            *self = Self::default();
            return out;
        }
        let mut error = self.error;
        if !matches!(self.phase, Phase::Idle | Phase::Complete) {
            if i.abort {
                error |= ERROR_ABORT;
            }
            if i.response.is_some() && self.phase != Phase::Response {
                error |= ERROR_PROTOCOL;
            }
        }
        match self.phase {
            Phase::Idle => {
                if let Some(d) = i.descriptor.filter(|_| out.descriptor_ready) {
                    self.descriptor = d;
                    self.segment = 0;
                    error = if d.valid() { 0 } else { ERROR_DESCRIPTOR };
                    if i.abort {
                        error |= ERROR_ABORT;
                    }
                    self.phase = if error == 0 {
                        Phase::Select
                    } else {
                        Phase::Complete
                    };
                }
            }
            Phase::Select => {
                if error != 0 {
                    self.phase = Phase::Complete;
                } else {
                    if self.segment == 0 && self.descriptor.config.bytes == 0 {
                        self.segment = 1;
                    }
                    let s = match self.segment {
                        0 => self.descriptor.config,
                        1 => self.descriptor.metadata(),
                        _ => self.descriptor.payload,
                    };
                    if s.bytes == 0 {
                        self.phase = Phase::Complete;
                    } else {
                        self.source = s.source_halfword;
                        self.destination = s.destination_word;
                        self.left = (s.bytes / 8) as u8;
                        self.phase = Phase::Request;
                    }
                }
            }
            Phase::Request => {
                if i.request_ready {
                    self.burst_beats = self.request().beats();
                    self.copy_beats = self.left.min(self.burst_beats);
                    self.beat = 0;
                    self.phase = Phase::Response;
                }
            }
            Phase::Response => {
                if let Some(r) = i.response {
                    if r.error {
                        error |= ERROR_MEMORY;
                    }
                    if self.malformed(r) {
                        error |= ERROR_PROTOCOL;
                    }
                    if r.error || r.last {
                        if error != 0 {
                            self.phase = Phase::Complete;
                        } else if self.left == self.copy_beats {
                            if self.segment == 2 {
                                self.phase = Phase::Complete;
                            } else {
                                self.segment += 1;
                                self.phase = Phase::Select;
                            }
                        } else {
                            self.left -= self.copy_beats;
                            self.source += u32::from(self.burst_beats) * 4;
                            self.destination += u16::from(self.copy_beats);
                            self.phase = Phase::Request;
                        }
                    } else {
                        self.beat = self.beat.saturating_add(1).min(31);
                    }
                }
            }
            Phase::Complete => {
                if i.completion_ready {
                    self.phase = Phase::Idle;
                }
            }
        }
        self.error = error;
        out
    }
}
