//! Actual selected Lighting, Runtime Sampling and registered Final, then actual
//! serial FramebufferEmu over the existing shared physical MC test adapter.
use digital_design_hardware_gowin::sdram_memory_controller::ports::OracleImage;
use gpu_v2::{
    framebuffer::ports::{Blend, Context as RopContext, DepthFunc, Header, MaterializedSurface},
    lighting::ports::*,
    system::pixel::{
        dispatch::{CommonContext, Input, SampleContext},
        foundation,
        foundation_backend::Backend,
        foundation_live::{self, Live},
        Basic,
    },
    texture::{
        ports::{self as tex, Filter, RefillEvent, RefillPort},
        sim::oracle as texture_oracle,
    },
};
use std::collections::{BTreeMap, VecDeque};
#[allow(dead_code)]
#[path = "support/pixel.rs"]
mod pixel;
#[allow(dead_code)]
#[path = "support/sdram/shared_pixel.rs"]
mod shared;
#[allow(dead_code)]
#[path = "support/texture.rs"]
mod texture;

const LIMIT: u64 = 200_000;

fn context(filter: Filter, second: bool, bypass: bool) -> CommonContext {
    CommonContext {
        lighting: LightingContext {
            material: Material {
                unlit: bypass,
                specular_color: [0; 3],
                shininess_code: 8,
            },
            light: Light {
                direction: [0, 0, 16384],
                ambient: if second { 64 } else { 32 },
                directional: if second { 128 } else { 192 },
            },
            projection: Projection::default(),
            epoch: if second { 23 } else { 11 },
        },
        sample: (!bypass).then_some(SampleContext {
            slot: 0,
            size_log2: 6,
            filter,
            bias_q8: 0,
        }),
        alpha: if second { 139 } else { 193 },
        rop: RopContext {
            depth: if second {
                DepthFunc::Always
            } else {
                DepthFunc::Less
            },
            depth_write: true,
            blend: if second {
                Blend::SrcOver
            } else {
                Blend::Replace
            },
        },
    }
}

fn inputs(
    ids: [gpu_v2::system::pixel::dispatch::ContextId; 2],
    count: usize,
    sparse: bool,
) -> Vec<Input> {
    (0..count)
        .map(|i| Input {
            context: ids[usize::from(i >= count / 2)],
            header: Header {
                x: ((i % 10) * 16 + (i / 20 % 2) * 2) as u16,
                y: (i / 10 % 2 * 16) as u8,
                mask: if sparse { [15, 5, 10, 3, 0][i % 5] } else { 15 },
            },
            basic: std::array::from_fn(|lane| Basic {
                tint: [(i * 23 + lane * 17) as u8, 181, (i * 7 + lane * 37) as u8],
                depth: (50000 - i * 13 - lane) as u16,
            }),
            light: [CompactPixelInput {
                normal: [0, 0, 1024],
                ndc: [0, 0],
            }; 4],
            uv_q16: std::array::from_fn(|lane| {
                [
                    (i % 4 * 8192 + lane % 2 * 1024) as i64,
                    (i % 4 * 4096 + lane / 2 * 1024) as i64,
                ]
            }),
            force_coarsest: i.is_multiple_of(17),
        })
        .collect()
}

fn goldens(
    inputs: &[Input],
    contexts: [CommonContext; 2],
    asset: Vec<u8>,
    slot: tex::Slot,
) -> Vec<[[u8; 3]; 4]> {
    let mut memory = texture::Image {
        bytes: asset,
        requests: Vec::new(),
    };
    let mut cache = texture_oracle::Cache::new(vec![slot]).unwrap();
    inputs
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let c = contexts[usize::from(i >= inputs.len() / 2)];
            let mut samples = [[255; 3]; 4];
            if let Some(s) = c.sample.filter(|_| q.header.mask != 0) {
                let quad = tex::QuadInput {
                    quad_id: 0,
                    mask: q.header.mask,
                    uv: q.uv_q16.map(|v| v.map(|x| x as f64 / 65536.0)),
                    slot: s.slot,
                    material_size_log2: s.size_log2,
                    filter: s.filter,
                    lod_bias: 0.0,
                    force_coarsest: q.force_coarsest,
                };
                for p in
                    texture_oracle::sample(&quad, &mut cache, &mut memory, tex::Config::counted())
                        .unwrap()
                        .pixels
                {
                    samples[usize::from(p.lane)] = p.rgb;
                }
            }
            samples
        })
        .collect()
}

