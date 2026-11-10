//! Experimental frame batches: read-only front/raster, ordered private tiles.
use super::super::{Effects, Memory, State, PLANE_WORDS};
use super::*;
use std::{mem::size_of, time::Instant};

const MAX_DRAWS: usize = 128;
const STRIP_ROWS: usize = 8; // Eight pairs; every 2x2 quad remains indivisible.
const MAX_PAYLOAD: usize = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default)]
pub enum RebinMode {
    TriangleTiles,
    #[default]
    TileScan,
    ParallelSort,
    LocalBuckets,
}

#[derive(Clone, Debug, Default)]
pub struct BatchStats {
    pub attempts: usize,
    pub applied: usize,
    pub fallbacks: usize,
    pub fallback_reason: &'static str,
    pub pool_threads: usize,
    pub collect_ns: u64,
    pub transform_ns: u64,
    pub setup_ns: u64,
    pub raster_ns: u64,
    pub packets_ns: u64,
    pub bin_allocate_merge_ns: u64,
    pub sort_ns: u64,
    pub tile_rop_allocate_ns: u64,
    pub commit_ns: u64,
    pub payload_peak_bytes: usize,
    pub transform_workers: u64,
    pub setup_workers: u64,
    pub raster_workers: u64,
    pub rop_workers: u64,
    /// Per successful batch: draws, triangles, strips, samples, tiles, payload peak.
    pub batches: Vec<[usize; 6]>,
    /// Stops: write/barrier, CB alias, DMA alias, texture alias, draw window, end.
    pub stops: [usize; 6],
    /// Source StateChange, Pixels, state changes whose draw has no Pixels,
    /// and total tile message references after broadcast.
    pub messages: [usize; 4],
    pub raster_samples_by_worker: Vec<usize>,
    pub rop_samples_by_worker: Vec<usize>,
}
impl BatchStats {
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{\"attempts\":{},\"applied\":{},\"fallbacks\":{},",
                "\"fallback_reason\":\"{}\",\"pool_threads\":{},",
                "\"collect_ns\":{},\"transform_ns\":{},\"setup_ns\":{},",
                "\"raster_ns\":{},\"packets_ns\":{},\"bin_allocate_merge_ns\":{},\"sort_ns\":{},",
                "\"tile_rop_allocate_ns\":{},\"commit_ns\":{},",
                "\"payload_peak_bytes\":{},\"worker_masks\":[{},{},{},{}],",
                "\"batches\":{:?},\"stops\":{:?},\"messages\":{:?},",
                "\"raster_samples_by_worker\":{:?},\"rop_samples_by_worker\":{:?}}}"
            ),
            self.attempts,
            self.applied,
            self.fallbacks,
            self.fallback_reason,
            self.pool_threads,
            self.collect_ns,
            self.transform_ns,
            self.setup_ns,
            self.raster_ns,
            self.packets_ns,
            self.bin_allocate_merge_ns,
            self.sort_ns,
            self.tile_rop_allocate_ns,
            self.commit_ns,
            self.payload_peak_bytes,
            self.transform_workers,
            self.setup_workers,
            self.raster_workers,
            self.rop_workers,
            self.batches,
            self.stops,
            self.messages,
            self.raster_samples_by_worker,
            self.rop_samples_by_worker
        )
    }
    fn fail(&mut self, reason: &'static str) -> Attempt {
        self.fallbacks += 1;
        self.fallback_reason = reason;
        Attempt::Fallback
    }
}
fn ns(start: Instant) -> u64 {
    start.elapsed().as_nanos() as u64
}
fn worker() -> u64 {
    1u64.checked_shl(rayon::current_thread_index().unwrap_or(0) as u32)
        .unwrap_or(0)
}

#[derive(Clone)]
pub(in crate::system::functional) struct DrawInput {
    pub mesh: Meshlet,
    pub uniform: [u32; 32],
    pub state: shading::DrawState,
    pub pc: u32,
}
pub(in crate::system::functional) enum Attempt {
    Applied(Box<State>, usize),
    Fallback,
    Skip,
}

