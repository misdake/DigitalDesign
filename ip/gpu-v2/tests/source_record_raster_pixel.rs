//! Actual source snapshot -> atomic numeric rows -> shared-MC complete image.
use digital_design_hardware_gowin::sdram_memory_controller::ports::OracleImage;
use gpu_v2::{
    framebuffer::ports as fb,
    frontend::{ports::Slot, source_capture as source},
    geometry::{record_transport as rec, source_record_link as link},
    lighting::{ports as light, sim::oracle as light_oracle},
    memory::ports::MemoryPort,
    system::pixel::*,
    texture::{ports as tex, sim::oracle as texture_oracle},
    triangle::{ports as tri, sim::oracle},
    vertex::ports::Transformed,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
#[allow(dead_code)]
#[path = "support/pixel.rs"]
mod pixel;
#[allow(dead_code, unused_imports)]
#[path = "support/record_raster.rs"]
mod rr;
#[allow(dead_code)]
#[path = "support/sdram/shared_pixel.rs"]
mod shared;
#[allow(dead_code)]
#[path = "support/texture.rs"]
mod texture;

const CONTEXT: u64 = 0x1234_5678_9abc_def0;
const TEX_BASE: u32 = 0x10000;
const BG_BASE: u64 = 0x20000;
#[path = "support/source_record_raster_producer.rs"]
mod producer;
fn artifacts() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-pixel-branches/source-render-integration");
    std::fs::create_dir_all(&path).unwrap();
    path
}
fn input(id: u32, points: [[f64; 2]; 3], color: [u16; 3], normal: [i16; 3]) -> tri::Input {
    tri::Input {
        id,
        vertices: std::array::from_fn(|i| Transformed {
            clip: [
                ((points[i][0] / 16.0 - 1.0) * 65536.0).round_ties_even() as i32,
                ((1.0 - points[i][1] / 16.0) * 65536.0).round_ties_even() as i32,
                0,
                65536,
            ],
            normal,
            uv: [[64, 96], [4095, 96], [64, 4095]][i],
            rgb565: color[i],
        }),
    }
}
fn attributes() -> rr::Attributes {
    let c = rr::profile();
    rr::Attributes {
        near: c.near_raw,
        far: c.far_raw,
        slot: 0,
        size: 5,
        filter: tex::Filter::Trilinear,
        default_light: false,
        default_sample: false,
    }
}
fn lighting() -> light::LightingContext {
    light::LightingContext {
        material: light::Material {
            shininess_code: 8,
            specular_color: [17, 93, 203],
            unlit: false,
        },
        light: light::Light {
            direction: [0, 0, 16384],
            ambient: 43,
            directional: 217,
        },
        projection: light::Projection::default(),
        epoch: 19,
    }
}
fn context() -> Context {
    let mut c = pixel::context();
    c.surface.width = 32;
    c.rop.depth = fb::DepthFunc::Always;
    c
}
fn slot() -> tex::Slot {
    let mut s = texture::slot(5, true);
    s.base_address = TEX_BASE;
    s
}
struct Fixture {
    initial: Vec<u8>,
    asset: Vec<u8>,
    slot: tex::Slot,
}
impl Fixture {
    fn new() -> Self {
        let slot = slot();
        let asset = texture::asset(slot, texture::pattern);
        let mut initial: Vec<_> = (0..0x21000)
            .map(|i| ((i * 13) ^ (i >> 6) ^ 0xa7) as u8)
            .collect();
        initial[..24576].copy_from_slice(&pixel::image());
        initial[TEX_BASE as usize..TEX_BASE as usize + asset.len()].copy_from_slice(&asset);
        assert!(asset.len().is_multiple_of(128));
        Self {
            initial,
            asset,
            slot,
        }
    }
    fn shared(&self) -> shared::Shared {
        // SAFETY: independent external initial memory stimulus, not computed results.
        let image = unsafe {
            OracleImage::from_host(0, self.initial.clone(), "record raster external image").unwrap()
        };
        shared::Shared::new(
            image,
            (
                u64::from(TEX_BASE),
                u64::from(TEX_BASE) + self.asset.len() as u64,
            ),
            BG_BASE,
            true,
            rr::MAX_WALL,
        )
        .unwrap()
    }
}
struct GoldenMemory<'a>(&'a [u8]);
impl gpu_v2::frontend::ports::MemoryPort for GoldenMemory<'_> {
    fn read_dma(&mut self, address: u64, bytes: usize) -> Result<Vec<u64>, String> {
        if bytes != 128 || !address.is_multiple_of(128) {
            return Err("golden sector".into());
        }
        let offset = address
            .checked_sub(u64::from(TEX_BASE))
            .ok_or("golden texture address")? as usize;
        let data = self
            .0
            .get(offset..offset + bytes)
            .ok_or("golden asset bounds")?;
        Ok(data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| u64::from_le_bytes(*b))
            .collect())
    }
}
// Independent original-vertex orientation; no decoded edge recurrence or contains().
fn covered(v: [[i64; 2]; 3], x: u16, y: u16) -> bool {
    let s = [i128::from(x) * 16 + 8, i128::from(y) * 16 + 8];
    (0..3).all(|i| {
        let p = v[i];
        let q = v[(i + 1) % 3];
        let cross = i128::from(q[0] - p[0]) * (s[1] - i128::from(p[1]))
            - i128::from(q[1] - p[1]) * (s[0] - i128::from(p[0]));
        cross > 0 || cross == 0 && (q[1] < p[1] || q[1] == p[1] && q[0] > p[0])
    })
}
fn reference_quad(r: &oracle::Report, fan: usize, x: u16, y: u16) -> Option<BranchQuad> {
    let mut mask = 0;
    for lane in 0..4 {
        if covered(
            r.triangles[fan].vertices,
            x + (lane % 2) as u16,
            y + (lane / 2) as u16,
        ) {
            mask |= 1 << lane;
        }
    }
    if mask == 0 {
        return None;
    }
    let samples: Vec<_> = (0..4)
        .map(|lane| {
            r.reference([
                f64::from(x) + 0.5 + (lane % 2) as f64,
                f64::from(y) + 0.5 + (lane / 2) as f64,
            ])
            .unwrap()
        })
        .collect();
    let mut q = BranchQuad {
        live: LiveQuad {
            quad: QuadInput {
                header: fb::Header {
                    x,
                    y: y as u8,
                    mask,
                },
                basic: [Basic::default(); 4],
                default_light: false,
                default_sample: false,
            },
            light: [light::PixelInput {
                normal: [0; 3],
                ndc: [0; 2],
            }; 4],
        },
        sample: Some(tex::QuadInput {
            quad_id: 0,
            mask,
            uv: [[0.; 2]; 4],
            slot: 0,
            material_size_log2: 5,
            filter: tex::Filter::Trilinear,
            lod_bias: 0.,
        }),
    };
    for (lane, s) in samples.iter().enumerate() {
        q.sample.as_mut().unwrap().uv[lane] = s.quantized.uv.map(|v| v as f64 / 262144.);
        if mask >> lane & 1 != 0 {
            q.live.quad.basic[lane] = Basic {
                tint: s
                    .rgb
                    .map(|v| (v.clamp(0., 1.) * 255.).round_ties_even() as u8),
                depth: s.quantized.depth,
            };
            let px = i32::from(x) + (lane % 2) as i32;
            let py = i32::from(y) + (lane / 2) as i32;
            q.live.light[lane] = light::PixelInput {
                normal: s.quantized.normal.map(|v| i16::try_from(v).unwrap()),
                ndc: [px * 4096 + 2048 - 65536, 65536 - py * 4096 - 2048],
            };
        }
    }
    Some(q)
}
// Checker-only vectors. They never drive the consumer or PixelBranches offers.
type GoldenFrame = (Vec<(u64, BranchQuad)>, Vec<pixel::Stimulus>, Vec<u8>, bool);
fn golden(reports: &[oracle::Report], f: &Fixture) -> GoldenFrame {
    let mut quads = Vec::new();
    let mut stimuli = Vec::new();
    let mut generation = 0;
    let mut cache = texture_oracle::Cache::new(vec![f.slot]).unwrap();
    let mut memory = GoldenMemory(&f.asset);
    let light = lighting();
    let mut helper_effect = false;
    for r in reports {
        for fan in 0..r.triangles.len() {
            generation += 1;
            for ty in 0..2 {
                for tx in 0..2 {
                    for y in (ty * 16..ty * 16 + 16).step_by(2) {
                        for x in (tx * 16..tx * 16 + 16).step_by(2) {
                            let Some(q) = reference_quad(r, fan, x, y) else {
                                continue;
                            };
                            let sampled = texture_oracle::sample(
                                q.sample.as_ref().unwrap(),
                                &mut cache,
                                &mut memory,
                                tex::Config::counted(),
                            )
                            .unwrap();
                            let mut colors = [[255; 3]; 4];
                            for p in &sampled.pixels {
                                colors[p.lane as usize] = p.rgb;
                            }
                            if q.live.quad.header.mask != 15 {
                                let mut bad = q.sample.clone().unwrap();
                                let first = (0..4).find(|&i| bad.mask >> i & 1 != 0).unwrap();
                                for i in 0..4 {
                                    if bad.mask >> i & 1 == 0 {
                                        bad.uv[i] = bad.uv[first];
                                    }
                                }
                                let mut bad_cache =
                                    texture_oracle::Cache::new(vec![f.slot]).unwrap();
                                let changed = texture_oracle::sample(
                                    &bad,
                                    &mut bad_cache,
                                    &mut memory,
                                    tex::Config::counted(),
                                )
                                .unwrap();
                                helper_effect |= sampled
                                    .pixels
                                    .iter()
                                    .zip(changed.pixels)
                                    .any(|(a, b)| a.rgb != b.rgb);
                            }
                            let lights = std::array::from_fn(|lane| {
                                if q.live.quad.header.mask >> lane & 1 == 0 {
                                    return light::LightingOutput { g: 0, h: 0 };
                                }
                                let o = light_oracle::evaluate(
                                    q.live.light[lane],
                                    light.material,
                                    light.light,
                                    light.projection,
                                    light_oracle::Config {
                                        rounding: light_oracle::RoundingPolicy {
                                            power: light_oracle::Rounding::Floor,
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    },
                                )
                                .unwrap();
                                light::LightingOutput {
                                    g: o.g as u16,
                                    h: o.h as u16,
                                }
                            });
                            stimuli.push(pixel::Stimulus {
                                quad: q.live.quad,
                                light: lights,
                                sample: colors,
                            });
                            quads.push((generation, q));
                        }
                    }
                }
            }
        }
    }
    let expected = pixel::golden(f.initial.clone(), &stimuli, context());
    (quads, stimuli, expected, helper_effect)
}
fn compare_offer(actual: &BranchQuad, expected: &BranchQuad) {
    assert_eq!(
        actual.live.quad, expected.live.quad,
        "independent Basic/header final-code boundary"
    );
    for lane in 0..4 {
        assert_eq!(
            actual.live.light[lane].normal,
            expected.live.light[lane].normal
        );
        assert_eq!(actual.live.light[lane].ndc, expected.live.light[lane].ndc);
    }
    assert_eq!(
        actual.sample.as_ref().unwrap().uv,
        expected.sample.as_ref().unwrap().uv,
        "independent helper Q18 codes"
    );
}

// Immutable external stimulus/checker data. No future Report reaches Producer.
fn corpus() -> Vec<tri::Input> {
    let mut clipped = input(
        7,
        [[16., 16.], [-130., -50.], [190., -50.]],
        [0xffff; 3],
        [0, 0, 16384],
    );
    for vertex in &mut clipped.vertices {
        vertex.uv = [2048, 1024];
    }
    vec![
        clipped,
        input(
            7,
            [[4., 4.], [20., 4.], [4., 20.]],
            [0xf800; 3],
            [0, 0, 16384],
        ),
        input(
            7,
            [[4.1, 4.1], [5.7, 4.1], [4.1, 5.7]],
            [0x07e0; 3],
            [-8192, 4096, 12288],
        ),
        input(
            7,
            [[-130., 6.], [5., 6.], [5., 25.]],
            [0x1234, 0xabcd, 0xffff],
            [8192, -4096, 16384],
        ),
        input(
            7,
            [[12., 12.], [18., 12.], [12., 18.]],
            [0x001f; 3],
            [4096, 8192, 12288],
        ),
        input(
            7,
            [[-50., -50.], [-40., -50.], [-50., -40.]],
            [0x001f; 3],
            [0, 0, 0],
        ),
    ]
}
fn task(ordinal: usize, epochs: [u32; 2]) -> source::Task {
    let slot = usize::from(ordinal != 0);
    let base = if ordinal == 0 { 0 } else { (ordinal - 1) * 3 };
    source::Task {
        triangle_id: 7,
        slot,
        epoch: epochs[slot],
        vertices: std::array::from_fn(|i| (base + i) as u8),
        context: CONTEXT,
    }
}
fn sources(inputs: &[tri::Input]) -> (source::Controller, [u32; 2]) {
    assert_eq!(inputs.len(), 6);
    let mut src = source::Controller::new(
        std::array::from_fn(|_| Slot::default()),
        CONTEXT,
        rr::MAX_WALL,
        rr::MAX_SOURCES,
    )
    .unwrap();
    let epochs = [src.allocate(0).unwrap(), src.allocate(1).unwrap()];
    for (ordinal, input) in inputs.iter().enumerate() {
        let t = task(ordinal, epochs);
        for (index, value) in t.vertices.iter().zip(&input.vertices) {
            src.publish_completed_vertex(t.slot, t.epoch, usize::from(*index), value)
                .unwrap();
        }
    }
    // Slot0 is sealed but still producing; slot1 is done but deliberately unsealed.
    src.finish_production(1, epochs[1]).unwrap();
    for ordinal in 0..4 {
        assert_eq!(
            src.submit(task(ordinal, epochs)).unwrap(),
            Some(ordinal as u64)
        );
    }
    src.seal(0, epochs[0]).unwrap();
    assert_eq!(src.queued(), 4);
    assert_eq!(src.submit(task(4, epochs)).unwrap(), None);
    (src, epochs)
}
fn check_edge(before: u64, after: u64) -> Result<(), String> {
    if after != before + 1 {
        return Err("duplicate or missing source/record clock callback edge".into());
    }
    Ok(())
}
fn pump_reader(
    reader: &mut rr::Reader,
    src: &mut source::Controller,
    lease: &mut link::Connection,
    ce: bool,
    action: rec::Input,
    ready: bool,
    accepted: bool,
) -> Result<link::Out, String> {
    let source_before = src.cycles();
    let reader_before = reader.stats.wall;
    let mut calls = 0;
    let out = reader.step_with_clock(ce, action, ready, accepted, |tr, input| {
        calls += 1;
        let out = lease.pump(src, tr, ce, input)?;
        check_edge(source_before, src.cycles())?;
        Ok(out)
    })?;
    assert_eq!(calls, 1);
    check_edge(reader_before, reader.stats.wall)?;
    Ok(out)
}

#[test]
fn actual_snapshot_numeric_rows_shared_mc_complete_image() {
    run_frame(false, "frame");
}
#[test]
fn actual_snapshot_ce_credit_reuse_and_finish_holds() {
    run_frame(true, "frame-paused");
}
fn run_frame(pauses: bool, name: &str) {
    use std::fmt::Write;
    let inputs = corpus();
    // Checker-only Report vector, kept outside every DUT source/writer interface.
    let reports: Vec<_> = inputs
        .iter()
        .map(|i| oracle::run(i, rr::profile()).unwrap())
        .collect();
    assert!(reports[0].triangles.len() >= 3);
    assert!(reports[5].triangles.is_empty());
    let fans: Vec<_> = reports.iter().map(|r| r.triangles.len()).collect();
    let record_count: usize = fans.iter().sum();
    assert!(record_count <= rr::MAX_RECORDS as usize);
    let f = Fixture::new();
    let (expected_quads, goldens, expected, helper_effect) = golden(&reports, &f);
    assert!(
        helper_effect,
        "uncovered helpers affect covered texture results"
    );
    let (mut src, epochs) = sources(&inputs);
    let mut lease = link::Connection::default();
    let mut writer = producer::Producer::default();
    let mut reader = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let s = f.shared();
    let (mut ro, mut fb) = s.views();
    let mut dut = PixelBranches::new(context(), lighting(), vec![f.slot], rr::MAX_WALL).unwrap();
    let mut next_task = 4;
    let mut full_retries = 0;
    let mut peak_pending = 4;
    let mut five_owned = false;
    let mut producer_guard = false;
    let mut unsealed_guard = false;
    let mut reused = false;
    let mut old_snapshot_after_reuse = 0;
    let mut release_before_complete = false;
    let mut source_release_before_complete = false;
    let mut accepted = 0;
    let mut results = 0;
    let mut lights = 0;
    let mut allocated = BTreeMap::new();
    let mut words = BTreeMap::<u64, [u64; rr::WORDS]>::new();
    let mut keys = Vec::new();
    let mut published_by_source = [0_usize; 6];
    let mut source_issues = [0_usize; 6];
    let mut source_returns = [0_usize; 6];
    let mut last_source_return = [None; 6];
    let mut third_fan_blocked = false;
    let mut last_use_edge = [None; 6];
    let mut confirmations = BTreeMap::new();
    let mut source_log = String::new();
    let mut pause_hits = BTreeSet::new();
    let mut pause_left = 0;
    let mut finish_sent = false;
    let mut previous: Option<BranchQuad> = None;
    let mut capture_started = false;
    for wall in 0..rr::MAX_WALL {
        let view = reader.view();
        assert!(view.published <= 2 && view.free <= 2 && src.queued() <= 4);
        let action = writer.action(&src).unwrap();
        third_fan_blocked |= action
            .reserve
            .is_some_and(|r| r.source.ticket == 0 && r.fan == 2)
            && view.published == 2
            && view.free == 0;
        let offer = reader.offer().cloned();
        if let Some(held) = &previous {
            compare_offer(offer.as_ref().expect("held quad disappeared"), held);
        }
        if pauses && pause_left == 0 {
            let hit = if capture_started
                && src.snapshot().is_none()
                && !pause_hits.contains("source-capture")
            {
                Some("source-capture")
            } else if lease.offer().is_some() && !pause_hits.contains("source-offer") {
                Some("source-offer")
            } else if lease.feedback().is_some() && !pause_hits.contains("source-feedback") {
                Some("source-feedback")
            } else if action.write.is_some() && !pause_hits.contains("writer-row") {
                Some("writer-row")
            } else if view.pending && !pause_hits.contains("record-pending") {
                Some("record-pending")
            } else if view.captured && !pause_hits.contains("record-captured") {
                Some("record-captured")
            } else if view.helper_lane == Some(2) && !pause_hits.contains("helper") {
                Some("helper")
            } else if view.phase == "confirm"
                && view.captured
                && !pause_hits.contains("final-return")
            {
                Some("final-return")
            } else if offer.is_some()
                && accepted + 1 == expected_quads.len()
                && !pause_hits.contains("last-quad")
            {
                Some("last-quad")
            } else {
                None
            };
            if let Some(hit) = hit {
                pause_hits.insert(hit);
                pause_left = 5;
            }
        }
        let ce = pause_left == 0;
        if pause_left > 0 {
            pause_left -= 1;
        }
        let all_source_done = next_task == 6
            && producer_guard
            && unsealed_guard
            && reused
            && src.slots().iter().all(|s| !s.held && !s.producing);
        let finish = ce
            && all_source_done
            && src.drained()
            && lease.offer().is_none()
            && lease.feedback().is_none()
            && writer.idle()
            && reader.drained()
            && offer.is_none();
        finish_sent |= finish;
        let clocks = s.clocks();
        let tick = dut
            .step(
                BranchTick {
                    ce,
                    quad: offer.clone(),
                    finish,
                    ..Default::default()
                },
                &mut ro,
                &mut fb,
            )
            .unwrap();
        assert_eq!(
            s.clocks(),
            (clocks.0 + 1, clocks.1 + 2),
            "sole physical MC clock"
        );
        if tick.live.model.quad_accepted {
            compare_offer(offer.as_ref().unwrap(), &expected_quads[accepted].1);
            let ticket = tick.live.model.ticket.unwrap();
            assert!(allocated.insert(ticket.serial, accepted).is_none());
            accepted += 1;
        }
        previous = offer.filter(|_| !tick.live.model.quad_accepted);
        if let Some(result) = tick.sample_returned {
            let index = allocated[&result.key.ticket.serial];
            assert_eq!(
                result.rgb,
                goldens[index].sample[usize::from(result.key.lane)]
            );
            results += 1;
        }
        if let Some((key, value, _)) = tick.live.light_returned {
            let index = allocated[&key.ticket.serial];
            assert_eq!(value, goldens[index].light[usize::from(key.lane)]);
            lights += 1;
        }
        let word_before = writer.held_word();
        let snapshot_before = src.snapshot().map(|s| s.input().unwrap());
        let offer_before = lease.offer();
        let feedback_before = lease.feedback();
        let out = pump_reader(
            &mut reader,
            &mut src,
            &mut lease,
            ce,
            action,
            wall >= 500,
            tick.live.model.quad_accepted,
        )
        .unwrap();
        writer.observe(ce, action, &out, &src).unwrap();
        if !ce {
            assert!(out.source.is_empty() && out.transport.is_empty());
            assert_eq!(lease.offer(), offer_before);
            assert_eq!(lease.feedback(), feedback_before);
            assert_eq!(writer.held_word(), word_before);
            assert_eq!(
                src.snapshot().map(|s| s.input().unwrap().vertices),
                snapshot_before.map(|s| s.vertices)
            );
        }
        for event in &out.transport {
            match event {
                rec::Event::SnapshotAccepted(owner) => {
                    assert_eq!(owner.context, CONTEXT);
                    assert_eq!(src.snapshot().unwrap().ticket, owner.ticket);
                    assert_eq!(
                        src.snapshot().unwrap().input().unwrap().vertices,
                        inputs[owner.ticket as usize].vertices
                    );
                }
                rec::Event::Reserved(key) => {
                    keys.push(*key);
                    assert!(words.insert(key.generation, [0; rr::WORDS]).is_none());
                }
                rec::Event::RowWritten { key, row } => {
                    words.get_mut(&key.generation).unwrap()[*row] = action.write.unwrap().word;
                    if reused && key.source.ticket == 0 {
                        old_snapshot_after_reuse += 1;
                    }
                }
                rec::Event::Published(key) => {
                    published_by_source[key.source.ticket as usize] += 1;
                }
                rec::Event::SnapshotLastUseAck(owner) => {
                    assert_eq!(
                        published_by_source[owner.ticket as usize],
                        fans[owner.ticket as usize]
                    );
                    assert!(last_use_edge[owner.ticket as usize].replace(wall).is_none());
                    assert!(src.snapshot().is_some(), "feedback must be later");
                }
                rec::Event::ConsumerCaptured(response) => {
                    assert_eq!(response.word, words[&response.key.generation][response.row]);
                    if response.last_attribute_capture {
                        assert!(confirmations
                            .insert(response.key.generation, wall)
                            .is_none());
                    }
                }
                rec::Event::RecordReleased(key) => {
                    assert!(confirmations[&key.generation] < wall);
                    release_before_complete |= !dut.complete();
                }
                _ => {}
            }
        }
        for event in &out.source {
            assert!(source_log.len() < 8_000_000);
            writeln!(source_log, "{wall},{event:?}").unwrap();
            match event {
                source::Event::ReadIssue { ticket, slot, row } => {
                    let at = source_issues[*ticket as usize];
                    let expected_task = task(*ticket as usize, epochs);
                    assert_eq!(*slot, expected_task.slot);
                    assert_eq!(
                        *row,
                        usize::from(expected_task.vertices[at / 7]) * 7 + at % 7
                    );
                    source_issues[*ticket as usize] += 1;
                    capture_started = true;
                }
                source::Event::ReadReturn {
                    ticket,
                    source_row,
                    data,
                } => {
                    let at = source_returns[*ticket as usize];
                    assert_eq!(*source_row, at);
                    assert_eq!(
                        *data,
                        inputs[*ticket as usize].vertices[at / 7].rows()[at % 7]
                    );
                    source_returns[*ticket as usize] += 1;
                    last_source_return[*ticket as usize] = Some(wall);
                }
                source::Event::SourceCaptured { ticket: 0, .. } => {
                    assert!(src.slots()[0].held && src.slots()[0].producing);
                    producer_guard = true;
                    src.finish_production(0, epochs[0]).unwrap();
                }
                source::Event::SourceCaptured { ticket: 5, .. } => {
                    assert!(src.slots()[1].held && !src.slots()[1].producing);
                    unsealed_guard = true;
                    src.seal(1, epochs[1]).unwrap();
                }
                source::Event::SourceReleased { slot: 0, epoch } if *epoch == epochs[0] => {
                    source_release_before_complete |= !dut.complete();
                    assert_eq!(src.snapshot().unwrap().ticket, 0);
                    let replacement = input(
                        99,
                        [[1., 1.], [3., 1.], [1., 3.]],
                        [0x001f; 3],
                        [-16384, 0, 0],
                    );
                    let new_epoch = src.allocate(0).unwrap();
                    assert_eq!(new_epoch, epochs[0] + 1);
                    for (index, vertex) in replacement.vertices.iter().enumerate() {
                        src.publish_completed_vertex(0, new_epoch, index, vertex)
                            .unwrap();
                    }
                    assert_ne!(&src.slots()[0].rows[..7], &inputs[0].vertices[0].rows());
                    assert_eq!(
                        src.snapshot().unwrap().input().unwrap().vertices,
                        inputs[0].vertices
                    );
                    src.finish_production(0, new_epoch).unwrap();
                    src.seal(0, new_epoch).unwrap();
                    reused = true;
                }
                source::Event::TriangleConsumed { ticket } => {
                    assert!(last_use_edge[*ticket as usize].unwrap() < wall);
                    assert_ne!(lease.feedback(), Some(*ticket));
                    if *ticket == 0 {
                        assert!(view.published > 0, "record survives snapshot");
                    }
                }
                _ => {}
            }
            if let source::Event::SourceCaptured { ticket, .. } = event {
                assert_eq!(
                    (
                        source_issues[*ticket as usize],
                        source_returns[*ticket as usize]
                    ),
                    (21, 21)
                );
                assert!(
                    last_source_return[*ticket as usize].unwrap() < wall,
                    "snapshot publication must follow its actual last return"
                );
            }
        }
        // Producer/adapter admissions use the new wall only for the next edge.
        if ce && next_task < 6 {
            match src.submit(task(next_task, epochs)).unwrap() {
                Some(ticket) => {
                    assert_eq!(ticket, next_task as u64);
                    next_task += 1;
                }
                None => {
                    full_retries += 1;
                }
            }
        }
        peak_pending = peak_pending.max(src.queued());
        five_owned |= next_task == 5 && src.queued() == 4 && !src.drained();
        if dut.complete() && reader.drained() && src.drained() {
            break;
        }
        assert!(wall + 1 < rr::MAX_WALL, "bounded composed frame");
    }
    assert!(finish_sent && dut.complete() && reader.drained() && src.drained() && s.gpu_idle());
    assert_eq!(accepted, expected_quads.len());
    let covered: usize = expected_quads
        .iter()
        .map(|(_, q)| q.live.quad.header.mask.count_ones() as usize)
        .sum();
    assert_eq!((results, lights), (covered, covered));
    assert_eq!(reader.stats.helpers, 4 * accepted as u64);
    assert_eq!(reader.stats.released, record_count as u64);
    assert_eq!(
        (lease.accepted(), lease.last_use(), lease.delivered()),
        (6, 6, 6)
    );
    assert!(writer.idle() && writer.stats.zero_fans == 1 && writer.stats.max_fans >= 3);
    assert_eq!(writer.stats.rows, record_count as u64 * rr::WORDS as u64);
    assert_eq!(
        writer.stats.rows, writer.stats.row_calls,
        "held rows must not recompute"
    );
    assert!(third_fan_blocked && writer.stats.blocked > 0 && reader.stats.peak_published == 2);
    assert_eq!(source_issues, [21; 6]);
    assert_eq!(source_returns, [21; 6]);
    assert!(keys.iter().enumerate().any(|(i, a)| keys[..i]
        .iter()
        .any(|b| b.slot == a.slot && b.generation != a.generation)));
    assert!(producer_guard && unsealed_guard && reused && old_snapshot_after_reuse > 51);
    assert!(full_retries > 0 && peak_pending == 4 && five_owned);
    assert!(release_before_complete && source_release_before_complete);
    if pauses {
        assert_eq!(
            pause_hits.len(),
            9,
            "directed hold coverage: {pause_hits:?}"
        );
    }
    s.stop_background();
    for edge in 0..4096 {
        if s.idle() {
            break;
        }
        ro.discard_edge().unwrap();
        fb.cycle(None, None).unwrap();
        assert!(edge + 1 < 4096, "bounded background drain");
    }
    assert!(s.idle());
    let mc = s.stats();
    assert!(mc.ro_started > 0 && mc.ro_terminals > 0 && mc.fb_writes > 0);
    assert_eq!(mc.fb_terminals, mc.fb_reads + mc.fb_writes);
    assert!(!s.tick_poisoned() && !s.physical_error());
    assert_eq!(
        s.image(),
        expected,
        "independent complete color/depth/texture/guard image"
    );
    std::fs::write(artifacts().join(format!("{name}-actual.bin")), s.image()).unwrap();
    std::fs::write(artifacts().join(format!("{name}-golden.bin")), &expected).unwrap();
    reader.save(&artifacts(), name);
    s.save(&artifacts(), name);
    std::fs::write(artifacts().join(format!("{name}-source.txt")), source_log).unwrap();
    std::fs::write(artifacts().join(format!("{name}-summary.txt")), format!(
        "quads={accepted}\ncovered={covered}\nfans={fans:?}\nfull_retries={full_retries}\npeak_pending={peak_pending}\nfive_owned={five_owned}\nold_snapshot_rows_after_reuse={old_snapshot_after_reuse}\npause_hits={pause_hits:?}\nwriter={:#?}\n", writer.stats)).unwrap();
}

fn one_source(value: &tri::Input) -> (source::Controller, u32) {
    let mut src = source::Controller::new(
        std::array::from_fn(|_| Slot::default()),
        CONTEXT,
        rr::MAX_WALL,
        rr::MAX_SOURCES,
    )
    .unwrap();
    let epoch = src.allocate(0).unwrap();
    for (i, vertex) in value.vertices.iter().enumerate() {
        src.publish_completed_vertex(0, epoch, i, vertex).unwrap();
    }
    src.submit(source::Task {
        triangle_id: value.id,
        slot: 0,
        epoch,
        vertices: [0, 1, 2],
        context: CONTEXT,
    })
    .unwrap()
    .unwrap();
    src.finish_production(0, epoch).unwrap();
    src.seal(0, epoch).unwrap();
    (src, epoch)
}

#[test]
fn callback_once_and_compatibility_wrapper_preserve_record_events() {
    let mut original = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let mut callback = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let owner = rec::SourceOwner {
        ticket: 0,
        context: CONTEXT,
    };
    let mut calls = 0;
    for wall in 0..8 {
        let ce = wall != 2;
        let input = match wall {
            0 => rec::Input {
                source_captured: Some(owner),
                ..Default::default()
            },
            1 => rec::Input {
                source_end: Some(rec::SourceEnd {
                    source: owner,
                    fans: 0,
                }),
                ..Default::default()
            },
            _ => rec::Input::default(),
        };
        let a = original.step(ce, input, true, false).unwrap();
        let b = callback
            .step_with_clock(ce, input, true, false, |tr, action| {
                calls += 1;
                Ok(link::Out {
                    source: vec![],
                    transport: tr.step(action).map_err(|e| format!("{e:?}"))?,
                })
            })
            .unwrap();
        assert_eq!(a, b.transport);
        if wall == 1 {
            assert_eq!(a, [rec::Event::SnapshotLastUseAck(owner)]);
        }
        assert_eq!(original.trace, callback.trace);
        assert_eq!(original.stats.wall, callback.stats.wall);
    }
    assert_eq!(calls, 8);
    assert!(original.drained() && callback.drained());
    std::fs::create_dir_all(artifacts()).unwrap();
    std::fs::write(artifacts().join("host-sizes.csv"), format!(
        "type,inline_host_bytes\nReader,{}\nDecoded,{}\nAttributes,{}\nBranchQuad,{}\nProducer,{}\nProducerStats,{}\nSourceTask,{}\nSourceSnapshot,{}\nRecordKey,{}\n",
        std::mem::size_of::<rr::Reader>(), std::mem::size_of::<rr::Decoded>(),
        std::mem::size_of::<rr::Attributes>(), std::mem::size_of::<BranchQuad>(),
        std::mem::size_of::<producer::Producer>(), std::mem::size_of::<producer::Stats>(),
        std::mem::size_of::<source::Task>(), std::mem::size_of::<source::Snapshot>(),
        std::mem::size_of::<rec::Key>(),
    )).unwrap();
}

#[test]
fn fake_source_owner_and_stale_source_epoch_are_rejected() {
    let value = corpus().remove(1);
    let (mut src, epoch) = one_source(&value);
    let mut lease = link::Connection::default();
    let mut reader = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let mut writer = producer::Producer::default();
    for wall in 0..64 {
        let out = pump_reader(
            &mut reader,
            &mut src,
            &mut lease,
            true,
            rec::Input::default(),
            true,
            false,
        )
        .unwrap();
        if out
            .source
            .iter()
            .any(|e| matches!(e, source::Event::SourceCaptured { .. }))
        {
            let real = rec::SourceOwner {
                ticket: 0,
                context: CONTEXT,
            };
            let fake = link::Out {
                source: vec![],
                transport: vec![rec::Event::SnapshotAccepted(rec::SourceOwner {
                    ticket: 1,
                    ..real
                })],
            };
            assert!(writer
                .observe(true, rec::Input::default(), &fake, &src)
                .unwrap_err()
                .contains("owner mismatch"));
            assert!(writer.idle() && writer.stats.rows == 0 && writer.stats.accepted == 0);
            let clock = src.cycles();
            // Caller cannot impersonate the connection's real snapshot offer.
            let error = reader
                .step_with_clock(
                    true,
                    rec::Input {
                        source_captured: Some(real),
                        ..Default::default()
                    },
                    true,
                    false,
                    |tr, action| lease.pump(&mut src, tr, true, action),
                )
                .unwrap_err();
            assert!(error.contains("source offer is owned"));
            assert_eq!(src.cycles(), clock);
            assert!(src.snapshot().is_some() && lease.accepted() == 0);
            assert!(!src.slots()[0].held);
            let next_epoch = src.allocate(0).unwrap();
            assert_eq!(next_epoch, epoch + 1);
            for (i, vertex) in value.vertices.iter().enumerate() {
                src.publish_completed_vertex(0, next_epoch, i, vertex)
                    .unwrap();
            }
            assert!(src
                .submit(source::Task {
                    triangle_id: value.id,
                    slot: 0,
                    epoch,
                    vertices: [0, 1, 2],
                    context: CONTEXT,
                })
                .unwrap_err()
                .contains("stale source slot epoch"));
            return;
        }
        assert!(wall + 1 < 64, "bounded real snapshot negative test");
    }
    panic!("snapshot negative test did not trigger");
}

#[test]
fn duplicate_connection_clock_is_detected_and_not_retried() {
    let (mut src, _) = one_source(&corpus().remove(1));
    let mut lease = link::Connection::default();
    let mut reader = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let before = src.cycles();
    let error = reader
        .step_with_clock(false, rec::Input::default(), true, false, |tr, action| {
            let out = lease.pump(&mut src, tr, false, action)?;
            lease.pump(&mut src, tr, false, rec::Input::default())?;
            check_edge(before, src.cycles())?;
            Ok(out)
        })
        .unwrap_err();
    assert!(error.contains("duplicate or missing"));
    assert_eq!(src.cycles(), before + 2);
    assert_eq!(reader.stats.wall, 1);
    assert!(reader.trace.is_empty() && lease.accepted() == 0);
    // Deliberately mutated owners are terminal; no recovery or success claim.
}

#[test]
fn actual_published_record_rejects_ack_before_final_attribute_transfer() {
    let (mut src, _) = one_source(&corpus().remove(1));
    let mut lease = link::Connection::default();
    let mut reader = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let mut writer = producer::Producer::default();
    for wall in 0..256 {
        let action = writer.action(&src).unwrap();
        let out =
            pump_reader(&mut reader, &mut src, &mut lease, true, action, true, false).unwrap();
        writer.observe(true, action, &out, &src).unwrap();
        if let Some(key) = out.transport.iter().find_map(|e| {
            if let rec::Event::Published(key) = e {
                Some(*key)
            } else {
                None
            }
        }) {
            let mut calls = 0;
            // Reader rejects consumer action forgery before invoking the clock.
            let error = reader
                .step_with_clock(
                    true,
                    rec::Input {
                        last_quad_ack: Some(key),
                        ..Default::default()
                    },
                    true,
                    false,
                    |_, _| {
                        calls += 1;
                        Ok(link::Out::default())
                    },
                )
                .unwrap_err();
            assert!(error.contains("reader-owned") && calls == 0);
            // The real controller independently rejects the same early ACK.
            let error = reader
                .step_with_clock(
                    true,
                    rec::Input::default(),
                    true,
                    false,
                    |tr, mut action| {
                        action.last_quad_ack = Some(key);
                        lease.pump(&mut src, tr, true, action)
                    },
                )
                .unwrap_err();
            assert!(error.contains("EarlyAck"));
            assert_eq!(reader.stats.released, 0);
            assert_eq!(reader.stats.confirmations, 0);
            assert!(reader.view().published > 0 && !reader.drained());
            return;
        }
        assert!(wall + 1 < 256, "bounded early ACK trigger");
    }
    panic!("early ACK test did not trigger");
}

#[test]
fn unsupported_numeric_profile_stops_before_any_record_is_reserved() {
    let mut reversed = corpus().remove(1);
    reversed.vertices.swap(1, 2);
    let (mut src, _) = one_source(&reversed);
    let mut lease = link::Connection::default();
    let mut reader = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let mut writer = producer::Producer::default();
    for wall in 0..64 {
        let action = writer.action(&src).unwrap();
        let out =
            pump_reader(&mut reader, &mut src, &mut lease, true, action, true, false).unwrap();
        if out
            .transport
            .iter()
            .any(|e| matches!(e, rec::Event::SnapshotAccepted(_)))
        {
            assert!(writer
                .observe(true, action, &out, &src)
                .unwrap_err()
                .contains("front-facing"));
            assert_eq!(reader.stats.reserved, 0);
            assert_eq!(writer.stats.rows, 0);
            assert_eq!(lease.last_use(), 0);
            assert!(
                src.snapshot().is_some(),
                "failed preflight retains its owned source"
            );
            return;
        }
        writer.observe(true, action, &out, &src).unwrap();
        assert!(wall + 1 < 64, "bounded preflight trigger");
    }
    panic!("unsupported numeric profile not rejected");
}

#[test]
fn source_link_cancel_is_terminal_but_real_accepted_fb_write_drains() {
    let input = corpus().remove(0);
    let report = oracle::run(&input, rr::profile()).unwrap();
    let f = Fixture::new();
    let (quads, goldens, _, _) = golden(&[report], &f);
    let (mut src, _) = one_source(&input);
    let mut lease = link::Connection::default();
    let mut reader = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let mut writer = producer::Producer::default();
    let s = f.shared();
    let (mut ro, mut fb) = s.views();
    let mut dut = PixelBranches::new(context(), lighting(), vec![f.slot], rr::MAX_WALL).unwrap();
    let mut accepted = 0;
    let mut writes = BTreeSet::new();
    let mut triggered = false;
    let mut finish_intent = false;
    for wall in 0..rr::MAX_WALL {
        if s.fb_write_active() && s.stats().fb_write_beats > 0 {
            triggered = true;
            break;
        }
        let offer = reader.offer().cloned();
        let finish = writer.idle()
            && reader.drained()
            && src.drained()
            && lease.offer().is_none()
            && lease.feedback().is_none()
            && offer.is_none();
        finish_intent |= finish;
        let clocks = s.clocks();
        let tick = dut
            .step(
                BranchTick {
                    quad: offer.clone(),
                    finish,
                    ..Default::default()
                },
                &mut ro,
                &mut fb,
            )
            .unwrap();
        assert_eq!(s.clocks(), (clocks.0 + 1, clocks.1 + 2));
        if tick.live.model.quad_accepted {
            compare_offer(offer.as_ref().unwrap(), &quads[accepted].1);
            accepted += 1;
        }
        if tick.live.model.framebuffer.response.accepted {
            let request = tick.live.model.framebuffer.request.unwrap();
            if request.write {
                writes.insert(request.address_bytes as usize);
            }
        }
        let action = writer.action(&src).unwrap();
        let out = pump_reader(
            &mut reader,
            &mut src,
            &mut lease,
            true,
            action,
            true,
            tick.live.model.quad_accepted,
        )
        .unwrap();
        writer.observe(true, action, &out, &src).unwrap();
        assert!(
            wall + 1 < rr::MAX_WALL,
            "bounded real write cancellation trigger"
        );
    }
    assert!(triggered && finish_intent && accepted > 0 && !writes.is_empty() && !dut.complete());
    let last_use = lease.last_use();
    let delivered = lease.delivered();
    let released = reader.stats.released;
    let source_before = src.cycles();
    src.cancel();
    let error = pump_reader(
        &mut reader,
        &mut src,
        &mut lease,
        true,
        rec::Input::default(),
        true,
        false,
    )
    .unwrap_err();
    assert!(error.contains("cancellation composition"));
    assert_eq!(src.cycles(), source_before + 1);
    assert_eq!((lease.last_use(), lease.delivered()), (last_use, delivered));
    assert_eq!(reader.stats.released, released);
    // Do not cancel/re-clock/recreate geometry owners after unsupported composition.
    // Only the separately supported downstream transport drain is exercised.
    dut.abort();
    s.abort_sampling();
    for edge in 0..4096 {
        ro.discard_edge().unwrap();
        let clocks = s.clocks();
        let tick = dut.step(BranchTick::default(), &mut ro, &mut fb).unwrap();
        assert_eq!(s.clocks(), (clocks.0 + 1, clocks.1 + 2));
        assert!(tick.sampling.is_none() && tick.sample_returned.is_none() && !dut.complete());
        assert!(
            !tick.live.model.framebuffer.response.accepted,
            "fault drain cannot admit new MC transactions"
        );
        if dut.framebuffer_drained() && s.idle() {
            break;
        }
        assert!(
            edge + 1 < 4096,
            "bounded accepted texture/FB/background drain"
        );
    }
    assert!(dut.faulted() && !dut.complete() && dut.framebuffer_drained() && s.idle());
    assert_eq!(
        reader.stats.released, released,
        "drain does not manufacture geometry success"
    );
    let stats = s.stats();
    assert_eq!(stats.fb_write_beats, stats.fb_writes * 16);
    assert_eq!(stats.fb_terminals, stats.fb_reads + stats.fb_writes);
    assert_eq!((lease.last_use(), lease.delivered()), (last_use, delivered));
    // Independent prefix rendering supplies the data of accepted external lines.
    // Guards and every unaccepted line must retain deliberately different initial data.
    let prefix = pixel::golden(f.initial.clone(), &goldens[..accepted], context());
    let mut partial = f.initial.clone();
    for &address in &writes {
        partial[address..address + 128].copy_from_slice(&prefix[address..address + 128]);
    }
    assert_eq!(
        s.image(),
        partial,
        "independent accepted-write footprint, no rollback"
    );
    assert_ne!(partial, f.initial);
    s.save(&artifacts(), "source-cancel-write");
    reader.save(&artifacts(), "source-cancel-write");
    std::fs::write(artifacts().join("source-cancel-write-summary.txt"), format!(
        "accepted={accepted}\nwrites={writes:?}\nlast_use_at_cancel={last_use}\ndelivered_at_cancel={delivered}\nrecord_release_at_cancel={released}\nunsupported geometry cancellation preserved; real accepted FB/RO drained; no render or geometry success\n"
    )).unwrap();
}

#[test]
fn held_snapshot_cancel_keeps_published_records_and_drains_accepted_texture() {
    let input = corpus().remove(0);
    let report = oracle::run(&input, rr::profile()).unwrap();
    let f = Fixture::new();
    let (quads, _, _, _) = golden(&[report], &f);
    let (mut src, _) = one_source(&input);
    let mut lease = link::Connection::default();
    let mut reader = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let mut writer = producer::Producer::default();
    let s = f.shared();
    let (mut ro, mut fb) = s.views();
    let mut dut = PixelBranches::new(context(), lighting(), vec![f.slot], rr::MAX_WALL).unwrap();
    let mut accepted = 0;
    let mut triggered = false;
    for wall in 0..rr::MAX_WALL {
        if accepted > 0
            && reader.view().published == 2
            && writer.stats.blocked > 0
            && src.snapshot().is_some()
            && s.ro_active()
        {
            triggered = true;
            break;
        }
        let offer = reader.offer().cloned();
        let clocks = s.clocks();
        let tick = dut
            .step(
                BranchTick {
                    quad: offer.clone(),
                    ..Default::default()
                },
                &mut ro,
                &mut fb,
            )
            .unwrap();
        assert_eq!(s.clocks(), (clocks.0 + 1, clocks.1 + 2));
        if tick.live.model.quad_accepted {
            compare_offer(offer.as_ref().unwrap(), &quads[accepted].1);
            accepted += 1;
        }
        let action = writer.action(&src).unwrap();
        let out = pump_reader(
            &mut reader,
            &mut src,
            &mut lease,
            true,
            action,
            true,
            tick.live.model.quad_accepted,
        )
        .unwrap();
        writer.observe(true, action, &out, &src).unwrap();
        assert!(
            wall + 1 < rr::MAX_WALL,
            "bounded held snapshot cancellation trigger"
        );
    }
    assert!(triggered && src.snapshot().unwrap().ticket == 0);
    assert_eq!((lease.last_use(), lease.delivered()), (0, 0));
    let released = reader.stats.released;
    src.cancel();
    let error = pump_reader(
        &mut reader,
        &mut src,
        &mut lease,
        true,
        rec::Input::default(),
        true,
        false,
    )
    .unwrap_err();
    assert!(error.contains("cancellation composition"));
    assert_eq!((lease.last_use(), lease.delivered()), (0, 0));
    assert_eq!(reader.view().published, 2);
    assert!(
        !reader.drained(),
        "unsupported composition must preserve live records"
    );
    dut.abort();
    s.abort_sampling();
    for edge in 0..4096 {
        ro.discard_edge().unwrap();
        let clocks = s.clocks();
        let tick = dut.step(BranchTick::default(), &mut ro, &mut fb).unwrap();
        assert_eq!(s.clocks(), (clocks.0 + 1, clocks.1 + 2));
        assert!(tick.sampling.is_none() && !dut.complete());
        if dut.framebuffer_drained() && s.idle() {
            break;
        }
        assert!(edge + 1 < 4096, "bounded actual accepted RO drain");
    }
    let stats = s.stats();
    assert!(s.idle() && dut.framebuffer_drained() && dut.faulted() && !dut.complete());
    assert_eq!(stats.fb_writes, 0);
    assert!(stats.ro_discarded_beats > 0 && stats.ro_terminals > 0);
    assert_eq!(
        s.image(),
        f.initial,
        "unaccepted dirty cache data is not an external write"
    );
    assert_eq!(reader.stats.released, released);
    assert_eq!(reader.view().published, 2);
    s.save(&artifacts(), "source-cancel-held");
    reader.save(&artifacts(), "source-cancel-held");
}