fn golden_light(c: CommonContext) -> LightingOutput {
    // A unit +Z normal and +Z light have exact cosine 1; specular is disabled.
    LightingOutput {
        g: if c.lighting.material.unlit {
            256
        } else {
            c.lighting.light.ambient + c.lighting.light.directional
        },
        h: 0,
    }
}

struct Memory {
    bytes: Vec<u8>,
    pending: VecDeque<(u64, u64)>,
    active: Option<(u64, u64, usize)>,
    next: u64,
    wall: u64,
}
impl RefillPort for Memory {
    fn submit_read(&mut self, address: u64, bytes: usize) -> Result<u64, String> {
        if bytes != 128 || address & 127 != 0 || self.pending.len() >= 4 {
            return Err("fixture refill shape/credit".into());
        }
        let id = self.next;
        self.next += 1;
        self.pending.push_back((id, address));
        Ok(id)
    }
    fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
        self.wall += 1;
        if let Some((id, address, beat)) = self.active {
            if beat == 16 {
                self.active = None;
                return Ok(vec![RefillEvent::Complete { id }]);
            }
            if self.wall % 3 != 1 {
                let offset =
                    usize::try_from(address - u64::from(texture::BASE)).unwrap() + beat * 8;
                let bytes = self.bytes.get(offset..offset + 8).ok_or("fixture bounds")?;
                self.active = Some((id, address, beat + 1));
                return Ok(vec![RefillEvent::Beat {
                    id,
                    index: beat,
                    data: u64::from_le_bytes(bytes.try_into().unwrap()),
                    last: beat == 15,
                }]);
            }
        } else if let Some((id, address)) = self.pending.pop_front() {
            self.active = Some((id, address, 0));
            return Ok(vec![RefillEvent::Started { id }]);
        }
        Ok(vec![])
    }
}

fn tick(wall: usize, input: Option<foundation::Beat>, stall: bool) -> foundation_live::Tick {
    foundation_live::Tick {
        ce: !stall || wall % 17 != 3 && wall % 17 != 4,
        input,
        lighting_issue_ready: !stall || wall % 13 != 6,
        lighting_result_ready: !stall || wall % 7 != 2,
        sampling_issue_ready: !stall || wall % 19 != 7,
        sampling_result_ready: !stall || wall % 5 != 1,
        final_issue_ready: !stall || wall % 23 != 10,
        final_result_ready: !stall || wall % 29 < 23,
        rop_ready: !stall || wall % 11 < 7,
    }
}

