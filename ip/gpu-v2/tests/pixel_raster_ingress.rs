//! Existing triangle oracle produces real coverage and four helper attributes;
//! the adapter/ingress keeps finite compact payloads and releases the source.
use gpu_v2::{
    framebuffer::ports::{Blend, Context, DepthFunc},
    lighting::ports::{Light, LightingContext, Material, Projection},
    system::{
        oracle::ports::RasterQuad,
        pixel::{dispatch::*, raster_input},
    },
    texture::ports::Filter,
    triangle::{ports, sim::oracle},
};
use std::collections::BTreeMap;

fn material() -> CommonContext {
    CommonContext {
        lighting: LightingContext {
            material: Material {
                unlit: false,
                shininess_code: 8,
                specular_color: [23; 3],
            },
            light: Light {
                direction: [0, 0, 16384],
                ambient: 32,
                directional: 192,
            },
            projection: Projection::default(),
            epoch: 1,
        },
        sample: Some(SampleContext {
            slot: 0,
            size_log2: 5,
            filter: Filter::Trilinear,
            bias_q8: 0,
        }),
        alpha: 255,
        rop: Context {
            depth: DepthFunc::Less,
            depth_write: true,
            blend: Blend::Replace,
        },
    }
}

fn raster() -> Vec<RasterQuad> {
    let report = oracle::run_continuous(
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
        ports::Config {
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

#[test]
fn real_triangle_attributes_enter_independent_compact_queues() {
    let mut dispatcher = Dispatcher::new(Config {
        max_wall: 2000,
        ..Default::default()
    })
    .unwrap();
    let context = dispatcher.set_context(0, material()).unwrap();
    let quads = raster();
    let quad = quads.iter().find(|q| q.mask != 15).unwrap();
    let (input, stats) =
        raster_input::convert(&dispatcher, context, quad.clone(), [16, 16]).unwrap();
    assert_eq!(stats.normal_clips, 0);
    assert_eq!(input.header.mask, quad.mask);
    for lane in 0..4 {
        let s = &quad.samples[lane];
        assert_eq!(
            input.uv_q18[lane],
            s.uv.map(|v| (v * 262144.0).round_ties_even() as i64)
        );
        if quad.mask & (1 << lane) != 0 {
            assert_eq!(input.light[lane].normal, [1, 205, 1024]);
            assert_eq!(
                input.light[lane].ndc,
                [
                    ((s.position[0] / 8.0 - 1.0) * 65536.0).round_ties_even() as i32,
                    ((1.0 - s.position[1] / 8.0) * 65536.0).round_ties_even() as i32
                ]
            );
        }
    }
    assert!(
        dispatcher
            .tick(Tick {
                ce: true,
                input: Some(input),
                ..Default::default()
            })
            .unwrap()
            .input_accepted
    );
    dispatcher
        .tick(Tick {
            ce: true,
            ..Default::default()
        })
        .unwrap();
    let offers = dispatcher.signals();
    assert_eq!(offers.sampling.unwrap().uv_q18, input.uv_q18);
    assert_eq!(offers.lighting.unwrap().pixels, input.light);
}

#[test]
fn invalid_projective_helpers_are_explicit_and_bypass_ignores_unused_fields() {
    let mut dispatcher = Dispatcher::new(Config::default()).unwrap();
    let context = dispatcher.set_context(0, material()).unwrap();
    let mut quad = raster().into_iter().find(|q| q.mask != 15).unwrap();
    let helper = (0..4).find(|lane| quad.mask & (1 << lane) == 0).unwrap();
    quad.invalid_helpers = 1 << helper;
    assert!(
        raster_input::convert(&dispatcher, context, quad.clone(), [16, 16])
            .unwrap_err()
            .contains("coarsest-LOD")
    );
    let mut bypass = material();
    bypass.lighting.material.unlit = true;
    bypass.sample = None;
    let bypass = dispatcher.set_context(1, bypass).unwrap();
    for sample in &mut quad.samples {
        sample.normal = [f64::NAN; 3];
        sample.uv = [f64::INFINITY; 2];
    }
    assert!(raster_input::convert(&dispatcher, bypass, quad, [16, 16]).is_ok());
}
