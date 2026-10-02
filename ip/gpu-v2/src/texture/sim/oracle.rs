//! CPU oracle: configurable quantization and stage goldens, plus an independent
//! floating sampler. Cache operations are atomic functional transactions, not cycles.
use super::super::ports::*;
use std::collections::BTreeMap;

const TAPS: [[u8; 2]; 4] = [[0, 0], [1, 0], [0, 1], [1, 1]];

pub fn expand565(word: u16) -> [u8; 3] {
    let r = (word >> 11) as u8;
    let g = ((word >> 5) & 63) as u8;
    let b = (word & 31) as u8;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}
/// (bank, word within a line). Virtual +1 coordinates intentionally precede wrap.
pub fn bank_local(x: u8, y: u8) -> (usize, usize) {
    (
        usize::from(((y & 1) ^ ((x >> 1) & 1)) << 1 | (x & 1)),
        usize::from((y & 7) << 1 | ((x >> 2) & 1)),
    )
}
pub fn rne_div(value: u64, denominator: u64) -> u64 {
    let q = value / denominator;
    let r = value % denominator;
    q + u64::from(r > denominator / 2 || r * 2 == denominator && q & 1 != 0)
}

#[derive(Clone, Debug)]
pub struct LodGolden {
    pub uv: [[f64; 2]; 4],
    /// Horizontal top/bottom, vertical left/right. Each stores (du,dv).
    pub derivatives: [[f64; 2]; 4],
    pub rho: f64,
    pub overflow: bool,
    pub exponent: i32,
    pub table_index: Option<u8>,
    pub ideal: f64,
    pub selected: f64,
    pub raw: u32,
}
#[derive(Clone, Debug)]
pub struct LayerGolden {
    pub n: u8,
    pub parent: u32,
    pub p: [f64; 2],
    pub integer: [i64; 2],
    pub fraction: [u32; 2],
    pub taps: [[u16; 2]; 4],
    pub coefficients: [u32; 4],
}
#[derive(Clone, Debug)]
pub struct PixelGolden {
    pub lane: u8,
    pub lambda: u32,
    pub layers: Vec<LayerGolden>,
    pub groups: Vec<Group4>,
}
#[derive(Clone, Debug)]
pub struct PreparedQuad {
    pub quad_id: u8,
    pub lod: LodGolden,
    pub pixels: Vec<PixelGolden>,
    pub config: Config,
}

pub(crate) fn check_input(input: &QuadInput, slots: &[Slot]) -> Result<Slot, String> {
    if input.quad_id > 15
        || input.mask > 15
        || !input.lod_bias.is_finite()
        || input
            .uv
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1_048_576.0)
    {
        return Err("texture quad ID/mask/finite UV or bias bounds".into());
    }
    let slot = *slots
        .get(usize::from(input.slot))
        .ok_or("texture slot index")?;
    slot.validate()?;
    if input.slot > 15 || input.material_size_log2 != slot.max_size_log2 {
        return Err("texture material and slot size disagree".into());
    }
    Ok(slot)
}
fn derivatives(uv: [[f64; 2]; 4]) -> [[f64; 2]; 4] {
    [(0, 1), (2, 3), (0, 2), (1, 3)].map(|(a, b)| [uv[b][0] - uv[a][0], uv[b][1] - uv[a][1]])
}
fn max_derivative(d: [[f64; 2]; 4]) -> f64 {
    d.into_iter().flatten().map(f64::abs).fold(0.0, f64::max)
}
fn clamp_lod(raw: f64, bias: f64, max: u8) -> f64 {
    (raw + bias).clamp(0.0, f64::from(max))
}