#[test]
fn actual_selected_branches_to_rop_input_all_filters_and_two_draws() {
    for filter in [Filter::Nearest, Filter::Bilinear, Filter::Trilinear] {
        for stall in [false, true] {
            let slot = texture::slot(6, true);
            let asset = texture::asset(slot, texture::pattern);
            let mut memory = Memory {
                bytes: asset.clone(),
                pending: VecDeque::new(),
                active: None,
                next: 0,
                wall: 0,
            };
            let mut live = Live::new(&[slot], LIMIT).unwrap();
            let contexts = [context(filter, false, false), context(filter, true, stall)];
            let ids = [
                live.open_draw(contexts[0]).unwrap().unwrap(),
                live.open_draw(contexts[1]).unwrap().unwrap(),
            ];
            assert!(live.open_draw(contexts[0]).unwrap().is_none());
            let inputs = inputs(ids, 64, stall);
            let samples = goldens(&inputs, contexts, asset, slot);
            let beats: Vec<_> = inputs
                .iter()
                .map(|&q| foundation::source_beats(q).unwrap())
                .collect();
            let mut source = 0;
            let mut row = 0;
            let mut closed = [false; 2];
            let mut owners = BTreeMap::new();
            let mut received = Vec::new();
            let mut context_waits = 0;
            let mut final_edges = Vec::new();
            let mut output_edges = Vec::new();
            let mut events = Vec::new();
            for wall in 0..LIMIT as usize {
                let offer = (source < 64 && wall >= 16 + source * 8).then(|| beats[source][row]);
                let t = tick(wall, offer, stall);
                let step = live.step(&mut memory, t).unwrap_or_else(|e| {
                    panic!("filter {filter:?} wall {wall} source {source}: {e}")
                });
                if let Some(ticket) = step.pixels.published {
                    owners.insert(ticket.serial, source);
                }
                if step.pixels.input_accepted {
                    row += 1;
                    if row == 8 {
                        row = 0;
                        source += 1;
                    }
                }
                if step.lighting_context_wait {
                    context_waits += 1;
                }
                if let Some(o) = step
                    .lighting
                    .output
                    .filter(|_| t.ce && t.lighting_result_ready)
                {
                    let ticket = live.pipeline().ticket((o.id / 4) as u8).unwrap();
                    let index = owners[&ticket.serial];
                    assert_eq!(o.output, golden_light(contexts[usize::from(index >= 32)]));
                }
                if step.pixels.final_stage.accepted {
                    final_edges.push(wall);
                    let key = step.pixels.final_accepted.unwrap();
                    events.push((
                        "final_accepted",
                        wall,
                        owners[&key.ticket.serial],
                        key.lane,
                        live.sampling().cache_stats().refills,
                    ));
                }
                if t.ce && t.rop_ready {
                    if let Some(r) = step.pixels.signals.rop {
                        let index = owners[&r.ticket.serial];
                        let q = inputs[index];
                        let c = contexts[usize::from(index >= 32)];
                        let lane = usize::from(r.row / 2);
                        let expected = if q.header.mask & (1 << lane) == 0 {
                            0
                        } else if r.row & 1 == 0 {
                            let rgb = pixel::rgb(
                                q.basic[lane].tint,
                                samples[index][lane],
                                golden_light(c),
                                c.lighting.material.specular_color,
                            );
                            u32::from_le_bytes([rgb[0], rgb[1], rgb[2], c.alpha])
                        } else {
                            u32::from(q.basic[lane].depth)
                        };
                        assert_eq!(
                            r.data, expected,
                            "actual branch result quad {index} row {}",
                            r.row
                        );
                        received.push((index, r.row));
                        output_edges.push(wall);
                        events.push((
                            "rop_row",
                            wall,
                            index,
                            r.row,
                            live.sampling().cache_stats().refills,
                        ));
                    }
                }
                if source >= 32 && !closed[0] {
                    live.close_draw(ids[0]).unwrap();
                    closed[0] = true;
                }
                if source == 64 && !closed[1] {
                    live.close_draw(ids[1]).unwrap();
                    closed[1] = true;
                }
                if live.idle() && source == 64 {
                    break;
                }
            }
            assert!(live.idle());
            let expected: Vec<_> = inputs
                .iter()
                .enumerate()
                .filter(|(_, q)| q.header.mask != 0)
                .flat_map(|(i, _)| (0..8u8).map(move |r| (i, r)))
                .collect();
            assert_eq!(received, expected);
            assert_eq!(
                live.sampling().stats.admissions,
                inputs
                    .iter()
                    .enumerate()
                    .filter(|(i, q)| {
                        q.header.mask != 0 && contexts[usize::from(*i >= 32)].sample.is_some()
                    })
                    .count() as u64
            );
            assert_eq!(live.sampling().stats.compilations, 0);
            assert_eq!(live.sampling().stats.peak_preparation_programs, 0);
            if !stall {
                assert!(
                    context_waits > 0,
                    "actual Lighting context switch must drain locally"
                );
            }
            let gaps = |edges: &[usize]| {
                let mut map = BTreeMap::new();
                for w in edges.windows(2) {
                    *map.entry(w[1] - w[0]).or_insert(0usize) += 1;
                }
                map
            };
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/pixel-foundation-20261007");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("live-{filter:?}-{stall}.txt")),format!("Lighting: selected Fast/free/Floor Q13; local context drain edges={context_waits}\nFinal accept gaps={:?}\nROP-input row gaps={:?}\nSampling cache stats={:?}\n",gaps(&final_edges),gaps(&output_edges),live.sampling().cache_stats())).unwrap();
            let mut csv = String::from("event,wall,quad,lane_or_row,refills\n");
            for &(event, wall, quad, row, refills) in &events {
                csv.push_str(&format!("{event},{wall},{quad},{row},{refills}\n"));
            }
            std::fs::write(dir.join(format!("live-{filter:?}-{stall}.csv")), csv).unwrap();
        }
    }
}

