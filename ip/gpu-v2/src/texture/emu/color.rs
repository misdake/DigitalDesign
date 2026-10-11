//! Two enabled-edge color pipeline with one registered 9x8 product edge.
//! The balanced four-tap tree and feedback share one edge; exact nearest /511
//! normalization and the held result write share the feedback edge. Live
//! ownership, CE and pre-edge result credits are retained.
use crate::texture::ports::Group4;

pub const RESULT_CAPACITY: usize = 16;
pub const LATENCY: u64 = 2;
/// Logical declarations for this standalone block. Pipeline product bits are
/// charged to the existing DSP roles; descriptor selection/CE/decode Logic still
/// requires lowering. Host watchdog/event history is not synthesized state.
#[derive(Clone, Copy, Debug)]
pub struct Allocation {
    pub dsp9_lanes: usize,
    pub dsp18_equivalents: usize,
    pub hard_product_bits: usize,
    pub datapath_ff_bits: usize,
    pub result_and_control_ff_bits: usize,
    pub bsram: usize,
    pub result_output_hard_bits: usize,
    pub sdp4_cells: usize,
}
pub const ALLOCATION: Allocation = Allocation {
    dsp9_lanes: 12,
    dsp18_equivalents: 6,
    hard_product_bits: 12 * 17,
    // Decode141, product metadata9, feedback58, stream owner7.
    // Twelve registered products occupy the bound DSP output registers.
    datapath_ff_bits: 141 + 9 + 58 + 7,
    // Asynchronous SRAM head: no payload head or shadow register.
    // Ring read/write pointers8 + occupancy5, credit5, fault1.
    result_and_control_ff_bits: 8 + 5 + 5 + 1,
    bsram: 0,
    result_output_hard_bits: 0,
    sdp4_cells: 8,
};
/// Closed Serial uses one32-bit register row, valid1, credits2 and fault1.
pub const SERIAL_CLOSED_ALLOCATION: Allocation = Allocation {
    result_and_control_ff_bits: 32 + 1 + 2 + 1,
    sdp4_cells: 0,
    ..ALLOCATION
};
const MAX_SUM: u32 = 255 * 511;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    /// Canonical Group4 word; the four texels are already in tap order.
    pub payload: i128,
    pub texels: [u16; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    /// quad_id << 2 | lane; independent of texture slot/context index.
    pub key: u8,
    pub rgb: [u8; 3],
}
#[derive(Clone, Copy, Debug)]
pub struct Tick {
    pub ce: bool,
    pub input: Option<Input>,
    pub output_ready: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Accepted { key: u8, first: bool, last: bool },
    Products { key: u8, values: [[u32; 3]; 4] },
    Partial { key: u8, value: [u32; 3] },
    Accumulate { key: u8, value: [u32; 3] },
    Queued(Output),
    Commit(Output),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub wall: u64,
    pub enabled: u64,
    pub pipeline: [bool; 2],
    /// Includes every last-group reservation, pipeline and queued result.
    pub result_credits: usize,
    pub queued: usize,
    pub result_head_valid: bool,
    pub result_pointers: [u8; 2],
    pub stream_owner: Option<u8>,
    pub accumulator_owner: Option<u8>,
}
/// Actual result memory accesses; observations never retain another payload.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResultAccess {
    pub write: Option<(u8, u32)>,
    /// Old owned asynchronous head, observed even when CE is low.
    pub read: Option<(u8, u32)>,
}
#[derive(Clone, Debug)]
pub struct Step {
    pub result_access: ResultAccess,
    /// Readiness for this offered packet, evaluated before the edge. No same-
    /// edge output-credit return is used to accept a blocked last group.
    pub input_ready: bool,
    pub accepted: bool,
    pub output: Option<Output>,
    pub events: Vec<Event>,
    pub snapshot: Snapshot,
}
#[derive(Clone, Copy)]
struct Meta {
    key: u8,
    first: bool,
    last: bool,
}
#[derive(Clone, Copy)]
struct Decoded {
    meta: Meta,
    weights: [u32; 4],
    colors: [[u8; 3]; 4],
}
#[derive(Clone, Copy)]
struct Products {
    meta: Meta,
    values: [[u32; 3]; 4],
}
#[derive(Clone, Copy)]
struct Sum {
    key: u8,
    value: [u32; 3],
}
pub struct ColorEmu {
    decoded: Option<Decoded>,
    products: Option<Products>,
    accumulator: Option<Sum>,
    stream_owner: Option<u8>,
    results: ResultFifo,
    credits: usize,
    wall: u64,
    enabled: u64,
    max_wall: u64,
    faulted: bool,
    trusted_weights: bool,
    integer_filter: Option<bool>,
}
impl ColorEmu {
    /// Independent two-edge integer filter experiment. Input comes from the
    /// conserved weight511 producer; true uses half-up taps, false floors.
    pub fn new_integer_filter(max_wall: u64, half_up: bool) -> Result<Self, String> {
        let mut value = Self::new(max_wall)?;
        value.integer_filter = Some(half_up);
        Ok(value)
    }
    pub fn allocation(&self) -> Allocation {
        if self.results.closed() {
            SERIAL_CLOSED_ALLOCATION
        } else {
            ALLOCATION
        }
    }
    /// Serial-only profile. Every enabled edge must present an always-ready
    /// sink; a sole result register replaces the standalone result SRAM.
    pub fn new_serial_closed(max_wall: u64) -> Result<Self, String> {
        let mut value = Self::new(max_wall)?;
        value.results.ram = ResultBank::Register(0x1234_5678);
        Ok(value)
    }
    /// Closed production caller only: weight prefixes are bounded by511.
    /// First/last owner, credit, header and sink checks remain active.
    pub(crate) fn new_serial_trusted(max_wall: u64) -> Result<Self, String> {
        let mut value = Self::new_serial_closed(max_wall)?;
        value.trusted_weights = true;
        Ok(value)
    }
    pub fn new(max_wall: u64) -> Result<Self, String> {
        if max_wall == 0 || max_wall > 2_000_000 {
            return Err("color watchdog bound".into());
        }
        Ok(Self {
            decoded: None,
            products: None,
            accumulator: None,
            stream_owner: None,
            results: ResultFifo::new(),
            credits: 0,
            wall: 0,
            enabled: 0,
            max_wall,
            faulted: false,
            trusted_weights: false,
            integer_filter: None,
        })
    }
    /// Reset control in this same instance. Payload RAM is deliberately retained.
    pub fn reset(&mut self) {
        self.decoded = None;
        self.products = None;
        self.accumulator = None;
        self.stream_owner = None;
        self.results.reset();
        self.credits = 0;
        self.wall = 0;
        self.enabled = 0;
        self.faulted = false;
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            wall: self.wall,
            enabled: self.enabled,
            pipeline: [self.decoded.is_some(), self.products.is_some()],
            result_credits: self.credits,
            queued: self.results.count,
            result_head_valid: self.results.count != 0,
            result_pointers: [self.results.head, self.results.tail],
            stream_owner: self.stream_owner,
            accumulator_owner: self.accumulator.map(|a| a.key),
        }
    }
    pub fn idle(&self) -> bool {
        let s = self.snapshot();
        !s.pipeline.into_iter().any(|v| v)
            && self.results.count == 0
            && self.accumulator.is_none()
            && self.stream_owner.is_none()
            && self.credits == 0
    }
    pub fn faulted(&self) -> bool {
        self.faulted
    }
    /// Pre-edge credit availability; CE remains a separate acceptance gate.
    /// A result consumed on this edge does not lend its credit to this input.
    pub fn input_ready(&self, last: bool) -> bool {
        !self.faulted && (self.results.closed() || !last || self.credits < RESULT_CAPACITY)
    }
    pub fn tick(&mut self, tick: Tick) -> Result<Step, String> {
        if self.faulted {
            return Err("color terminal fault; reset before reuse".into());
        }
        if self.wall == self.max_wall {
            self.faulted = true;
            return Err("color wall watchdog".into());
        }
        let r = self.advance(tick);
        if r.is_err() {
            self.faulted = true;
        }
        r
    }
    fn advance(&mut self, tick: Tick) -> Result<Step, String> {
        if self.results.closed() && tick.ce && !tick.output_ready {
            return Err("closed Serial Color requires an always-ready sink".into());
        }
        self.wall += 1;
        let output = self.results.output();
        let last = tick.input.is_none_or(|p| p.payload >> 65 & 1 != 0);
        let input_ready = tick.ce && self.input_ready(last);
        let accepted = input_ready && tick.input.is_some();
        // Decode/ownership is checked only for an accepted request. A caller
        // holds the same request while blocked; no values come from a Program.
        let incoming = if accepted {
            let p = tick.input.unwrap();
            let g = Group4::unpack72(p.payload)?;
            let key = g.quad_id * 4 + g.lane;
            if g.first {
                if self.stream_owner.is_some() {
                    return Err("color first interrupted sample".into());
                }
            } else if self.stream_owner != Some(key) {
                return Err("color non-first owner/order".into());
            }
            Some(Decoded {
                meta: Meta {
                    key,
                    first: g.first,
                    last: g.last,
                },
                weights: g.coefficients,
                colors: p.texels.map(expand),
            })
        } else {
            None
        };
        let mut events = vec![];
        let mut queued_output = None;
        let mut accumulator = self.accumulator;
        // Validate this edge before any FIFO access. In particular, an invalid
        // admission suppresses an older result write and a simultaneous pop.
        if tick.ce {
            if let Some(p) = self.products {
                let mut partial = std::array::from_fn(|c| {
                    if let Some(half_up) = self.integer_filter {
                        p.values
                            .iter()
                            .map(|tap| (tap[c] + if half_up { 256 } else { 0 }) >> 9)
                            .sum()
                    } else {
                        (p.values[0][c] + p.values[1][c]) + (p.values[2][c] + p.values[3][c])
                    }
                });
                // Diagnostic distinction only. RTL folds this into the
                // same-edge sum check: unsigned feedback is nonnegative,
                // and its full 20-bit sum cannot overflow.
                if !self.trusted_weights && partial.iter().any(|&v| v > MAX_SUM) {
                    return Err("color partial outside legal domain".into());
                }
                if self.trusted_weights {
                    partial.iter_mut().for_each(|v| *v &= 0x1ffff);
                }
                let previous = if p.meta.first {
                    if self.accumulator.is_some() {
                        return Err("color feedback first interrupted sample".into());
                    }
                    [0; 3]
                } else {
                    let a = self.accumulator.ok_or("color feedback missing owner")?;
                    if a.key != p.meta.key {
                        return Err("color feedback owner/order".into());
                    }
                    a.value
                };
                let mut value = std::array::from_fn(|c| previous[c] + partial[c]);
                if self.integer_filter.is_some() && value.iter().any(|&v| v > 255) {
                    return Err("integer filter requires a conserved weight511 prefix".into());
                }
                if !self.trusted_weights && value.iter().any(|&v| v > MAX_SUM) {
                    return Err("color sum outside legal domain".into());
                }
                if self.trusted_weights {
                    value.iter_mut().for_each(|v| *v &= 0x1ffff);
                }
                let sum = Sum {
                    key: p.meta.key,
                    value,
                };
                accumulator = (!p.meta.last).then_some(sum);
                if p.meta.last {
                    let mut rgb = [0; 3];
                    for (out, value) in rgb.iter_mut().zip(sum.value) {
                        if self.integer_filter.is_some() {
                            *out = value as u8;
                            continue;
                        }
                        let (h, inc) = if self.trusted_weights {
                            normalize_17(value)
                        } else {
                            normalize_parts(value)?
                        };
                        *out = h.wrapping_add(inc);
                    }
                    queued_output = Some(Output { key: sum.key, rgb });
                }
                events.push(Event::Partial {
                    key: p.meta.key,
                    value: partial,
                });
                events.push(Event::Accumulate {
                    key: sum.key,
                    value,
                });
            }
        }
        let pop = tick.ce && tick.output_ready && output.is_some();
        let credit_next =
            self.credits + usize::from(incoming.is_some_and(|d| d.meta.last)) - usize::from(pop);
        let count_next =
            self.results.count + usize::from(queued_output.is_some()) - usize::from(pop);
        let in_flight_last = if tick.ce {
            usize::from(incoming.is_some_and(|d| d.meta.last))
                + usize::from(self.decoded.is_some_and(|d| d.meta.last))
        } else {
            usize::from(self.decoded.is_some_and(|d| d.meta.last))
                + usize::from(self.products.is_some_and(|p| p.meta.last))
        };
        let credit_limit = if self.results.closed() {
            3
        } else {
            RESULT_CAPACITY
        };
        if credit_next > credit_limit || credit_next != count_next + in_flight_last {
            return Err("color result credit ownership".into());
        }
        let result_access = self.results.tick(tick.ce, pop, queued_output)?;
        if tick.ce {
            self.enabled += 1;
            self.credits = credit_next;
            self.accumulator = accumulator;
            if pop {
                events.insert(0, Event::Commit(output.unwrap()));
            }
            if let Some(value) = queued_output {
                events.push(Event::Queued(value));
            }
            self.products = self.decoded.map(|d| {
                let values = std::array::from_fn(|t| {
                    std::array::from_fn(|c| d.weights[t] * u32::from(d.colors[t][c]))
                });
                events.push(Event::Products {
                    key: d.meta.key,
                    values,
                });
                Products {
                    meta: d.meta,
                    values,
                }
            });
            self.decoded = incoming;
            if let Some(d) = incoming {
                self.stream_owner = (!d.meta.last).then_some(d.meta.key);
                events.push(Event::Accepted {
                    key: d.meta.key,
                    first: d.meta.first,
                    last: d.meta.last,
                });
            }
        }
        Ok(Step {
            result_access,
            input_ready,
            accepted,
            output,
            events,
            snapshot: self.snapshot(),
        })
    }
}
// Fixed Color result FIFO, with sixteen logical credits in eight RAM16SDP4.
// The asynchronous RAM head is observed directly, without a payload register.
struct ResultFifo {
    ram: ResultBank,
    head: u8,
    tail: u8,
    count: usize,
}
enum ResultBank {
    Sram([u32; 16]),
    Register(u32),
}
impl ResultFifo {
    fn closed(&self) -> bool {
        matches!(self.ram, ResultBank::Register(_))
    }
    fn read(&self) -> u32 {
        match &self.ram {
            ResultBank::Sram(ram) => ram[usize::from(self.head)],
            ResultBank::Register(word) => *word,
        }
    }
    fn new() -> Self {
        Self {
            ram: ResultBank::Sram(std::array::from_fn(|i| 0x1234_5678 ^ i as u32)),
            head: 0,
            tail: 0,
            count: 0,
        }
    }
    fn reset(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.count = 0;
    }
    fn output(&self) -> Option<Output> {
        let word = self.read();
        (self.count != 0).then_some(Output {
            key: (word >> 24) as u8,
            rgb: [word as u8, (word >> 8) as u8, (word >> 16) as u8],
        })
    }
    fn tick(&mut self, ce: bool, pop: bool, push: Option<Output>) -> Result<ResultAccess, String> {
        let mut ports = ResultAccess {
            read: (self.count != 0).then_some((self.head, self.read())),
            write: None,
        };
        if !ce {
            return Ok(ports);
        }
        if pop && self.count == 0 {
            return Err("color result unowned pop".into());
        }
        let capacity = if self.closed() { 1 } else { 16 };
        if self.count + usize::from(push.is_some()) - usize::from(pop) > capacity {
            return Err("color reserved result overflow".into());
        }
        // Valid old head may never alias a write: all16 credits at full capacity
        // are already in RAM, leaving no in-flight result to write on that edge.
        if !self.closed() && self.count != 0 && push.is_some() && self.head == self.tail {
            return Err("color RAM owned read/write collision".into());
        }
        if pop && !self.closed() {
            self.head = (self.head + 1) & 15;
        }
        if let Some(o) = push {
            let word = u32::from(o.key) << 24
                | u32::from(o.rgb[2]) << 16
                | u32::from(o.rgb[1]) << 8
                | u32::from(o.rgb[0]);
            ports.write = Some((self.tail, word));
            match &mut self.ram {
                ResultBank::Sram(ram) => {
                    ram[usize::from(self.tail)] = word;
                    self.tail = (self.tail + 1) & 15;
                }
                ResultBank::Register(row) => *row = word,
            }
        }
        self.count = self.count + usize::from(push.is_some()) - usize::from(pop);
        Ok(ports)
    }
}

