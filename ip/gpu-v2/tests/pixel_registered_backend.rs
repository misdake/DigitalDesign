type Backend = gpu_v2::system::pixel::registered_backend::Backend<RefillBridge<shared::RoView>>;
// Full register-only sampler backend; independent shared physical MC/guards.
use digital_design_hardware_gowin::sdram_memory_controller::ports::OracleImage;
use gpu_v2::{
    framebuffer::ports::{Blend, Context as RopContext, DepthFunc, Header},
    lighting::{
        ports::*,
        sim::{counted, oracle},
        LightingProfile, LightingQuantization,
    },
    system::pixel::{composition, dispatch::*, Basic},
    texture::{
        emu::refill_bridge::RefillBridge,
        ports::{self as tex},
        sim::oracle as tex_oracle,
        sim::staged::bound::serial,
    },
};
#[allow(dead_code)]
#[path = "support/pixel.rs"]
mod pixel;
#[allow(dead_code)]
#[path = "support/sdram/shared_pixel.rs"]
mod shared;
#[allow(dead_code)]
#[path = "support/texture.rs"]
mod support;
fn context(mode: usize) -> CommonContext {
    CommonContext {
        lighting: LightingContext {
            material: Material {
                unlit: mode & 1 != 0,
                specular_color: [31, 17, 9],
                shininess_code: 8,
            },
            light: Light {
                direction: [0, 0, 16384],
                ambient: 32,
                directional: 192,
            },
            projection: Projection::default(),
            epoch: mode as u16 + 11,
        },
        sample: (mode & 2 == 0).then_some(SampleContext {
            slot: 0,
            size_log2: 6,
            filter: tex::Filter::Trilinear,
            bias_q8: 64,
        }),
        alpha: 193,
        rop: RopContext {
            depth: [
                DepthFunc::Less,
                DepthFunc::Always,
                DepthFunc::Greater,
                DepthFunc::Always,
            ][mode],
            depth_write: mode != 2,
            blend: if mode & 1 == 0 {
                Blend::Replace
            } else {
                Blend::SrcOver
            },
        },
    }
}