#[test]
fn actual_serial_rop_cache_shared_mc_complete_image_and_backpressure() {
    let filter = Filter::Trilinear;
    let slot = texture::slot(6, true);
    let asset = texture::asset(slot, texture::pattern);
    let surface = MaterializedSurface {
        color_base_bytes: 0x8000,
        depth_base_bytes: 0xb000,
        width: 160,
        height: 32,
    };
    let mut initial: Vec<u8> = (0..0x21000)
        .map(|i| ((i * 71 + 19) ^ (i >> 3)) as u8)
        .collect();
    initial[texture::BASE as usize..texture::BASE as usize + asset.len()].copy_from_slice(&asset);
    let image =
        unsafe { OracleImage::from_host(0, initial.clone(), "published pixel backend").unwrap() };
    let hub = shared::Shared::new(
        image,
        (
            u64::from(texture::BASE),
            u64::from(texture::BASE) + asset.len() as u64,
        ),
        0x20000,
        true,
        LIMIT,
    )
    .unwrap();
    let (mut ro, mut fb) = hub.views();
    let mut backend = Backend::new(&[slot], surface, LIMIT).unwrap();
    let contexts = [context(filter, false, false), context(filter, true, true)];
    let ids = [
        backend.open_draw(contexts[0]).unwrap().unwrap(),
        backend.open_draw(contexts[1]).unwrap().unwrap(),
    ];
    let inputs = inputs(ids, 36, true);
    let samples = goldens(&inputs, contexts, asset, slot);
    let mut golden = initial;
    for (i, q) in inputs.iter().enumerate() {
        let c = contexts[usize::from(i >= 18)];
        let stimulus = pixel::Stimulus {
            quad: gpu_v2::system::pixel::QuadInput {
                header: q.header,
                basic: q.basic,
                default_light: c.lighting.material.unlit,
                default_sample: c.sample.is_none(),
            },
            light: [golden_light(c); 4],
            sample: samples[i],
        };
        golden = pixel::golden(
            golden,
            &[stimulus],
            gpu_v2::system::pixel::Context {
                surface,
                rop: c.rop,
                specular: c.lighting.material.specular_color,
                alpha: c.alpha,
            },
        );
    }
    let beats: Vec<_> = inputs
        .iter()
        .map(|&q| foundation::source_beats(q).unwrap())
        .collect();
    let mut source = 0;
    let mut row = 0;
    let mut closed = [false; 2];
    let mut rop_stalls = 0;
    let mut context_stalls = 0;
    let mut clocks = 0;
    for wall in 0..LIMIT as usize {
        let offer = (source < 36 && wall >= 16 + source * 4).then(|| beats[source][row]);
        let t = tick(wall, offer, true);
        let step = backend
            .step(&mut ro, &mut fb, t)
            .unwrap_or_else(|e| panic!("shared MC wall {wall} source {source}: {e}"));
        clocks += 1;
        if step.pixels.pixels.input_accepted {
            row += 1;
            if row == 8 {
                row = 0;
                source += 1;
            }
        }
        if t.ce && step.pixels.pixels.signals.rop.is_some() && !step.framebuffer.input_accepted {
            rop_stalls += 1;
        }
        context_stalls += usize::from(step.context_stall);
        if source >= 18 && !closed[0] {
            backend.close_draw(ids[0]).unwrap();
            closed[0] = true;
        }
        if source == 36 && !closed[1] {
            backend.close_draw(ids[1]).unwrap();
            backend.request_finish().unwrap();
            closed[1] = true;
        }
        if backend.complete() {
            hub.stop_background();
            if hub.idle() {
                break;
            }
        }
    }
    assert!(backend.complete() && hub.idle());
    assert_eq!(backend.pixels().sampling().stats.compilations, 0);
    assert_eq!(
        backend.pixels().sampling().stats.peak_preparation_programs,
        0
    );
    assert!(!backend.faulted() && !hub.physical_error());
    assert_eq!(
        hub.image(),
        golden,
        "full image/guards differ after actual ROP flush"
    );
    assert!(
        rop_stalls > 0,
        "actual serial ROP backpressure was not exercised"
    );
    assert!(context_stalls > 0);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-foundation-20261007/shared-mc");
    hub.save(&dir, "foundation");
    std::fs::write(dir.join("summary.txt"),format!("actual serial ROP/cache: wall clocks={clocks}; row stalls={rop_stalls}; context stalls={context_stalls}\n{:?}\n",hub.stats())).unwrap();
}

