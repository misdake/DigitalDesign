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
#[allow(dead_code)]
#[path = "support/source_record_raster_producer.rs"]
mod producer;
fn artifacts() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-pixel-branches/persistent-frontend-20261004");
    std::fs::create_dir_all(&path).unwrap();
    path
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
            force_coarsest: false,
        }),
    };
    for (lane, s) in samples.iter().enumerate() {
        let sample = q.sample.as_mut().unwrap();
        let captured = s.quantized.uv.map(|v| v as f64 / 65536.);
        if s.quantized
            .uv
            .iter()
            .all(|&v| (-131072..=131071).contains(&v))
        {
            sample.uv[lane] = captured;
        } else {
            assert_eq!(mask >> lane & 1, 0, "only uncovered helper may overflow");
            sample.force_coarsest = true;
        }
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
                normal: s.quantized.normal.map(|v| i16::try_from(v * 16).unwrap()),
                ndc: [px * 1024 + 512 - 16384, 16384 - py * 1024 - 512],
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

#[path = "support/persistent_frontend.rs"]
mod frontend;
use gpu_v2::{
    command_processor::ports::Command,
    frontend::{
        ports::Input as FrontendInput,
        sim::timed::{Action, Config as FrontendConfig},
    },
    scratchpad::ports::{DmaDescriptor, REGION_BYTES},
    vertex::{
        ports::{Context as VertexContext, PackedVertex},
        sim::oracle as vertex_oracle,
    },
};

fn program() -> FrontendInput {
    let points = [
        [[16, 16], [-130, -50], [190, -50]],
        [[4, 4], [20, 4], [4, 20]],
        [[66, 66], [91, 66], [66, 91]],
        [[-130, 6], [5, 6], [5, 25]],
        [[12, 12], [18, 12], [12, 18]],
        [[-50, -50], [-40, -50], [-50, -40]],
    ];
    let normals = [
        [0, 0, 127],
        [0, 0, 127],
        [-64, 32, 96],
        [64, -32, 127],
        [32, 64, 96],
        [0, 0, 0],
    ];
    let colors = [
        [0xffff; 3],
        [0xf800; 3],
        [0x07e0; 3],
        [0x1234, 0xabcd, 0xffff],
        [0x001f; 3],
        [0x001f; 3],
    ];
    let mut memory = Vec::new();
    let mut contexts = Vec::new(); // Immutable external command stimulus only.
    for draw in 0..6 {
        let tiny = draw == 2;
        let context = VertexContext {
            grid_shift: if tiny { 8 } else { 12 },
            base: if tiny {
                [-65536, -65536, 0]
            } else {
                [-598016, -270336, 0]
            },
            mvp: [
                [65536, 0, 0, 0],
                [0, -65536, 0, 0],
                [0, 0, 65536, 0],
                [0, 0, 0, 65536],
            ],
            ..VertexContext::default()
        };
        for vertex in 0..3 {
            let xy = points[draw][vertex];
            let xyz = if tiny {
                [xy[0] as u16, xy[1] as u16, 0]
            } else {
                [(xy[0] + 130) as u16, (xy[1] + 50) as u16, 0]
            };
            let uv = if draw == 0 {
                [2048, 1024]
            } else {
                [[64, 96], [4095, 96], [64, 4095]][vertex]
            };
            let packed =
                PackedVertex::encode(xyz, normals[draw], uv, colors[draw][vertex]).unwrap();
            for word in packed.0 {
                memory.extend(word.to_le_bytes());
            }
        }
        memory.resize((draw + 1) * 40, 0xa5);
        contexts.push(context);
    }
    let dma = |draw: usize| {
        Command::Dma(DmaDescriptor {
            physical_addr: 0x1000 + draw as u64 * 40,
            scratchpad_addr: draw % 2 * REGION_BYTES,
            byte_count: 40,
            completion_token: (draw % 2) as u8,
        })
    };
    let mut commands = vec![dma(0), dma(1)];
    for (draw, context) in contexts.into_iter().enumerate() {
        if draw >= 2 {
            commands.push(dma(draw));
        }
        commands.push(Command::Wait((draw % 2) as u8));
        commands.push(Command::Draw {
            region: draw % 2,
            byte_offset: 0,
            vertices: 3,
            context,
        });
    }
    commands.push(Command::Fence);
    FrontendInput {
        commands,
        memory_base: 0x1000,
        memory,
    }
}
fn independent_inputs(input: &FrontendInput) -> Vec<tri::Input> {
    // Checker decodes original bytes directly; never reads runtime Plan/slots.
    input
        .commands
        .iter()
        .filter_map(|cmd| {
            let Command::Draw { context, .. } = cmd else {
                return None;
            };
            Some(context)
        })
        .enumerate()
        .map(|(draw, context)| tri::Input {
            id: 7,
            vertices: std::array::from_fn(|v| {
                let start = draw * 40 + v * 12;
                let packed = PackedVertex(std::array::from_fn(|i| {
                    u32::from_le_bytes(
                        input.memory[start + i * 4..start + i * 4 + 4]
                            .try_into()
                            .unwrap(),
                    )
                }));
                vertex_oracle::run(context, packed, &Default::default())
                    .unwrap()
                    .output
            }),
        })
        .collect()
}

#[test]
fn persistent_packed_vertices_to_complete_shared_mc_frame() {
    run_frame(false, "persistent-frame");
}
#[test]
fn persistent_frontend_ce_pauses_and_old_slot_reuse() {
    run_frame(true, "persistent-frame-paused");
}
fn run_frame(pauses: bool, name: &str) {
    use std::fmt::Write;
    let input = program();
    let inputs = independent_inputs(&input);
    let reports: Vec<_> = inputs
        .iter()
        .map(|i| oracle::run(i, rr::profile()).unwrap())
        .collect();
    assert!(reports[0].triangles.len() >= 3 && reports[5].triangles.is_empty());
    let fans: Vec<_> = reports.iter().map(|r| r.triangles.len()).collect();
    let record_count: usize = fans.iter().sum();
    assert!(record_count <= rr::MAX_RECORDS as usize);
    let f = Fixture::new();
    let (expected_quads, goldens, expected, helper_effect) = golden(&reports, &f);
    assert!(helper_effect);
    let mut src = source::Controller::new(
        std::array::from_fn(|_| Slot::default()),
        CONTEXT,
        rr::MAX_WALL,
        rr::MAX_SOURCES,
    )
    .unwrap();
    let mut front = frontend::Frontend::new(
        &input,
        FrontendConfig {
            max_cycles: rr::MAX_WALL,
            ..Default::default()
        },
        CONTEXT,
    )
    .unwrap();
    let mut lease = link::Connection::default();
    let mut writer = producer::Producer::default();
    let mut reader = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let shared = f.shared();
    let (mut ro, mut fb) = shared.views();
    let mut dut = PixelBranches::new(context(), lighting(), vec![f.slot], rr::MAX_WALL).unwrap();
    let mut accepted = 0;
    let mut results = 0;
    let mut lights = 0;
    let mut allocated = BTreeMap::new();
    let mut words = BTreeMap::<u64, [u64; rr::WORDS]>::new();
    let mut source_reads = [0; 6];
    let mut source_returns = [0; 6];
    let mut last_return = [None; 6];
    let mut write_masks = [[0_u8; 3]; 6];
    let mut publications = [0; 6];
    let mut current_draw = 0;
    let mut started = 0;
    let mut completed = 0;
    let mut epochs = BTreeMap::<(usize, u32), usize>::new();
    let mut release_edges = BTreeMap::<(usize, u32), u64>::new();
    let mut confirmations = BTreeMap::new();
    let mut last_use = [None; 6];
    let mut frontend_log = String::new();
    let mut source_log = String::new();
    let mut previous: Option<BranchQuad> = None;
    let mut reused_during_snapshot = false;
    let mut old_rows_after_reuse = 0;
    let mut both_slot_wait = 0;
    let mut fence_before_render = false;
    let mut finish_sent = false;
    let mut pause_hits = BTreeSet::new();
    for wall in 0..rr::MAX_WALL {
        let view = reader.view();
        let action = writer.action(&src).unwrap();
        let offer = reader.offer().cloned();
        if let Some(held) = &previous {
            compare_offer(offer.as_ref().unwrap(), held);
        }
        let ce = !pauses || wall % 47 >= 5;
        if !ce {
            if action.write.is_some() {
                pause_hits.insert("write");
            }
            if view.pending {
                pause_hits.insert("pending");
            }
            if offer.is_some() {
                pause_hits.insert("quad");
            }
            if front.engine.current_plan().is_some() {
                pause_hits.insert("vertex");
            }
        }
        // Snapshot metadata before the sole source/record/MC edge.
        let permit = src.prepare_producer_edge(ce);
        let pending_before = front.pending();
        let snapshot_before = src.snapshot().cloned(); // Checker-only witness.
        let slots_before = src.slots().clone(); // Checker-only; never a DUT input.
        if matches!(
            input.commands.get(front.engine.pc()),
            Some(Command::Draw { .. })
        ) && front.engine.retained_plan_count() == 0
            && slots_before.iter().all(|s| s.held)
        {
            both_slot_wait += 1;
        }
        let finish = ce
            && front.topology_drained()
            && src.drained()
            && src.slots().iter().all(|s| !s.held)
            && lease.offer().is_none()
            && lease.feedback().is_none()
            && writer.idle()
            && reader.drained();
        finish_sent |= finish;
        let clocks = shared.clocks();
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
        assert_eq!(shared.clocks(), (clocks.0 + 1, clocks.1 + 2));
        if tick.live.model.quad_accepted {
            compare_offer(offer.as_ref().unwrap(), &expected_quads[accepted].1);
            assert!(allocated
                .insert(tick.live.model.ticket.unwrap().serial, accepted)
                .is_none());
            accepted += 1;
        }
        previous = offer.filter(|_| !tick.live.model.quad_accepted);
        if let Some(result) = tick.sample_returned {
            let index = allocated[&result.key.ticket.serial];
            assert_eq!(result.rgb, goldens[index].sample[result.key.lane as usize]);
            results += 1;
        }
        if let Some((key, value, _)) = tick.live.light_returned {
            let index = allocated[&key.ticket.serial];
            assert_eq!(value, goldens[index].light[key.lane as usize]);
            lights += 1;
        }
        let mut calls = 0;
        let out = reader
            .step_with_clock(
                ce,
                action,
                wall >= 500,
                tick.live.model.quad_accepted,
                |tr, action| {
                    calls += 1;
                    lease.pump(&mut src, tr, ce, action)
                },
            )
            .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(reader.stats.wall, wall + 1);
        assert_eq!(src.cycles(), wall + 1);
        writer.observe(ce, action, &out, &src).unwrap();
        let front_out = front.after_clock(&mut src, permit, ce).unwrap();
        assert_eq!(front.engine.wall(), wall + 1);
        assert!(front.engine.retained_plan_count() <= 1);
        assert!(front.engine.fault().is_none());
        fence_before_render |= front.engine.fence() && !dut.complete();
        if !ce {
            assert!(out.source.is_empty() && out.transport.is_empty());
            assert_eq!(front.pending(), pending_before);
            assert_eq!(src.slots(), &slots_before);
            assert_eq!(src.snapshot(), snapshot_before.as_ref());
            assert!(front_out.records.iter().all(|r| !matches!(
                r.action,
                Action::DrawStart { .. }
                    | Action::VertexWrite { .. }
                    | Action::Publish { .. }
                    | Action::DrawDone { .. }
            )));
        }
        for record in &front_out.records {
            writeln!(frontend_log, "{record:?}").unwrap();
            match record.action {
                Action::DrawStart { slot, epoch, .. } => {
                    assert!(!slots_before[slot].held, "cannot borrow same-edge release");
                    if epoch > 1 {
                        assert!(release_edges[&(slot, epoch - 1)] < wall);
                        reused_during_snapshot |= src.snapshot().is_some();
                    }
                    current_draw = started;
                    started += 1;
                    assert!(epochs.insert((slot, epoch), current_draw).is_none());
                }
                Action::VertexWrite {
                    slot,
                    epoch,
                    row,
                    data,
                } => {
                    let draw = epochs[&(slot, epoch)];
                    let vertex = row / 7;
                    let pos = row % 7;
                    assert_eq!(data, inputs[draw].vertices[vertex].rows()[pos]);
                    assert_eq!(write_masks[draw][vertex] & (1 << pos), 0);
                    write_masks[draw][vertex] |= 1 << pos;
                }
                Action::Publish {
                    slot,
                    epoch,
                    vertex,
                } => {
                    let draw = epochs[&(slot, epoch)];
                    assert_eq!(write_masks[draw][vertex], 0x7f);
                    publications[draw] += 1;
                }
                Action::DrawDone { .. } => {
                    assert_eq!(publications[current_draw], 3);
                    completed += 1;
                }
                _ => {}
            }
        }
        for event in &out.source {
            writeln!(source_log, "{wall},{event:?}").unwrap();
            match event {
                source::Event::ReadIssue { ticket, slot, row } => {
                    assert_eq!(
                        epochs[&(*slot, slots_before[*slot].epoch)],
                        *ticket as usize
                    );
                    assert_eq!(*row, source_reads[*ticket as usize]);
                    source_reads[*ticket as usize] += 1;
                }
                source::Event::ReadReturn {
                    ticket,
                    source_row,
                    data,
                } => {
                    let i = *ticket as usize;
                    assert_eq!(*source_row, source_returns[i]);
                    assert_eq!(
                        *data,
                        inputs[i].vertices[source_row / 7].rows()[source_row % 7]
                    );
                    source_returns[i] += 1;
                    last_return[i] = Some(wall);
                }
                source::Event::SourceCaptured { ticket, .. } => {
                    let i = *ticket as usize;
                    assert_eq!((source_reads[i], source_returns[i]), (21, 21));
                    assert!(last_return[i].unwrap() < wall);
                }
                source::Event::SourceReleased { slot, epoch } => {
                    assert!(release_edges.insert((*slot, *epoch), wall).is_none());
                }
                source::Event::TriangleConsumed { ticket } => {
                    assert!(last_use[*ticket as usize].unwrap() < wall)
                }
                _ => {}
            }
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
                    assert!(words.insert(key.generation, [0; rr::WORDS]).is_none());
                }
                rec::Event::RowWritten { key, row } => {
                    words.get_mut(&key.generation).unwrap()[*row] = action.write.unwrap().word;
                    if reused_during_snapshot && key.source.ticket == 0 {
                        old_rows_after_reuse += 1;
                    }
                }
                rec::Event::Published(key) => {
                    assert_eq!(
                        words[&key.generation],
                        rr::Encoded::from_report(
                            &reports[key.source.ticket as usize],
                            key.fan as usize
                        )
                        .unwrap()
                        .0
                    );
                }
                rec::Event::SnapshotLastUseAck(owner) => {
                    last_use[owner.ticket as usize] = Some(wall);
                }
                rec::Event::ConsumerCaptured(r) => {
                    assert_eq!(r.word, words[&r.key.generation][r.row]);
                    if r.last_attribute_capture {
                        confirmations.insert(r.key.generation, wall);
                    }
                }
                rec::Event::RecordReleased(key) => assert!(confirmations[&key.generation] < wall),
                _ => {}
            }
        }
        assert!(frontend_log.len() < 8_000_000 && source_log.len() < 8_000_000);
        if dut.complete() && reader.drained() && src.drained() {
            break;
        }
        assert!(wall + 1 < rr::MAX_WALL);
    }
    assert!(
        finish_sent && dut.complete() && reader.drained() && src.drained() && shared.gpu_idle()
    );
    assert_eq!((started, completed, front.admitted), (6, 6, 6));
    assert_eq!(write_masks, [[0x7f; 3]; 6]);
    assert_eq!(source_reads, [21; 6]);
    assert_eq!(source_returns, [21; 6]);
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
    assert_eq!(writer.stats.rows, writer.stats.row_calls);
    assert!(
        reused_during_snapshot
            && old_rows_after_reuse > 0
            && both_slot_wait > 0
            && fence_before_render
    );
    if pauses {
        assert_eq!(pause_hits.len(), 4);
    }
    shared.stop_background();
    for edge in 0..4096 {
        if shared.idle() {
            break;
        }
        ro.discard_edge().unwrap();
        fb.cycle(None, None).unwrap();
        assert!(edge + 1 < 4096);
    }
    assert!(shared.idle());
    let mc = shared.stats();
    assert!(mc.ro_started > 0 && mc.ro_terminals > 0 && mc.fb_writes > 0);
    assert_eq!(mc.fb_terminals, mc.fb_reads + mc.fb_writes);
    assert!(!shared.tick_poisoned() && !shared.physical_error());
    assert_eq!(
        shared.image(),
        expected,
        "entire independent framebuffer/depth/texture/guards"
    );
    std::fs::write(
        artifacts().join(format!("{name}-actual.bin")),
        shared.image(),
    )
    .unwrap();
    std::fs::write(artifacts().join(format!("{name}-golden.bin")), expected).unwrap();
    std::fs::write(
        artifacts().join(format!("{name}-frontend.txt")),
        frontend_log,
    )
    .unwrap();
    std::fs::write(artifacts().join(format!("{name}-source.txt")), source_log).unwrap();
    std::fs::write(artifacts().join(format!("{name}-summary.txt")),format!("fans={fans:?}\nquads={accepted}\ncovered={covered}\nboth_slot_wait={both_slot_wait}\nold_rows_after_reuse={old_rows_after_reuse}\nfrontend_fence_before_render={fence_before_render}\nwriter={:?}\n",writer.stats)).unwrap();
    reader.save(&artifacts(), name);
    shared.save(&artifacts(), name);
}