#[test]
fn actual_branches_final_rop_cache_shared_mc_complete_image() {
    connected_case(false);
}
#[test]
fn actual_raster_quad_to_framebuffer_shared_mc_complete_image() {
    connected_case(true);
}
fn connected_case(raster_scene: bool) {
    const LIMIT: u64 = 200_000;
    let slots = [support::slot(6, true)];
    let asset = support::asset(slots[0], support::pattern);
    let surface = gpu_v2::framebuffer::ports::MaterializedSurface {
        color_base_bytes: 0x8000,
        depth_base_bytes: 0xb000,
        width: 160,
        height: 32,
    };
    let mut initial: Vec<u8> = (0..0x21000)
        .map(|i| ((i * 71 + 19) ^ (i >> 3)) as u8)
        .collect();
    initial[support::BASE as usize..support::BASE as usize + asset.len()].copy_from_slice(&asset);
    let mut golden_mem = support::Image {
        bytes: asset,
        requests: vec![],
    };
    let mut golden_cache = tex_oracle::Cache::new(slots.to_vec()).unwrap();
    // SAFETY: test-only raw byte initialization, independent of numerical DUT results.
    let image =
        unsafe { OracleImage::from_host(0, initial.clone(), "actual pixel backend").unwrap() };
    let hub = shared::Shared::new(
        image,
        (
            u64::from(support::BASE),
            u64::from(support::BASE) + golden_mem.bytes.len() as u64,
        ),
        0x20000,
        true,
        LIMIT,
    )
    .unwrap();
    let (ro, mut fb) = hub.views();
    let mut backend = Backend::new(
        Config {
            max_wall: LIMIT,
            ..Default::default()
        },
        slots.to_vec(),
        RefillBridge::new(ro),
        serial::Config {
            nearest_bypass: true,
            short_alignment: true,
        },
        surface,
    )
    .unwrap();
    let ids = std::array::from_fn::<_, 4, _>(|i| backend.set_context(i as u8, context(i)).unwrap());
    let mut inputs: Vec<_> = (0..36)
        .map(|i| Input {
            context: ids[i % 4],
            header: Header {
                x: (i % 10 * 16 + (i / 20 % 2) * 2) as u16,
                y: (i / 10 % 2 * 16) as u8,
                mask: [15, 5, 10, 3, 0][i % 5],
            },
            basic: std::array::from_fn(|lane| Basic {
                tint: [(i * 23 + lane) as u8, 181, (i * 7 + lane * 17) as u8],
                depth: (50000 - i * 13 - lane) as u16,
            }),
            light: std::array::from_fn(|lane| CompactPixelInput {
                normal: [[0, 0, 1024], [256, 384, 921], [0, 0, 0], [-1024, 0, 0]][lane],
                ndc: [i as i32 * 1024 - 16384, lane as i32 * 8192],
            }),
            uv_q18: std::array::from_fn(|lane| {
                [
                    i as i64 * 4096 - 131072 + (lane as i64 & 1) * 8191,
                    i as i64 * 2027 + (lane as i64 >> 1) * 4096,
                ]
            }),
        })
        .collect();
    if raster_scene {
        inputs = raster_inputs(&backend, ids);
    }
    let mut goldens = Vec::new();
    for q in &inputs {
        let c = context(q.context.slot as usize);
        let light_cfg = oracle::Config::from_counted(counted::Config::lit_queue_resource_profile(
            LightingProfile::Fast,
            LightingQuantization::CompensatedFloor,
        ));
        let lighting = std::array::from_fn::<_, 4, _>(|lane| {
            if c.lighting.material.unlit {
                LightingOutput { g: 256, h: 0 }
            } else {
                oracle::evaluate_output(
                    q.light[lane].expanded().unwrap(),
                    c.lighting.material,
                    c.lighting.light,
                    c.lighting.projection,
                    light_cfg,
                )
                .unwrap()
            }
        });
        let mut colors = [[255; 3]; 4];
        if let Some(s) = c.sample.filter(|_| q.header.mask != 0) {
            let quad = tex::QuadInput {
                quad_id: 0,
                mask: q.header.mask,
                uv: q.uv_q18.map(|v| v.map(|x| x as f64 / 262144.0)),
                slot: s.slot,
                material_size_log2: s.size_log2,
                filter: s.filter,
                lod_bias: f64::from(s.bias_q8) / 256.0,
            };
            for p in tex_oracle::sample(
                &quad,
                &mut golden_cache,
                &mut golden_mem,
                tex::Config::counted(),
            )
            .unwrap()
            .pixels
            {
                colors[usize::from(p.lane)] = p.rgb;
            }
        }
        goldens.push((lighting, colors));
    }
    let mut expected = initial;
    for (i, q) in inputs.iter().enumerate() {
        let c = context(q.context.slot as usize);
        expected = pixel::golden(
            expected,
            &[pixel::Stimulus {
                quad: gpu_v2::system::pixel::QuadInput {
                    header: q.header,
                    basic: q.basic,
                    default_light: c.lighting.material.unlit,
                    default_sample: c.sample.is_none(),
                },
                light: goldens[i].0,
                sample: goldens[i].1,
            }],
            gpu_v2::system::pixel::Context {
                surface,
                rop: c.rop,
                specular: c.lighting.material.specular_color,
                alpha: c.alpha,
            },
        );
    }
    let mut offered = 0;
    let mut rows = 0;
    let mut done = false;
    let mut ce0_memory = false;
    for wall in 0..LIMIT {
        let ce = wall % 13 < 10;
        ce0_memory |= !ce && (hub.ro_active() || hub.fb_write_active());
        let step = backend
            .step(
                &mut fb,
                composition::Tick {
                    ce,
                    input: inputs.get(offered).copied(),
                    lighting_result_ready: wall % 17 != 0,
                    sampling_issue_ready: wall % 19 != 0,
                    sampling_result_ready: wall % 23 != 0,
                    final_issue_ready: wall % 7 != 0,
                    final_result_ready: wall % 31 < 21,
                    rop_ready: wall % 29 < 19,
                    finish: offered == inputs.len(),
                },
            )
            .unwrap();
        if step.pixels.dispatch.input_accepted {
            offered += 1;
        }
        if step.framebuffer.input_accepted {
            rows += 1;
        }
        if backend.complete() {
            done = true;
            break;
        }
    }
    assert!(done && hub.gpu_idle(), "full backend watchdog");
    assert_eq!(offered, inputs.len());
    assert_eq!(
        rows,
        inputs.iter().filter(|q| q.header.mask != 0).count() * 8
    );
    assert_eq!(
        hub.image(),
        expected,
        "whole image including guards and texture"
    );
    let stats = hub.stats();
    assert!(stats.ro_beats > 0 && stats.fb_read_beats > 0 && stats.fb_write_beats > 0);
    assert!(stats.bg_completed.iter().all(|x| *x > 0));
    assert!(ce0_memory);
    assert_eq!(stats.fb_read_beats, stats.fb_reads * 16);
    assert_eq!(stats.fb_write_beats, stats.fb_writes * 16);
    assert_eq!(stats.fb_terminals, stats.fb_reads + stats.fb_writes);
    hub.save(
        std::path::Path::new("target/gpu-overnight-20261006"),
        if raster_scene {
            "registered-backend-raster"
        } else {
            "registered-backend-shared"
        },
    );
}