#[test]
fn reused_lighting_bank_reloads_context_and_empty_draw_does_not_finish_frame() {
    let slot = texture::slot(6, true);
    let asset = texture::asset(slot, texture::pattern);
    let mut memory = Memory {
        bytes: asset,
        pending: VecDeque::new(),
        active: None,
        next: 0,
        wall: 0,
    };
    let mut live = Live::new(&[slot], LIMIT).unwrap();
    let mut wall = 0;
    // Same bank repeatedly changes its lighting/alpha. No sampled operand is
    // involved, so every actual output independently identifies the new draw.
    for draw in 0..6 {
        let mut c = context(Filter::Nearest, draw & 1 != 0, false);
        c.sample = None;
        let id = live.open_draw(c).unwrap().unwrap();
        assert_eq!(id.slot, 0);
        let q = inputs([id; 2], 2, false)[0];
        let beats = foundation::source_beats(q).unwrap();
        let mut source_row = 0;
        let mut received = 0;
        for _ in 0..1024 {
            let t = tick(wall, (source_row < 8).then(|| beats[source_row]), false);
            let step = live.step(&mut memory, t).unwrap();
            wall += 1;
            if step.pixels.input_accepted {
                source_row += 1;
                if source_row == 8 {
                    live.close_draw(id).unwrap();
                }
            }
            if let Some(r) = step.pixels.signals.rop {
                let lane = usize::from(r.row / 2);
                let expected = if r.row & 1 == 0 {
                    let rgb = pixel::rgb(q.basic[lane].tint, [255; 3], golden_light(c), [0; 3]);
                    u32::from_le_bytes([rgb[0], rgb[1], rgb[2], c.alpha])
                } else {
                    u32::from(q.basic[lane].depth)
                };
                assert_eq!(r.data, expected, "reused bank draw {draw} row {}", r.row);
                received += 1;
            }
            if live.idle() {
                break;
            }
        }
        assert!(live.idle());
        assert_eq!(received, 8);
    }
    struct EmptyMemory {
        clocks: usize,
    }
    impl gpu_v2::memory::ports::MemoryPort for EmptyMemory {
        fn cycle(
            &mut self,
            request: Option<gpu_v2::memory::ports::Request>,
            write: Option<u64>,
        ) -> Result<gpu_v2::memory::ports::Response, String> {
            assert!(
                request.is_none() && write.is_none(),
                "empty frame must not issue memory"
            );
            self.clocks += 1;
            Ok(Default::default())
        }
    }
    let mut physical = EmptyMemory { clocks: 0 };
    let mut backend = Backend::new(
        &[slot],
        MaterializedSurface {
            color_base_bytes: 0,
            depth_base_bytes: 0x2000,
            width: 16,
            height: 16,
        },
        128,
    )
    .unwrap();
    let c = context(Filter::Nearest, false, true);
    for draw in 0..3 {
        let id = backend.open_draw(c).unwrap().unwrap();
        backend.close_draw(id).unwrap();
        backend
            .step(&mut memory, &mut physical, tick(draw, None, false))
            .unwrap();
        assert!(!backend.complete(), "draw boundary is not frame finish");
    }
    backend.request_finish().unwrap();
    assert!(backend.open_draw(c).is_err());
    for edge in 0..32 {
        backend
            .step(&mut memory, &mut physical, tick(edge, None, false))
            .unwrap();
        if backend.complete() {
            break;
        }
    }
    assert!(backend.complete());
    assert!(physical.clocks >= 4);
}