struct Transformed {
    vertices: Vec<Vertex>,
    worker: u64,
}
struct Setup {
    triangle: Option<Triangle>,
    spans: [Spans; 120],
    rows: std::ops::Range<usize>,
    quads: usize,
    draw: usize,
    worker: u64,
}
#[derive(Clone, Copy)]
struct Strip {
    triangle: usize,
    first: usize,
    end: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sample {
    x: u16,
    y: u16,
    depth: u16,
    draw: u16,
    color: [u8; 4],
}
struct Raster {
    pixels: Vec<Sample>,
    work: usize,
    worker: u64,
    draw: usize,
    triangle: usize,
}

struct Piece {
    tile: usize,
    pixels: Vec<Sample>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MessageKind {
    // A DRAW binds its selected Instance and current Render state. Global,
    // projection, transforms and texture state are already baked into Pixels.
    StateChange { pc: u32, state: u16 },
    Pixels { pc: u32, triangle: u32, piece: u32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TileMessage {
    order: u32,
    kind: MessageKind,
}
impl TileMessage {
    fn belongs(self, tile: usize, pieces: &[Piece]) -> bool {
        match self.kind {
            MessageKind::StateChange { .. } => true,
            MessageKind::Pixels { piece, .. } => pieces[piece as usize].tile == tile,
        }
    }
}

fn overlaps(start: usize, end: usize, target: [usize; 2]) -> bool {
    target
        .into_iter()
        .any(|base| start < base + PLANE_WORDS && base < end)
}

pub(in crate::system::functional) fn try_execute(
    memory: &mut Memory<'_>,
    state: &State,
    base: usize,
    window: std::ops::Range<usize>,
    maximum: usize,
    rebin_mode: RebinMode,
    stats: &mut BatchStats,
) -> Attempt {
    let Some(target) = state.target else {
        return Attempt::Skip;
    };
    stats.attempts += 1;
    let start = Instant::now();
    let mut next = state.clone();
    next.pending = Some(Vec::new());
    let mut pc = window.start;
    while pc < window.end {
        if next.pending.as_ref().unwrap().len() == MAX_DRAWS {
            stats.stops[4] += 1;
            break;
        }
        let at = base + pc * 2;
        if overlaps(at, at + 2, target) {
            stats.stops[1] += 1;
            break;
        }
        let instruction = match memory.read(at..at + 2) {
            Ok(words) => u32::from(words[0]) | (u32::from(words[1]) << 16),
            Err(_) => {
                stats.collect_ns += ns(start);
                return stats.fail("command-read");
            }
        };
        let opcode = instruction >> 28;
        if !matches!(opcode, 0..=7 | 9) {
            stats.stops[if opcode == 15 { 5 } else { 0 }] += 1;
            break;
        }
        let source = if opcode == 1 {
            let source = ((instruction >> 10) & 0x3ffff) as usize * 16;
            Some((
                source,
                source + [16, 32, 64, 0][((instruction >> 1) & 3) as usize],
            ))
        } else if opcode == 2 {
            let source = ((instruction >> 12) & 65535) as usize * 64;
            Some((
                source,
                source + 64 + (((instruction >> 1) & 7) as usize + 1) * 64,
            ))
        } else {
            None
        };
        if source.is_some_and(|(lo, hi)| overlaps(lo, hi, target)) {
            stats.stops[2] += 1;
            break;
        }
        if opcode == 3 {
            if let (Some(mesh), Some(render)) =
                (&next.pages[(instruction & 1) as usize], next.render)
            {
                if !mesh.rgb && render.texture.overlaps_target(target) {
                    stats.stops[3] += 1;
                    break;
                }
            }
        }
        // This whitelist never calls Memory::write/store/copy or SET_TARGET.
        // State changes stay private. Reads are disjoint from all pending writes.
        if next.command(memory, instruction, maximum).is_err() {
            stats.collect_ns += ns(start);
            return stats.fail("frontend");
        }
        if opcode == 3 {
            next.pending.as_mut().unwrap().last_mut().unwrap().pc = (pc * 4) as u32;
        }
        pc += 1;
    }
    stats.collect_ns += ns(start);
    let draws = next.pending.take().unwrap();
    if draws.len() < 2 {
        return Attempt::Skip;
    }
    stats.pool_threads = rayon::current_num_threads();
    stats.raster_samples_by_worker.resize(stats.pool_threads, 0);
    stats.rop_samples_by_worker.resize(stats.pool_threads, 0);
    let sources = draws.capacity() * size_of::<DrawInput>()
        + size_of::<State>()
        + draws
            .iter()
            .map(|d| {
                d.mesh.vertices.capacity() * size_of::<[u32; 3]>()
                    + d.mesh.triangles.capacity() * size_of::<[usize; 3]>()
            })
            .sum::<usize>();
    let mut peak = sources;

    let start = Instant::now();
    let transformed: Vec<_> = draws
        .par_iter()
        .map(|d| {
            d.mesh.transform(&d.uniform).map(|vertices| Transformed {
                vertices,
                worker: worker(),
            })
        })
        .collect();
    stats.transform_ns += ns(start);
    if transformed.iter().any(Result::is_err) {
        return stats.fail("transform");
    }
    let vertices_bytes = transformed.capacity() * size_of::<Result<Transformed, String>>()
        + transformed
            .iter()
            .map(|t| t.as_ref().unwrap().vertices.capacity() * size_of::<Vertex>())
            .sum::<usize>();
    for t in &transformed {
        stats.transform_workers |= t.as_ref().unwrap().worker;
    }
    let indices: Vec<_> = draws
        .iter()
        .enumerate()
        .flat_map(|(draw, d)| d.mesh.triangles.iter().map(move |&indices| (draw, indices)))
        .collect();
    let start = Instant::now();
    let setups: Vec<Result<Setup, String>> = indices
        .par_iter()
        .map(|&(draw, indices)| {
            let vertices = &transformed[draw].as_ref().unwrap().vertices;
            let triangle = Triangle::new(indices.map(|i| vertices[i]), draws[draw].mesh.cull)?;
            let rows = triangle.as_ref().and_then(Triangle::rows).unwrap_or(0..0);
            let spans = std::array::from_fn(|row| {
                if rows.contains(&row) {
                    [
                        triangle.as_ref().unwrap().span(row * 2),
                        triangle.as_ref().unwrap().span(row * 2 + 1),
                    ]
                } else {
                    [None, None]
                }
            });
            let quads = spans
                .iter()
                .filter_map(|&s| span_range(s))
                .map(|r| (r.end() - r.start()) / 2 + 1)
                .sum();
            Ok(Setup {
                triangle,
                spans,
                rows,
                quads,
                draw,
                worker: worker(),
            })
        })
        .collect();
    stats.setup_ns += ns(start);
    let setup_bytes = setups.capacity() * size_of::<Result<Setup, String>>()
        + indices.capacity() * size_of::<(usize, [usize; 3])>();
    peak = peak.max(sources + vertices_bytes + setup_bytes);
    stats.payload_peak_bytes = stats.payload_peak_bytes.max(peak);
    if setups.iter().any(Result::is_err) {
        return stats.fail("triangle-setup");
    }
    let vertex_work: usize = draws.iter().map(|d| d.mesh.vertices.len()).sum();
    let setup_work: usize = setups
        .iter()
        .map(|s| {
            1 + if s.as_ref().unwrap().triangle.is_some() {
                240
            } else {
                0
            }
        })
        .sum();
    let quads: usize = setups.iter().map(|s| s.as_ref().unwrap().quads).sum();
    let Some(upper) = quads
        .checked_mul(40)
        .and_then(|work| work.checked_add(vertex_work + setup_work))
    else {
        return stats.fail("work-overflow");
    };
    if upper > maximum.saturating_sub(next.report.work) {
        return stats.fail("work-budget");
    }
    if quads
        .checked_mul(4 * size_of::<Sample>() * 3)
        .and_then(|bytes| bytes.checked_add(sources + setup_bytes))
        .is_none_or(|bytes| bytes > MAX_PAYLOAD)
    {
        return stats.fail("workspace-limit");
    }
    for setup in &setups {
        stats.setup_workers |= setup.as_ref().unwrap().worker;
    }
    drop(transformed);
    let strips: Vec<_> = setups
        .iter()
        .enumerate()
        .flat_map(|(triangle, s)| {
            let s = s.as_ref().unwrap();
            (s.rows.start..s.rows.end)
                .step_by(STRIP_ROWS)
                .map(move |first| Strip {
                    triangle,
                    first,
                    end: (first + STRIP_ROWS).min(s.rows.end),
                })
        })
        .collect();
    let start = Instant::now();
    let raster: Vec<Result<Raster, String>> = strips
        .par_iter()
        .map(|strip| {
            let setup = setups[strip.triangle].as_ref().unwrap();
            let triangle = setup.triangle.as_ref().unwrap();
            let draw = &draws[setup.draw];
            let quads: usize = setup.spans[strip.first..strip.end]
                .iter()
                .filter_map(|&s| span_range(s))
                .map(|r| (r.end() - r.start()) / 2 + 1)
                .sum();
            let mut pixels = Vec::with_capacity(quads * 4);
            let mut work = 0;
            for row in strip.first..strip.end {
                let spans = setup.spans[row];
                if let Some(range) = span_range(spans) {
                    for x in range.step_by(2) {
                        work += 4;
                        let quad = Quad::new(x, row * 2, spans);
                        let prepared = prepare_quad(memory, triangle, quad, draw.state)?;
                        for (lane, fragment) in prepared.into_iter().enumerate() {
                            if let Some(fragment) = fragment {
                                let (charge, color, depth) = fragment.finish()?;
                                work += charge;
                                let [x, y] = quad.xy(lane);
                                pixels.push(Sample {
                                    x: x as u16,
                                    y: y as u16,
                                    depth,
                                    draw: setup.draw as u16,
                                    color,
                                });
                            }
                        }
                    }
                }
            }
            Ok(Raster {
                pixels,
                work,
                worker: worker(),
                draw: setup.draw,
                triangle: strip.triangle,
            })
        })
        .collect();
    stats.raster_ns += ns(start);
    if raster.iter().any(Result::is_err) {
        return stats.fail("raster-shade");
    }
    #[cfg(test)]
    if let Ok(path) = std::env::var("GPU_SIM_STAGE_DIAGNOSTIC_OUT") {
        tests::stage_isolation(&path, memory, &draws, &setups, &strips, &raster);
    }
    let raster_records = raster.capacity() * size_of::<Result<Raster, String>>();
    let mut remaining_bytes: usize = raster
        .iter()
        .map(|r| r.as_ref().unwrap().pixels.capacity() * size_of::<Sample>())
        .sum();
    peak = peak.max(
        sources
            + setup_bytes
            + strips.capacity() * size_of::<Strip>()
            + raster_records
            + remaining_bytes,
    );
    stats.payload_peak_bytes = stats.payload_peak_bytes.max(peak);
    let triangle_count = setups.len();
    drop(setups);
    drop(indices);
    let strip_count = strips.len();
    drop(strips);
    let start = Instant::now();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut pieces_bytes = 0;
    let mut source: Vec<TileMessage> = Vec::new();
    let mut groups = Vec::new();
    let mut samples = 0;
    let mut work = vertex_work + setup_work;
    let mut raster = raster.into_iter().map(Result::unwrap).peekable();
    for (draw, input) in draws.iter().enumerate() {
        let before_pieces = pieces.len();
        let first = source.len();
        source.push(TileMessage {
            order: first as u32,
            kind: MessageKind::StateChange {
                pc: input.pc,
                state: draw as u16,
            },
        });
        groups.push(first..source.len()); // Retain even a draw with no pixels.
        let mut triangle = None;
        let mut group_start = source.len();
        while raster.peek().is_some_and(|r| r.draw == draw) {
            let row = raster.next().unwrap();
            if triangle != Some(row.triangle) {
                if group_start < source.len() {
                    groups.push(group_start..source.len());
                }
                group_start = source.len();
                triangle = Some(row.triangle);
            }
            stats.raster_workers |= row.worker;
            if let Some(count) = stats
                .raster_samples_by_worker
                .get_mut(row.worker.trailing_zeros() as usize)
            {
                *count += row.pixels.len();
            }
            work += row.work;
            let mut slots = [usize::MAX; 400];
            let mut row_pieces: Vec<Piece> = Vec::new();
            let current_bytes = row.pixels.capacity() * size_of::<Sample>();
            remaining_bytes -= current_bytes;
            for pixel in row.pixels {
                let tile = (usize::from(pixel.x) >> 4) * 16 + (usize::from(pixel.y) >> 4);
                if slots[tile] == usize::MAX {
                    slots[tile] = row_pieces.len();
                    row_pieces.push(Piece {
                        tile,
                        pixels: Vec::new(),
                    });
                }
                row_pieces[slots[tile]].pixels.push(pixel);
                samples += 1;
            }
            for piece in row_pieces {
                let id = pieces.len();
                let old_capacity = pieces.capacity();
                pieces_bytes += piece.pixels.capacity() * size_of::<Sample>();
                pieces.push(piece);
                pieces_bytes += (pieces.capacity() - old_capacity) * size_of::<Piece>();
                source.push(TileMessage {
                    order: source.len() as u32,
                    kind: MessageKind::Pixels {
                        pc: input.pc,
                        triangle: row.triangle as u32,
                        piece: id as u32,
                    },
                });
            }
            let piece_bytes = pieces_bytes;
            peak = peak.max(
                sources
                    + raster_records
                    + remaining_bytes
                    + current_bytes
                    + piece_bytes
                    + source.capacity() * size_of::<TileMessage>(),
            );
        }
        if group_start < source.len() {
            groups.push(group_start..source.len());
        }
        if before_pieces == pieces.len() {
            stats.messages[2] += 1;
        }
    }
    stats.packets_ns += ns(start);
    if next.report.charge(work, maximum).is_err() {
        return stats.fail("actual-work");
    }
    let prepared_bytes = pieces.capacity() * size_of::<Piece>()
        + pieces
            .iter()
            .map(|p| p.pixels.capacity() * size_of::<Sample>())
            .sum::<usize>()
        + source.capacity() * size_of::<TileMessage>()
        + groups.capacity() * size_of::<std::ops::Range<usize>>();
    #[cfg(test)]
    if let Ok(path) = std::env::var("GPU_SIM_SHARED_REBIN_OUT") {
        tests::compare_shared_rebin(&path, &source, &groups, &pieces);
    }
    let rebin = rebin(&source, &groups, &pieces, rebin_mode);
    stats.messages[0] += draws.len();
    stats.messages[1] += pieces.len();
    stats.messages[3] += rebin.bins.iter().map(Vec::len).sum::<usize>();
    stats.bin_allocate_merge_ns += rebin.rebin_ns;
    stats.sort_ns += rebin.sort_ns;
    peak = peak.max(sources + prepared_bytes + rebin.peak_bytes);
    let bins = rebin.bins;
    let bins_bytes = bins.capacity() * size_of::<Vec<TileMessage>>()
        + bins
            .iter()
            .map(|b| b.capacity() * size_of::<TileMessage>())
            .sum::<usize>();
    let start = Instant::now();
    let tiles: Vec<Result<Tile, String>> = bins
        .par_iter()
        .enumerate()
        .map(|(index, bin)| {
            let mut tile = Tile::new(memory, target, index);
            let mut active = None;
            for message in bin {
                match message.kind {
                    MessageKind::StateChange { state, .. } => active = Some(usize::from(state)),
                    MessageKind::Pixels { piece, .. } => {
                        let state = active.ok_or("Pixels before initial StateChange")?;
                        for pixel in &pieces[piece as usize].pixels {
                            if usize::from(pixel.draw) != state {
                                return Err("tile state ordering invariant".into());
                            }
                            shading::rop(
                                &mut tile,
                                [usize::from(pixel.x), usize::from(pixel.y)],
                                pixel.depth,
                                pixel.color,
                                &draws[state].state,
                            )?;
                            tile.samples += 1;
                        }
                    }
                }
            }
            Ok(tile)
        })
        .collect();
    stats.tile_rop_allocate_ns += ns(start);
    peak = peak.max(
        sources
            + prepared_bytes
            + bins_bytes
            + tiles.capacity() * size_of::<Result<Tile, String>>(),
    );
    stats.payload_peak_bytes = stats.payload_peak_bytes.max(peak);
    if tiles.iter().any(Result::is_err) {
        return stats.fail("tile-rop");
    }
    let tile_count = tiles.len();
    let start = Instant::now();
    for tile in tiles {
        let tile = tile.unwrap();
        if tile.samples > 0 {
            stats.rop_workers |= tile.worker;
        }
        if let Some(count) = stats
            .rop_samples_by_worker
            .get_mut(tile.worker.trailing_zeros() as usize)
        {
            *count += tile.samples;
        }
        for (plane, base) in tile.base.into_iter().enumerate() {
            memory.words[base..base + 256]
                .copy_from_slice(&tile.words[plane * 256..plane * 256 + 256]);
            if let (Some(access), Some(flags)) = (&mut memory.access, &tile.flags) {
                access[base * 2..base * 2 + 512]
                    .copy_from_slice(&flags[plane * 512..plane * 512 + 512]);
            }
        }
        merge_writes(&mut memory.effects, tile.effects.written_bytes);
    }
    stats.commit_ns += ns(start);
    stats.applied += 1;
    stats.batches.push([
        draws.len(),
        triangle_count,
        strip_count,
        samples,
        tile_count,
        peak,
    ]);
    Attempt::Applied(Box::new(next), pc)
}

fn merge_writes(effects: &mut Effects, range: Option<(u32, u32)>) {
    if let Some((first, end)) = range {
        effects.written_bytes = Some(match effects.written_bytes {
            Some((lo, hi)) => (lo.min(first), hi.max(end)),
            None => (first, end),
        });
    }
}

struct Rebinned {
    bins: Vec<Vec<TileMessage>>,
    rebin_ns: u64,
    sort_ns: u64,
    peak_bytes: usize,
}
fn bin_bytes(bins: &[Vec<TileMessage>]) -> usize {
    std::mem::size_of_val(bins)
        + bins
            .iter()
            .map(|b| b.capacity() * size_of::<TileMessage>())
            .sum::<usize>()
}
fn rebin(
    source: &[TileMessage],
    groups: &[std::ops::Range<usize>],
    pieces: &[Piece],
    mode: RebinMode,
) -> Rebinned {
    let start = Instant::now();
    let mut extra = 0;
    let mut bins: Vec<Vec<TileMessage>> = match mode {
        RebinMode::TriangleTiles => {
            let mut bins: Vec<Vec<TileMessage>> = (0..400).map(|_| Vec::new()).collect();
            for group in groups {
                bins.par_iter_mut().enumerate().for_each(|(tile, bin)| {
                    bin.extend(
                        source[group.clone()]
                            .iter()
                            .copied()
                            .filter(|m| m.belongs(tile, pieces)),
                    )
                });
            }
            bins
        }
        RebinMode::TileScan => (0..400)
            .into_par_iter()
            .map(|tile| {
                source
                    .iter()
                    .copied()
                    .filter(|m| m.belongs(tile, pieces))
                    .collect()
            })
            .collect(),
        RebinMode::ParallelSort => {
            use std::sync::Mutex;
            let bins: Vec<_> = (0..400).map(|_| Mutex::new(Vec::new())).collect();
            groups.par_iter().for_each(|group| {
                source[group.clone()]
                    .par_iter()
                    .for_each(|&message| match message.kind {
                        MessageKind::StateChange { .. } => {
                            for bin in &bins {
                                bin.lock().unwrap().push(message);
                            }
                        }
                        MessageKind::Pixels { piece, .. } => bins[pieces[piece as usize].tile]
                            .lock()
                            .unwrap()
                            .push(message),
                    })
            });
            extra = 400 * size_of::<Mutex<Vec<TileMessage>>>();
            bins.into_iter().map(|b| b.into_inner().unwrap()).collect()
        }
        RebinMode::LocalBuckets => {
            let local: Vec<Vec<Vec<TileMessage>>> = source
                .par_iter()
                .with_min_len(32)
                .fold(
                    || {
                        (0..400)
                            .map(|_| Vec::new())
                            .collect::<Vec<Vec<TileMessage>>>()
                    },
                    |mut bins, &message| {
                        match message.kind {
                            MessageKind::StateChange { .. } => {
                                for bin in &mut bins {
                                    bin.push(message);
                                }
                            }
                            MessageKind::Pixels { piece, .. } => {
                                bins[pieces[piece as usize].tile].push(message)
                            }
                        }
                        bins
                    },
                )
                .collect();
            extra = local.capacity() * size_of::<Vec<Vec<TileMessage>>>()
                + local.iter().map(|b| bin_bytes(b)).sum::<usize>();
            (0..400)
                .into_par_iter()
                .map(|tile| {
                    let mut bin = Vec::new();
                    for local in &local {
                        bin.extend_from_slice(&local[tile]);
                    }
                    bin
                })
                .collect()
        }
    };
    let rebin_ns = ns(start);
    let start = Instant::now();
    if matches!(mode, RebinMode::ParallelSort | RebinMode::LocalBuckets) {
        bins.par_iter_mut()
            .for_each(|bin| bin.sort_unstable_by_key(|m| m.order));
    }
    let sort_ns = ns(start);
    let peak_bytes = bin_bytes(&bins) + extra;
    Rebinned {
        bins,
        rebin_ns,
        sort_ns,
        peak_bytes,
    }
}

struct Tile {
    base: [usize; 2],
    words: [u16; 512],
    flags: Option<[u8; 1024]>,
    effects: Effects,
    worker: u64,
    samples: usize,
}
impl Tile {
    fn new(memory: &Memory<'_>, target: [usize; 2], index: usize) -> Self {
        let base = target.map(|b| b + index * 256);
        let mut tile = Self {
            base,
            words: [0; 512],
            flags: memory.access.as_ref().map(|_| [0; 1024]),
            effects: Effects::default(),
            worker: worker(),
            samples: 0,
        };
        // Copying a preimage is not a GPU read: unknown untouched bytes remain
        // unknown. Permissions are checked only by the original ROP operations.
        for (plane, base) in base.into_iter().enumerate() {
            tile.words[plane * 256..plane * 256 + 256]
                .copy_from_slice(&memory.words[base..base + 256]);
            if let (Some(flags), Some(access)) = (&mut tile.flags, &memory.access) {
                flags[plane * 512..plane * 512 + 512]
                    .copy_from_slice(&access[base * 2..base * 2 + 512]);
            }
        }
        tile
    }
    fn slot(&self, address: usize) -> Result<usize, String> {
        self.base
            .iter()
            .enumerate()
            .find_map(|(plane, &base)| {
                (base..base + 256)
                    .contains(&address)
                    .then(|| plane * 256 + address - base)
            })
            .ok_or_else(|| "tile ownership invariant".into())
    }
}
impl shading::PixelMemory for Tile {
    fn load(&self, address: usize) -> Result<u16, String> {
        let slot = self.slot(address)?;
        if let Some(flags) = &self.flags {
            if let Some(i) = flags[slot * 2..slot * 2 + 2]
                .iter()
                .position(|f| f & 1 == 0)
            {
                return Err(format!("read of uncaptured byte {:#x}", address * 2 + i));
            }
        }
        Ok(self.words[slot])
    }
    fn store(&mut self, address: usize, value: u16) -> Result<(), String> {
        let slot = self.slot(address)?;
        if let Some(flags) = &mut self.flags {
            let flags = &mut flags[slot * 2..slot * 2 + 2];
            if let Some(i) = flags.iter().position(|f| f & 2 == 0) {
                return Err(format!(
                    "write outside captured writable bytes at {:#x}",
                    address * 2 + i
                ));
            }
            for f in flags {
                *f |= 1;
            }
        }
        self.words[slot] = value;
        merge_writes(
            &mut self.effects,
            Some(((address * 2) as u32, (address * 2 + 2) as u32)),
        );
        Ok(())
    }
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