use gpu_v2::system::oracle::ports::RasterQuad;
use std::collections::BTreeMap;
fn backend_raster() -> Vec<RasterQuad> {
    let report = gpu_v2::triangle::sim::oracle::run_continuous(
        7,
        [
            [-0.8, -0.8, 0.0, 1.0],
            [0.8, -0.8, 0.0, 1.0],
            [-0.8, 0.8, 0.0, 1.0],
        ],
        [
            [0.0, 0.0, 0.2, 0.4, 0.6, 0.001, 0.2, 1.0],
            [1.0, 0.0, 0.9, 0.4, 0.6, 0.001, 0.2, 1.0],
            [0.0, 1.0, 0.2, 0.8, 0.6, 0.001, 0.2, 1.0],
        ],
        gpu_v2::triangle::ports::Config {
            width: 16,
            height: 16,
            max_samples: 256,
            ..Default::default()
        },
    )
    .unwrap();
    let mut coverage = BTreeMap::<[u16; 2], u8>::new();
    for (xy, _, _) in report.rasterize().unwrap() {
        *coverage.entry([xy[0] & !1, xy[1] & !1]).or_default() |=
            1 << ((xy[0] & 1) + 2 * (xy[1] & 1));
    }
    coverage
        .into_iter()
        .map(|(xy, mask)| RasterQuad {
            triangle: 7,
            xy,
            mask,
            invalid_helpers: 0,
            samples: std::array::from_fn(|lane| {
                report
                    .evaluate([
                        f64::from(xy[0]) + 0.5 + (lane % 2) as f64,
                        f64::from(xy[1]) + 0.5 + (lane / 2) as f64,
                    ])
                    .unwrap()
            }),
        })
        .collect()
}

fn raster_inputs(backend: &Backend, ids: [ContextId; 4]) -> Vec<Input> {
    backend_raster()
        .into_iter()
        .enumerate()
        .map(|(i, q)| {
            gpu_v2::system::pixel::raster_input::convert(
                backend.pixels().dispatch(),
                ids[i % 4],
                q,
                [16, 16],
            )
            .unwrap()
            .0
        })
        .collect()
}