#[test]
fn actual_fixed_footprint_single_draw_checks_all_pixels_and_reports_live_gaps() {
    fixed_footprint_stream(false, false);
}

#[test]
fn actual_one_group_warm_nearest_bilinear_stream_has_ii2_to_rop() {
    fixed_footprint_stream(true, true);
}

#[test]
#[ignore = "retained cross-tile Bilinear current-cadence counterexample; not a universal II2 claim"]
fn actual_original_cross_tile_bilinear_ii2_counterexample() {
    fixed_footprint_stream(true, false);
}

fn fixed_footprint_stream(require_ii2: bool, one_group: bool) {
    let filters: &[Filter] = if one_group {
        &[Filter::Nearest, Filter::Bilinear]
    } else {
        &[Filter::Nearest, Filter::Bilinear, Filter::Trilinear]
    };
    for &filter in filters {
        let slot = texture::slot(6, true);
        let asset = texture::asset(slot, texture::pattern);
        let mut memory = Memory {
            bytes: asset.clone(),
            pending: VecDeque::new(),
            active: None,
            next: 0,
            wall: 0,
        };
        let mut live = Live::new(&[slot], LIMIT).unwrap();
        let c = context(filter, false, false);
        let id = live.open_draw(c).unwrap().unwrap();
        let mut inputs = inputs([id; 2], 64, false);
        for q in &mut inputs {
            q.force_coarsest = false;
            // Trilinear deliberately uses a fractional LOD (1.5 texels),
            // rather than relabeling the single-plane LOD0 bilinear workload.
            let delta = if filter == Filter::Trilinear {
                1536
            } else {
                1024
            };
            q.uv_q16 = std::array::from_fn(|lane| {
                [
                    4096 + (lane % 2) as i64 * delta,
                    (if one_group { 4096 } else { 8192 }) + (lane / 2) as i64 * delta,
                ]
            });
        }
        let samples = goldens(&inputs, [c; 2], asset, slot);
        let beats: Vec<_> = inputs
            .iter()
            .map(|&q| foundation::source_beats(q).unwrap())
            .collect();
        let mut source = 0;
        let mut row = 0;
        let mut owners = BTreeMap::new();
        let mut warm_final = Vec::new();
        let mut warm_rows = Vec::new();
        let mut received = 0;
        let mut alias_waits = 0;
        let mut trace = String::from("event,wall,quad,lane_or_row,refills\n");
        let mut lifecycle = String::from("event,wall,quad,lane_or_row\n");
        let mut capacity = String::from("wall,source_quad,source_row,offered,accepted,input_ready,global_before,global_after,light_input,sample_input,global_full,light_full,sample_full\n");
        for wall in 0..LIMIT as usize {
            let t = tick(
                wall,
                (source < 64 && wall >= 16 + source * 8).then(|| beats[source][row]),
                false,
            );
            let before = live.pipeline().capacity_snapshot();
            let source_index = source;
            let source_row = row;
            let step = live.step(&mut memory, t).unwrap();
            alias_waits += usize::from(step.sampling_alias_wait);
            capacity.push_str(&format!(
                "{wall},{source_index},{source_row},{},{},{},{},{},{},{},{},{},{}\n",
                u8::from(t.input.is_some()),
                u8::from(step.pixels.input_accepted),
                u8::from(step.pixels.signals.input_ready),
                before.global,
                live.pipeline().live_status(),
                before.lighting_input,
                before.sampling_input,
                u8::from(before.global == foundation::GLOBAL_SLOTS),
                u8::from(before.lighting_input == 2),
                u8::from(before.sampling_input == 8)
            ));
            let mut record = |event: &str, index: usize, lane: u8| {
                lifecycle.push_str(&format!("{event},{wall},{index},{lane}\n"));
            };
            if let Some(ticket) = step.pixels.published {
                assert_eq!(ticket.quad as usize, source % foundation::GLOBAL_SLOTS);
                owners.insert(ticket.serial, source);
                record("published", source, 7);
            }
            if step.pixels.input_accepted {
                if row == 0 {
                    record("source_first", source, 0);
                }
                row += 1;
                if row == 8 {
                    row = 0;
                    source += 1;
                    if source == 64 {
                        live.close_draw(id).unwrap();
                    }
                }
            }
            if step.pixels.lighting_accepted {
                let key = step.pixels.signals.lighting.unwrap().key;
                record("lighting_admit", owners[&key.ticket.serial], key.lane);
            }
            if step.pixels.sampling_accepted {
                assert!(
                    !step.sampling_alias_wait,
                    "admission borrowed aliased release"
                );
                let ticket = step.pixels.signals.sampling.unwrap().ticket;
                record("sampling_admit", owners[&ticket.serial], 0);
            }
            if let Some(o) = step.lighting.output {
                let ticket = live.pipeline().ticket((o.id / 4) as u8).unwrap();
                record("lighting_result", owners[&ticket.serial], (o.id % 4) as u8);
            }
            if let Some(write) = step.sample_written {
                record(
                    "sampling_result",
                    owners[&write.key.ticket.serial],
                    write.key.lane,
                );
            }
            for event in &step.sampling.preparation.events {
                use gpu_v2::texture::sim::staged::bound::control::Event;
                let (name, public_quad) = match event {
                    Event::SharedRelease { program, .. } => {
                        ("sampling_shared_release", *program as u8)
                    }
                    Event::Release { program } => ("sampling_preparation_release", *program as u8),
                    _ => continue,
                };
                let ticket = live
                    .sampling_destination(public_quad)
                    .or_else(|| {
                        step.sample_written
                            .filter(|w| w.key.ticket.quad & 15 == public_quad)
                            .map(|w| w.key.ticket)
                    })
                    .expect("preparation release lost public destination witness");
                record(name, owners[&ticket.serial], 0);
            }
            for access in &step.pixels.accesses {
                let event = match (access.store, access.write) {
                    (foundation::Store::Basic, false) => Some("join_read"),
                    (foundation::Store::Output, true) if access.address % 8 == 7 => {
                        Some("output_published")
                    }
                    _ => None,
                };
                if let Some(event) = event {
                    let ticket = live.pipeline().ticket((access.address / 8) as u8).unwrap();
                    record(event, owners[&ticket.serial], (access.address % 8) as u8);
                }
            }
            if step.pixels.final_stage.accepted {
                let key = step.pixels.final_accepted.unwrap();
                let index = owners[&key.ticket.serial];
                record("final_accepted", index, key.lane);
                if index >= 16 {
                    warm_final.push(wall);
                }
                trace.push_str(&format!(
                    "final_accepted,{wall},{index},{},{}\n",
                    key.lane,
                    live.sampling().cache_stats().refills
                ));
            }
            if let Some(r) = step.pixels.signals.rop {
                let index = owners[&r.ticket.serial];
                record("rop_row", index, r.row);
                let lane = usize::from(r.row / 2);
                let q = inputs[index];
                let expected = if r.row & 1 == 0 {
                    let rgb = pixel::rgb(
                        q.basic[lane].tint,
                        samples[index][lane],
                        golden_light(c),
                        [0; 3],
                    );
                    u32::from_le_bytes([rgb[0], rgb[1], rgb[2], c.alpha])
                } else {
                    u32::from(q.basic[lane].depth)
                };
                assert_eq!(r.data, expected, "warm {filter:?} quad{index} row{}", r.row);
                assert_eq!(r.row as usize, received % 8);
                assert_eq!(index, received / 8);
                received += 1;
                if index >= 16 {
                    warm_rows.push(wall);
                }
                trace.push_str(&format!(
                    "rop_row,{wall},{index},{},{}\n",
                    r.row,
                    live.sampling().cache_stats().refills
                ));
            }
            if let Some(ticket) = step.pixels.retired {
                record("status_retired", owners[&ticket.serial], 7);
            }
            if live.idle() && source == 64 {
                break;
            }
        }
        assert!(live.idle());
        assert_eq!(received, 512);
        assert_eq!(warm_final.len(), 48 * 4);
        assert_eq!(warm_rows.len(), 48 * 8);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/pixel-foundation-20261007");
        let name = if one_group {
            format!("warm-one-group-{filter:?}")
        } else {
            format!("warm-{filter:?}")
        };
        std::fs::write(dir.join(format!("{name}.csv")), trace).unwrap();
        std::fs::write(dir.join(format!("{name}-lifecycle.csv")), lifecycle).unwrap();
        std::fs::write(dir.join(format!("{name}-capacity.csv")), capacity).unwrap();
        assert_eq!(live.sampling().stats.admissions, 64);
        assert_eq!(live.sampling().stats.compilations, 0);
        assert!(
            alias_waits > 0,
            "public ID alias backpressure was not exercised"
        );
        if one_group {
            assert_eq!(
                live.sampling().stats.link.packets,
                256,
                "one packet group per pixel"
            );
            assert_eq!(live.sampling().stats.link.captures, 256);
            assert_eq!(
                live.sampling().cache_stats().refills,
                1,
                "only initial cold tile refill"
            );
        }
        // A fixed warm footprint is not sufficient to declare end-to-end II2:
        // the finite integrated owners can still leave holes between cohorts.
        // Keep every measured interval, including those holes. The controlled
        // branch test is the separate sustained row/calendar certificate.
        let gaps = |edges: &[usize]| {
            let mut histogram = BTreeMap::new();
            for w in edges.windows(2) {
                *histogram.entry(w[1] - w[0]).or_insert(0usize) += 1;
            }
            histogram
        };
        let final_gaps = gaps(&warm_final);
        let rop_gaps = gaps(&warm_rows);
        std::fs::write(dir.join(format!("{name}.txt")), format!(
            "Predeclared quads16..63, one draw, full mask, fixed footprint. Every pixel/row checked independently. one_group={one_group}; Trilinear uses fractional two-plane LOD.\nFinal acceptance gaps={final_gaps:?}\nROP-input row gaps={rop_gaps:?}\nSampling alias wait edges={alias_waits}\nSampling stats={:?}\n", live.sampling().stats
        )).unwrap();
        if require_ii2 && filter != Filter::Trilinear {
            for w in warm_final.windows(2) {
                assert_eq!(w[1] - w[0], 2, "whole warm {filter:?} Final gap");
            }
            for w in warm_rows.windows(2) {
                assert_eq!(w[1] - w[0], 1, "whole warm {filter:?} ROP row gap");
            }
        }
    }
}
