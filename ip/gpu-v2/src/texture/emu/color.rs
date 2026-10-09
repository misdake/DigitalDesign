//! Eight enabled-edge color pipeline from already captured cache texels.
//! Twelve 9x8 products occupy three Multiply9 macros (six DSP18 equivalents),
//! with three registered product stages. These are Rust role/latency declarations,
//! not generated RTL or fitted resources. No numerical template is evaluated.
use crate::texture::ports::Group4;
use std::collections::VecDeque;

pub const RESULT_CAPACITY: usize = 16;
pub const LATENCY: u64 = 8;
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
}
pub const ALLOCATION: Allocation = Allocation {
    dsp9_lanes: 12,
    dsp18_equivalents: 6,
    hard_product_bits: 12 * 17 * 3,
    // decoded140, product metadata24, pairs110, partial59, final sum57,
    // normalization33, pipeline valid8, feedback58, input-stream owner7.
    datapath_ff_bits: 140 + 24 + 110 + 59 + 57 + 33 + 8 + 58 + 7,
    // Result16x30, ring read/write pointers8 + occupancy5, credit5, fault1.
    result_and_control_ff_bits: 16 * 30 + 8 + 5 + 5 + 1,
    bsram: 0,
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
    pub pipeline: [bool; 8],
    /// Includes every last-group reservation, pipeline and queued result.
    pub result_credits: usize,
    pub queued: usize,
    pub stream_owner: Option<u8>,
    pub accumulator_owner: Option<u8>,
}
#[derive(Clone, Debug)]
pub struct Step {
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
struct Pairs {
    meta: Meta,
    values: [[u32; 3]; 2],
}
#[derive(Clone, Copy)]
struct Partial {
    meta: Meta,
    value: [u32; 3],
}
#[derive(Clone, Copy)]
struct Sum {
    key: u8,
    value: [u32; 3],
}
#[derive(Clone, Copy)]
struct Normalize {
    key: u8,
    parts: [(u8, u8); 3],
}

pub struct ColorEmu {
    decoded: Option<Decoded>,
    products: [Option<Products>; 3],
    pairs: Option<Pairs>,
    partial: Option<Partial>,
    sum: Option<Sum>,
    normalize: Option<Normalize>,
    accumulator: Option<Sum>,
    stream_owner: Option<u8>,
    results: VecDeque<Output>,
    credits: usize,
    wall: u64,
    enabled: u64,
    max_wall: u64,
    faulted: bool,
}
impl ColorEmu {
    pub fn new(max_wall: u64) -> Result<Self, String> {
        if max_wall == 0 || max_wall > 2_000_000 {
            return Err("color watchdog bound".into());
        }
        Ok(Self {
            decoded: None,
            products: [None; 3],
            pairs: None,
            partial: None,
            sum: None,
            normalize: None,
            accumulator: None,
            stream_owner: None,
            results: VecDeque::new(),
            credits: 0,
            wall: 0,
            enabled: 0,
            max_wall,
            faulted: false,
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            wall: self.wall,
            enabled: self.enabled,
            pipeline: [
                self.decoded.is_some(),
                self.products[0].is_some(),
                self.products[1].is_some(),
                self.products[2].is_some(),
                self.pairs.is_some(),
                self.partial.is_some(),
                self.sum.is_some(),
                self.normalize.is_some(),
            ],
            result_credits: self.credits,
            queued: self.results.len(),
            stream_owner: self.stream_owner,
            accumulator_owner: self.accumulator.map(|a| a.key),
        }
    }
    pub fn idle(&self) -> bool {
        let s = self.snapshot();
        !s.pipeline.into_iter().any(|v| v)
            && self.results.is_empty()
            && self.accumulator.is_none()
            && self.stream_owner.is_none()
            && self.credits == 0
    }
    pub fn faulted(&self) -> bool {
        self.faulted
    }
    pub fn tick(&mut self, tick: Tick) -> Result<Step, String> {
        if self.faulted {
            return Err("color terminal fault; recreate before reuse".into());
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
        self.wall += 1;
        let output = self.results.front().copied();
        let last = tick.input.is_none_or(|p| p.payload >> 65 & 1 != 0);
        let input_ready = tick.ce && (!last || self.credits < RESULT_CAPACITY);
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
        if tick.ce {
            self.enabled += 1;
            if tick.output_ready {
                if let Some(value) = self.results.pop_front() {
                    self.credits -= 1;
                    events.push(Event::Commit(value));
                }
            }
            if let Some(n) = self.normalize.take() {
                let value = Output {
                    key: n.key,
                    rgb: n.parts.map(|(h, inc)| h + inc),
                };
                if self.results.len() == RESULT_CAPACITY {
                    return Err("color reserved result overflow".into());
                }
                self.results.push_back(value);
                events.push(Event::Queued(value));
            }
            if let Some(s) = self.sum.take() {
                let mut parts = [(0, 0); 3];
                for (out, value) in parts.iter_mut().zip(s.value) {
                    *out = normalize_parts(value)?;
                }
                self.normalize = Some(Normalize { key: s.key, parts });
            }
            if let Some(p) = self.partial.take() {
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
                let value = std::array::from_fn(|c| previous[c] + p.value[c]);
                if value.iter().any(|&v| v > MAX_SUM) {
                    return Err("color sum outside legal domain".into());
                }
                let sum = Sum {
                    key: p.meta.key,
                    value,
                };
                self.accumulator = (!p.meta.last).then_some(sum);
                self.sum = p.meta.last.then_some(sum);
                events.push(Event::Accumulate {
                    key: sum.key,
                    value,
                });
            }
            if let Some(p) = self.pairs.take() {
                let value = std::array::from_fn(|c| p.values[0][c] + p.values[1][c]);
                if value.iter().any(|&v| v > MAX_SUM) {
                    return Err("color partial outside legal domain".into());
                }
                self.partial = Some(Partial {
                    meta: p.meta,
                    value,
                });
                events.push(Event::Partial {
                    key: p.meta.key,
                    value,
                });
            }
            if let Some(p) = self.products[2].take() {
                let values = std::array::from_fn(|i| {
                    std::array::from_fn(|c| p.values[2 * i][c] + p.values[2 * i + 1][c])
                });
                if values.iter().flatten().any(|&v| v > MAX_SUM) {
                    return Err("color pair outside legal domain".into());
                }
                self.pairs = Some(Pairs {
                    meta: p.meta,
                    values,
                });
            }
            self.products[2] = self.products[1].take();
            if let Some(p) = self.products[2] {
                events.push(Event::Products {
                    key: p.meta.key,
                    values: p.values,
                });
            }
            self.products[1] = self.products[0].take();
            if let Some(d) = self.decoded.take() {
                self.products[0] = Some(Products {
                    meta: d.meta,
                    values: std::array::from_fn(|t| {
                        std::array::from_fn(|c| d.weights[t] * u32::from(d.colors[t][c]))
                    }),
                });
            }
            if let Some(d) = incoming {
                self.stream_owner = (!d.meta.last).then_some(d.meta.key);
                self.credits += usize::from(d.meta.last);
                events.push(Event::Accepted {
                    key: d.meta.key,
                    first: d.meta.first,
                    last: d.meta.last,
                });
                self.decoded = Some(d);
            }
        }
        let s = self.snapshot();
        let in_flight_last = usize::from(self.decoded.is_some_and(|d| d.meta.last))
            + self
                .products
                .iter()
                .filter(|p| p.is_some_and(|p| p.meta.last))
                .count()
            + usize::from(self.pairs.is_some_and(|p| p.meta.last))
            + usize::from(self.partial.is_some_and(|p| p.meta.last))
            + usize::from(self.sum.is_some())
            + usize::from(self.normalize.is_some());
        if self.credits > RESULT_CAPACITY || self.credits != self.results.len() + in_flight_last {
            return Err("color result credit ownership".into());
        }
        Ok(Step {
            input_ready,
            accepted,
            output,
            events,
            snapshot: s,
        })
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
    let h = (value >> 9) as u8;
    let low = value & 255;
    let inc = u8::from(value & 256 != 0 || u32::from(h) + low >= 256);
    Ok((h, inc))
}
#[cfg(test)]
mod tests {
    use super::*;
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