fn expand(v: u16) -> [u8; 3] {
    let r = (v >> 11) as u8;
    let g = ((v >> 5) & 63) as u8;
    let b = (v & 31) as u8;
    [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2]
}
fn normalize_parts(value: u32) -> Result<(u8, u8), String> {
    if value > MAX_SUM {
        return Err("color normalize outside legal domain".into());
    }
    Ok(normalize_17(value))
}
fn normalize_17(value: u32) -> (u8, u8) {
    let h = (value >> 9) as u8;
    let low = value & 255;
    let inc = u8::from(value & 256 != 0 || u32::from(h) + low >= 256);
    (h, inc)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trusted_conserved_stream_matches_checked_pipeline_and_integer_golden() {
        let mut trusted = ColorEmu::new_serial_trusted(10_000).unwrap();
        let mut checked = ColorEmu::new_serial_closed(10_000).unwrap();
        let mut input = Vec::new();
        let mut golden = Vec::new();
        for pixel in 0..128_u16 {
            let key = (pixel % 64) as u8;
            let mut sum = [0_u32; 3];
            for (group, weight) in [1, 2, 3, 505].into_iter().enumerate() {
                let raw = if pixel == 0 {
                    0xffff
                } else {
                    pixel.wrapping_mul(613) ^ (group as u16 * 977)
                };
                let packet = Group4 {
                    key: crate::texture::ports::TileKey {
                        slot: 0,
                        n: 5,
                        x: 0,
                        y: 0,
                    },
                    top_left_local: [0, 0],
                    coefficients: [weight, 0, 0, 0],
                    first: group == 0,
                    last: group == 3,
                    quad_id: key / 4,
                    lane: key % 4,
                };
                input.push(Input {
                    payload: packet.pack72().unwrap() as i128,
                    texels: [raw; 4],
                });
                let rgb = expand(raw);
                for c in 0..3 {
                    sum[c] += u32::from(rgb[c]) * weight;
                }
            }
            golden.push(Output {
                key,
                rgb: sum.map(|s| ((s + 255) / 511) as u8),
            });
        }
        let mut at = 0;
        let mut output = Vec::new();
        let mut replacements = 0;
        for wall in 0..2_000 {
            let tick = Tick {
                ce: wall % 7 != 3,
                input: input.get(at).copied(),
                output_ready: true,
            };
            let a = trusted.tick(tick).unwrap();
            let b = checked.tick(tick).unwrap();
            assert_eq!(a.events, b.events);
            assert_eq!(a.snapshot, b.snapshot);
            assert_eq!(a.output, b.output);
            at += usize::from(a.accepted);
            replacements += usize::from(
                tick.ce && a.result_access.read.is_some() && a.result_access.write.is_some(),
            );
            if tick.ce {
                if let Some(v) = a.output {
                    output.push(v);
                }
            }
            if at == input.len() && trusted.idle() {
                break;
            }
        }
        assert_eq!(output, golden);
        assert!(trusted.idle() && checked.idle());
        // A continuous last-group stream separately exercises old-pop/new-push.
        trusted.reset();
        checked.reset();
        for wall in 0..100 {
            let packet = Group4 {
                key: crate::texture::ports::TileKey {
                    slot: 0,
                    n: 0,
                    x: 0,
                    y: 0,
                },
                top_left_local: [0, 0],
                coefficients: [511, 0, 0, 0],
                first: true,
                last: true,
                quad_id: (wall % 16) as u8,
                lane: 0,
            };
            let tick = Tick {
                ce: true,
                input: (wall < 96).then_some(Input {
                    payload: packet.pack72().unwrap() as i128,
                    texels: [0xffff; 4],
                }),
                output_ready: true,
            };
            let a = trusted.tick(tick).unwrap();
            let b = checked.tick(tick).unwrap();
            assert_eq!(a.events, b.events);
            replacements +=
                usize::from(a.result_access.read.is_some() && a.result_access.write.is_some());
        }
        assert!(replacements >= 90);
        assert!(trusted.idle());
    }
    #[test]
    fn normalize_matches_independent_division_for_entire_legal_domain() {
        for value in 0..=MAX_SUM {
            let (h, inc) = normalize_parts(value).unwrap();
            assert_eq!(
                u32::from(h) + u32::from(inc),
                (value + 255) / 511,
                "sum={value}"
            );
        }
        assert_eq!(
            normalize_parts(MAX_SUM + 1).unwrap_err(),
            "color normalize outside legal domain"
        );
    }
}