pub fn prepare(input: &QuadInput, slots: &[Slot], config: Config) -> Result<PreparedQuad, String> {
    config.validate()?;
    let slot = check_input(input, slots)?;
    let max_lod = if slot.has_full_mip {
        slot.max_size_log2
    } else {
        0
    };
    let uv = input.uv.map(|p| {
        p.map(|v| match config.uv_fraction {
            Some(f) => {
                (v * 2.0_f64.powi(i32::from(f))).round_ties_even() / 2.0_f64.powi(i32::from(f))
            }
            None => v,
        })
    });
    let d = derivatives(uv);
    let slope = max_derivative(d);
    let rho = slope * f64::from(1_u16 << slot.max_size_log2);
    let overflow = slope > config.derivative_limit;
    // Compare against continuous input UV, not an exact-log oracle of already
    // quantized derivatives. UV precision and overflow policy must show up here.
    let input_rho = max_derivative(derivatives(input.uv)) * f64::from(1_u16 << slot.max_size_log2);
    let ideal = clamp_lod(input_rho.log2(), input.lod_bias, max_lod);
    let exponent = if rho > 0.0 {
        rho.log2().floor() as i32
    } else {
        0
    };
    let mut table_index = None;
    let bias = if config.lod_method == LodMethod::Table64Nearest {
        (input.lod_bias * 256.0).round_ties_even() / 256.0
    } else {
        input.lod_bias
    };
    let selected = if overflow {
        f64::from(max_lod)
    } else if rho == 0.0 {
        0.0
    } else {
        let log = match config.lod_method {
            LodMethod::Exact => rho.log2(),
            LodMethod::Table64 => {
                let mantissa = rho / 2.0_f64.powi(exponent);
                let index = ((mantissa - 1.0) * 64.0).floor().clamp(0.0, 63.0) as u8;
                table_index = Some(index);
                // Oracle-only ROM generation; a counted implementation will own
                // the selected immutable integer table and its storage cost.
                let fraction =
                    ((1.0 + f64::from(index) / 64.0).log2() * 256.0).round_ties_even() / 256.0;
                f64::from(exponent) + fraction
            }
            LodMethod::Table64Nearest => {
                let mantissa = rho / 2.0_f64.powi(exponent);
                let index = ((mantissa - 1.0) * 64.0).round_ties_even() as u8;
                table_index = Some(index);
                if index == 64 {
                    f64::from(exponent + 1)
                } else {
                    f64::from(exponent)
                        + ((1.0 + f64::from(index) / 64.0).log2() * 256.0).round_ties_even() / 256.0
                }
            }
        };
        clamp_lod(log, bias, max_lod)
    };
    let lod_scale = 1_u32 << config.lod_fraction;
    let raw = (selected * f64::from(lod_scale)).round_ties_even() as u32;
    let selected = f64::from(raw) / f64::from(lod_scale);
    let lod = LodGolden {
        uv,
        derivatives: d,
        rho,
        overflow,
        exponent,
        table_index,
        ideal,
        selected,
        raw,
    };
    let mut pixels = Vec::new();
    let scale = config.coefficient_scale();
    // These depend only on the shared quad LOD/material, never on lane UV.
    let level = if input.filter == Filter::Trilinear {
        raw / lod_scale
    } else {
        match config.mip_selection {
            MipSelection::Floor => raw / lod_scale,
            MipSelection::Nearest => (raw + lod_scale / 2) / lod_scale,
        }
    }
    .min(u32::from(max_lod)) as u8;
    let n = slot.max_size_log2 - level;
    let lambda = if input.filter == Filter::Trilinear && level < max_lod {
        rne_div(
            u64::from(raw % lod_scale) * u64::from(scale),
            u64::from(lod_scale),
        ) as u32
    } else {
        0
    };
    let mip_parents = [(n, scale - lambda), (n.saturating_sub(1), lambda)];
    for (lane, &pixel_uv) in uv.iter().enumerate() {
        if input.mask & (1 << lane) == 0 {
            continue;
        }
        let mut layers = Vec::new();
        let mut groups = Vec::new();
        for (size, parent) in mip_parents {
            if parent == 0 {
                continue;
            }
            let layer = prepare_layer(pixel_uv, size, parent, input.filter, config);
            // Stable first-tap encounter order within a mip; all mip0 groups
            // precede mip1. Zero-only groups are omitted before first/last marks.
            let mut keys = Vec::new();
            for (tap, &weight) in layer.taps.iter().zip(&layer.coefficients) {
                if weight == 0 {
                    continue;
                }
                let key = TileKey {
                    slot: input.slot,
                    n: size,
                    x: (tap[0] >> 3) as u8,
                    y: (tap[1] >> 3) as u8,
                };
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            for key in keys {
                groups.push(Group4 {
                    key,
                    top_left_local: [(layer.taps[0][0] & 7) as u8, (layer.taps[0][1] & 7) as u8],
                    coefficients: std::array::from_fn(|t| {
                        let tap = layer.taps[t];
                        if tap[0] >> 3 == u16::from(key.x) && tap[1] >> 3 == u16::from(key.y) {
                            layer.coefficients[t]
                        } else {
                            0
                        }
                    }),
                    first: false,
                    last: false,
                    quad_id: input.quad_id,
                    lane: lane as u8,
                });
            }
            layers.push(layer);
        }
        groups.first_mut().ok_or("empty texture sample")?.first = true;
        groups.last_mut().unwrap().last = true;
        pixels.push(PixelGolden {
            lane: lane as u8,
            lambda,
            layers,
            groups,
        });
    }
    Ok(PreparedQuad {
        quad_id: input.quad_id,
        lod,
        pixels,
        config,
    })
}

fn split(total: u32, frac: u32, denominator: u32) -> [u32; 2] {
    let high = (u64::from(total) * u64::from(frac) / u64::from(denominator)) as u32;
    [total - high, high]
}
fn prepare_layer(uv: [f64; 2], n: u8, parent: u32, filter: Filter, config: Config) -> LayerGolden {
    let a = 1_u16 << n.max(1);
    let wrapped = uv.map(|v| v.rem_euclid(1.0));
    let p = wrapped.map(|v| v * f64::from(a) - if filter == Filter::Nearest { 0.0 } else { 0.5 });
    let integer = p.map(|v| v.floor() as i64);
    let denominator = 1_u32 << config.coordinate_fraction;
    // U(F,F) fractions use floor, never round a fraction to the next texel.
    let fraction = p.map(|v| ((v - v.floor()) * f64::from(denominator)).floor() as u32);
    let taps = TAPS.map(|[dx, dy]| {
        [
            (integer[0] + i64::from(dx)).rem_euclid(i64::from(a)) as u16,
            (integer[1] + i64::from(dy)).rem_euclid(i64::from(a)) as u16,
        ]
    });
    let coefficients = if filter == Filter::Nearest {
        [parent, 0, 0, 0]
    } else {
        let row = split(parent, fraction[1], denominator);
        let top = split(row[0], fraction[0], denominator);
        let bottom = split(row[1], fraction[0], denominator);
        [top[0], top[1], bottom[0], bottom[1]]
    };
    LayerGolden {
        n,
        parent,
        p,
        integer,
        fraction,
        taps,
        coefficients,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Invalid,
    Filling,
    Ready,
}
#[derive(Clone, Copy, Debug)]
struct Line {
    state: State,
    key: Option<TileKey>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Demand,
    Prefetch,
}
#[derive(Clone, Debug)]
pub enum CacheEvent {
    Hit {
        key: TileKey,
        line: usize,
        access: Access,
    },
    Allocate {
        key: TileKey,
        line: usize,
        victim: Option<TileKey>,
        address: u64,
    },
    Beat {
        line: usize,
        index: usize,
        data: u64,
    },
    Ready {
        key: TileKey,
        line: usize,
    },
    Fault {
        key: TileKey,
        line: usize,
    },
    Invalidate,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub refills: usize,
    pub beats: usize,
}

/// Fixed functional 16-set/4-way, four 1024x16 data banks. No hardware capacity
/// or timing claim: a complete refill/read is one atomic host operation.
pub struct Cache {
    slots: Vec<Slot>,
    lines: [[Line; 4]; 16],
    banks: [[u16; 1024]; 4],
    plru: [u8; 16],
    faulted: bool,
    pub stats: CacheStats,
}
impl Cache {
    pub fn new(slots: Vec<Slot>) -> Result<Self, String> {
        if slots.is_empty() || slots.len() > 16 {
            return Err("texture slot count bound".into());
        }
        for slot in &slots {
            if slot.valid {
                slot.validate()?;
            }
        }
        Ok(Self {
            slots,
            lines: [[Line {
                state: State::Invalid,
                key: None,
            }; 4]; 16],
            banks: [[0; 1024]; 4],
            plru: [0; 16],
            faulted: false,
            stats: CacheStats::default(),
        })
    }
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }
    pub fn lookup(&self, key: TileKey) -> Option<(usize, State)> {
        self.lines[key.set()]
            .iter()
            .enumerate()
            .find(|(_, line)| line.state != State::Invalid && line.key == Some(key))
            .map(|(way, line)| (key.set() * 4 + way, line.state))
    }
    pub fn plru_order(&self, set: usize) -> [usize; 4] {
        let bits = self.plru[set];
        let root = usize::from(bits & 1);
        let left = usize::from((bits >> 1) & 1);
        let right = usize::from((bits >> 2) & 1);
        let halves = [[left, left ^ 1], [2 + right, 2 + (right ^ 1)]];
        [
            halves[root][0],
            halves[root][1],
            halves[root ^ 1][0],
            halves[root ^ 1][1],
        ]
    }
    fn touch(&mut self, set: usize, way: usize) {
        let bits = &mut self.plru[set];
        if way < 2 {
            *bits = (*bits & !3) | 1 | (((way ^ 1) as u8) << 1);
        } else {
            *bits = (*bits & !5) | (((way ^ 1) as u8 & 1) << 2);
        }
    }
    /// Rebind after the synchronous call boundary: there can be no old in-flight
    /// request here. A future asynchronous model must enforce its own barrier.
    pub fn rebind(&mut self, slots: Vec<Slot>) -> Result<CacheEvent, String> {
        if self.faulted {
            return Err("texture service fault is terminal; recreate/drain the composition".into());
        }
        let next = Self::new(slots)?;
        self.slots = next.slots;
        for line in self.lines.iter_mut().flatten() {
            line.state = State::Invalid;
        }
        Ok(CacheEvent::Invalidate)
    }
    fn ensure<M: MemoryPort>(
        &mut self,
        key: TileKey,
        access: Access,
        memory: &mut M,
        events: &mut Vec<CacheEvent>,
    ) -> Result<usize, String> {
        if self.faulted {
            return Err("texture cache has a terminal service fault".into());
        }
        let address = key.address(&self.slots)?;
        if let Some((line, State::Ready)) = self.lookup(key) {
            self.stats.hits += 1;
            self.touch(key.set(), line % 4);
            events.push(CacheEvent::Hit { key, line, access });
            return Ok(line);
        }
        let set = key.set();
        let way = self.lines[set]
            .iter()
            .position(|l| l.state == State::Invalid)
            .or_else(|| {
                self.plru_order(set)
                    .into_iter()
                    .find(|&w| self.lines[set][w].state == State::Ready)
            })
            .ok_or("texture cache has no replaceable way")?;
        let line = set * 4 + way;
        let victim = if self.lines[set][way].state == State::Ready {
            self.lines[set][way].key
        } else {
            None
        };
        self.lines[set][way] = Line {
            state: State::Filling,
            key: Some(key),
        };
        events.push(CacheEvent::Allocate {
            key,
            line,
            victim,
            address,
        });
        self.stats.misses += 1;
        // The returned Vec is an unconstrained oracle sink for the sixteen
        // committed non-backpressurable beats. Not a proposed hardware buffer.
        let response = memory.read_dma(address, TILE_BYTES).and_then(|beats| {
            if beats.len() == 16 {
                Ok(beats)
            } else {
                Err("texture refill requires sixteen real 64-bit beats".into())
            }
        });
        let beats = match response {
            Ok(beats) => beats,
            Err(reason) => {
                // An adapter watchdog can leave accepted service work in flight.
                // Preserve FILLING and stop this composition; never recycle the
                // destination or pretend to have canceled that committed refill.
                self.faulted = true;
                events.push(CacheEvent::Fault { key, line });
                return Err(reason);
            }
        };
        for (index, &data) in beats.iter().enumerate() {
            for texel in 0..4 {
                let linear = index * 4 + texel;
                let (bank, local) = bank_local((linear % 8) as u8, (linear / 8) as u8);
                self.banks[bank][line * 16 + local] = (data >> (16 * texel)) as u16;
            }
            events.push(CacheEvent::Beat { line, index, data });
        }
        self.lines[set][way].state = State::Ready;
        self.touch(set, way);
        self.stats.refills += 1;
        self.stats.beats += 16;
        events.push(CacheEvent::Ready { key, line });
        Ok(line)
    }
    /// An explicit optional hint consumed atomically. Hint FIFO admission/drop,
    /// demand promotion and concurrent dual-tag arbitration belong to timed/emu.
    pub fn prefetch<M: MemoryPort>(
        &mut self,
        key: TileKey,
        memory: &mut M,
    ) -> Result<Vec<CacheEvent>, String> {
        let mut events = Vec::new();
        self.ensure(key, Access::Prefetch, memory, &mut events)?;
        Ok(events)
    }
    pub(crate) fn read_group<M: MemoryPort>(
        &mut self,
        group: &Group4,
        memory: &mut M,
        events: &mut Vec<CacheEvent>,
    ) -> Result<[u16; 4], String> {
        let line = self.ensure(group.key, Access::Demand, memory, events)?;
        // No other allocation interleaves with these four reads: equivalent
        // functional read reservation, without claiming a clocked implementation.
        Ok(TAPS.map(|[dx, dy]| {
            let (bank, local) =
                bank_local(group.top_left_local[0] + dx, group.top_left_local[1] + dy);
            self.banks[bank][line * 16 + local]
        }))
    }
}

#[derive(Clone, Debug)]
pub struct GroupGolden {
    pub group: Group4,
    pub texels: [u16; 4],
    pub expanded: [[u8; 3]; 4],
    pub partial: [u64; 3],
    pub accumulator: [u64; 3],
}
#[derive(Clone, Debug)]
pub struct PixelOutput {
    pub lane: u8,
    pub rgb: [u8; 3],
    pub groups: Vec<GroupGolden>,
}
#[derive(Clone, Debug)]
pub struct Output {
    pub prepared: PreparedQuad,
    pub pixels: Vec<PixelOutput>,
    pub cache_events: Vec<CacheEvent>,
}

pub fn sample<M: MemoryPort>(
    input: &QuadInput,
    cache: &mut Cache,
    memory: &mut M,
    config: Config,
) -> Result<Output, String> {
    let prepared = prepare(input, cache.slots(), config)?;
    let mut pixels = Vec::new();
    let mut cache_events = Vec::new();
    let scale = u64::from(config.coefficient_scale());
    for pixel in &prepared.pixels {
        let mut accumulator = [0_u64; 3];
        let mut groups = Vec::new();
        for group in &pixel.groups {
            let texels = cache.read_group(group, memory, &mut cache_events)?;
            let expanded = texels.map(expand565);
            let partial = std::array::from_fn(|channel| {
                (0..4)
                    .map(|tap| {
                        u64::from(expanded[tap][channel]) * u64::from(group.coefficients[tap])
                    })
                    .sum::<u64>()
            });
            for channel in 0..3 {
                accumulator[channel] += partial[channel];
            }
            if accumulator.iter().any(|&v| v > 255 * scale) {
                return Err("texture accumulator exceeds conserved bound".into());
            }
            groups.push(GroupGolden {
                group: group.clone(),
                texels,
                expanded,
                partial,
                accumulator,
            });
        }
        let rgb = accumulator.map(|v| rne_div(v, scale) as u8);
        pixels.push(PixelOutput {
            lane: pixel.lane,
            rgb,
            groups,
        });
    }
    Ok(Output {
        prepared,
        pixels,
        cache_events,
    })
}
pub fn run<M: MemoryPort>(
    inputs: &[QuadInput],
    cache: &mut Cache,
    memory: &mut M,
    config: Config,
) -> Result<Vec<Output>, String> {
    config.validate()?;
    if inputs.len() > config.max_quads {
        return Err("texture oracle quad budget".into());
    }
    inputs
        .iter()
        .map(|input| sample(input, cache, memory, config))
        .collect()
}

/// Independent continuous reference. No Group4, cache bank, conservative split,
/// quantized UV or mantissa table is reused. Reads the linear RAW565 tile payload.
pub fn reference<M: MemoryPort>(
    input: &QuadInput,
    slots: &[Slot],
    memory: &mut M,
    mip_selection: MipSelection,
) -> Result<Vec<(u8, [f64; 3])>, String> {
    let slot = check_input(input, slots)?;
    let max_lod = if slot.has_full_mip {
        slot.max_size_log2
    } else {
        0
    };
    let mut slope = 0.0_f64;
    for (a, b) in [(0, 1), (2, 3), (0, 2), (1, 3)] {
        for axis in 0..2 {
            slope = slope.max((input.uv[b][axis] - input.uv[a][axis]).abs());
        }
    }
    let lod = clamp_lod(
        (slope * f64::from(1_u16 << slot.max_size_log2)).log2(),
        input.lod_bias,
        max_lod,
    );
    let layer = if input.filter == Filter::Trilinear || mip_selection == MipSelection::Floor {
        lod.floor()
    } else {
        (lod + 0.5).floor()
    } as u8;
    let lambda = if input.filter == Filter::Trilinear && layer < max_lod {
        lod - lod.floor()
    } else {
        0.0
    };
    let mut tiles = BTreeMap::<u64, Vec<u64>>::new();
    let mut output = Vec::new();
    for lane in 0..4 {
        if input.mask & (1 << lane) == 0 {
            continue;
        }
        let mut rgb = [0.0; 3];
        for (level, parent) in [(layer, 1.0 - lambda), (layer.saturating_add(1), lambda)] {
            if parent == 0.0 {
                continue;
            }
            let n = slot.max_size_log2 - level;
            let size = 1_i64 << n.max(1);
            let p = input.uv[lane].map(|v| {
                v.rem_euclid(1.0) * size as f64
                    - if input.filter == Filter::Nearest {
                        0.0
                    } else {
                        0.5
                    }
            });
            let frac = p.map(|v| v - v.floor());
            for tap in 0..if input.filter == Filter::Nearest {
                1
            } else {
                4
            } {
                let dx = tap % 2;
                let dy = tap / 2;
                let x = (p[0].floor() as i64 + dx).rem_euclid(size) as u64;
                let y = (p[1].floor() as i64 + dy).rem_euclid(size) as u64;
                // Independently derive row-major tile address and mip prefix.
                let side = 1_u64 << n.saturating_sub(3);
                let prefix = if slot.has_full_mip {
                    (0..n).map(|j| 1_u64 << (2 * j.saturating_sub(3))).sum()
                } else {
                    0
                };
                let address = u64::from(slot.base_address) + (prefix + y / 8 * side + x / 8) * 128;
                if let std::collections::btree_map::Entry::Vacant(entry) = tiles.entry(address) {
                    let data = memory.read_dma(address, 128)?;
                    if data.len() != 16 {
                        return Err("reference texture missing payload".into());
                    }
                    entry.insert(data);
                }
                let index = (y % 8 * 8 + x % 8) as usize;
                let word = (tiles[&address][index / 4] >> (16 * (index % 4))) as u16;
                // Deliberately independent bit replication for the reference.
                let color = [
                    u32::from(word >> 11) * 8 + u32::from(word >> 13),
                    u32::from((word >> 5) & 63) * 4 + u32::from((word >> 9) & 3),
                    u32::from(word & 31) * 8 + u32::from((word >> 2) & 7),
                ];
                let weight = if input.filter == Filter::Nearest {
                    parent
                } else {
                    parent
                        * if dx == 0 { 1.0 - frac[0] } else { frac[0] }
                        * if dy == 0 { 1.0 - frac[1] } else { frac[1] }
                };
                for c in 0..3 {
                    rgb[c] += weight * f64::from(color[c]);
                }
            }
        }
        output.push((lane as u8, rgb));
    }
    Ok(output)
}
