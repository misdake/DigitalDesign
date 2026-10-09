//! Dispatcher/real branches/Final with one actual Combination clock owner.
//! ROP row consumption is controlled here, not framebuffer-cache proof.
use digital_design_hardware_gowin::sdram_memory_controller::ports::OracleImage;
use gpu_v2::{
    framebuffer::ports::{Blend, Context, DepthFunc, Header},
    lighting::ports::{CompactPixelInput, Light, LightingContext, Material, Projection},
    memory::ports::MemoryPort,
    system::pixel::{
        composition::{FinalBranches, Tick},
        dispatch::{CommonContext, Config, Input, SampleContext},
        final_rgb, Basic,
    },
    texture::{ports::Filter, sim::oracle},
};
#[allow(dead_code)]
#[path = "support/sdram/shared_pixel.rs"]
mod shared;
#[allow(dead_code)]
#[path = "support/texture.rs"]
mod texture;

#[test]
fn shared_clock_follows_sampling_poll_on_every_wall_edge() {
    const LIMIT: u64 = 40_000;
    let slot = texture::slot(5, true);
    let asset = texture::asset(slot, texture::pattern);
    let mut initial: Vec<_> = (0..0x21000).map(|i| (i * 13 + 77) as u8).collect();
    initial[texture::BASE as usize..texture::BASE as usize + asset.len()].copy_from_slice(&asset);
    // SAFETY: test-only immutable byte stimulus, no DUT numerical answers.
    let image = unsafe {
        OracleImage::from_host(0, initial.clone(), "dispatcher shared-clock stimulus").unwrap()
    };
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
    let mut engines = FinalBranches::new(
        Config {
            max_wall: LIMIT,
            ..Default::default()
        },
        &[slot],
    )
    .unwrap();
    let context = engines
        .set_context(
            0,
            CommonContext {
                lighting: LightingContext {
                    material: Material {
                        unlit: true,
                        shininess_code: 0,
                        specular_color: [0; 3],
                    },
                    light: Light {
                        direction: [0, 0, 16384],
                        ambient: 0,
                        directional: 0,
                    },
                    projection: Projection::default(),
                    epoch: 1,
                },
                sample: Some(SampleContext {
                    slot: 0,
                    size_log2: 5,
                    filter: Filter::Trilinear,
                    bias_q8: 32,
                }),
                alpha: 193,
                rop: Context {
                    depth: DepthFunc::Always,
                    depth_write: true,
                    blend: Blend::Replace,
                },
            },
        )
        .unwrap();
    let input = Input {
        force_coarsest: false,
        context,
        header: Header {
            x: 0,
            y: 0,
            mask: 5,
        },
        basic: [Basic {
            tint: [193, 117, 243],
            depth: 20123,
        }; 4],
        light: [CompactPixelInput {
            normal: [0, 0, 1024],
            ndc: [0; 2],
        }; 4],
        uv_q16: [
            [32750, 16375],
            [34800, 16375],
            [32750, 18425],
            [34800, 18425],
        ],
    };
    let mut reference = texture::Image {
        bytes: asset,
        requests: vec![],
    };
    let mut cache = oracle::Cache::new(vec![slot]).unwrap();
    let golden = oracle::sample(
        &gpu_v2::texture::ports::QuadInput {
            force_coarsest: false,
            quad_id: 0,
            mask: 5,
            uv: input.uv_q16.map(|v| v.map(|x| x as f64 / 65536.0)),
            slot: 0,
            material_size_log2: 5,
            filter: Filter::Trilinear,
            lod_bias: 0.125,
        },
        &mut cache,
        &mut reference,
        gpu_v2::texture::ports::Config::counted(),
    )
    .unwrap();
    let mut accepted = false;
    let mut rows = 0;
    let mut clock_calls = 0;
    let mut ce0_active = false;
    for wall in 0..LIMIT {
        let ce = wall % 11 < 8;
        let before = engines.branches().dispatch().signals();
        if let Some(j) = before.final_input {
            let expected = golden.pixels.iter().find(|p| p.lane == j.key.lane).unwrap();
            assert_eq!(j.texture, expected.rgb);
        }
        ce0_active |= !ce && hub.ro_active();
        let step = engines
            .step_with_rop(
                &mut ro,
                Tick {
                    ce,
                    input: (!accepted).then_some(input),
                    lighting_result_ready: true,
                    sampling_issue_ready: true,
                    sampling_result_ready: true,
                    final_issue_ready: wall % 7 != 3,
                    final_result_ready: wall % 17 < 13,
                    finish: accepted,
                    ..Default::default()
                },
                |row| {
                    // Only this call advances the arbiter+gearbox+memory controller.
                    fb.cycle(None, None)?;
                    clock_calls += 1;
                    if ce {
                        if let Some(row) = row {
                            assert_eq!(row.row, rows);
                            assert_eq!(row.header, input.header);
                            if row.row & 1 == 0 && input.header.mask & (1 << (row.row / 2)) != 0 {
                                let p = golden
                                    .pixels
                                    .iter()
                                    .find(|p| p.lane == row.row / 2)
                                    .unwrap();
                                let rgb = final_rgb(
                                    input.basic[0].tint,
                                    p.rgb,
                                    gpu_v2::lighting::ports::LightingOutput { g: 256, h: 0 },
                                    [0; 3],
                                )
                                .unwrap();
                                assert_eq!(
                                    row.data,
                                    u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 193])
                                );
                            }
                            rows += 1;
                        }
                    }
                    Ok(ce)
                },
            )
            .unwrap();
        accepted |= step.branches.dispatch.input_accepted;
        if engines.complete() {
            hub.stop_background();
            if hub.idle() {
                break;
            }
        }
    }
    assert!(
        engines.complete() && hub.idle(),
        "shared wall-clock watchdog"
    );
    assert!(accepted && ce0_active);
    assert_eq!(rows, 8);
    let stats = hub.stats();
    assert_eq!(stats.frame_edges, clock_calls);
    assert_eq!(clock_calls, engines.branches().dispatch().stats.wall);
    assert_eq!(stats.ro_beats, stats.ro_submitted * 16);
    assert_eq!(stats.ro_terminals, stats.ro_submitted);
    assert!(stats.ro_submitted > 0 && stats.bg_completed.iter().all(|&n| n > 0));
    assert!(!hub.physical_error() && !hub.tick_poisoned());
    assert_eq!(
        hub.image(),
        initial,
        "read-only shared fixture changed guards"
    );
    hub.save(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/gpu-overnight-20261006/shared-clock"),
        "dispatcher",
    );
}
