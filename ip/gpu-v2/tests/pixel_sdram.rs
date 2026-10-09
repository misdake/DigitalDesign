//! J1 only: external reference branch results, actual serial cycle MC writeback.
use digital_design_hardware_gowin::sdram_memory_controller::ports::OracleImage;
use gpu_v2::{
    lighting::{
        ports as light,
        sim::{counted, oracle as lighting_oracle},
    },
    system::pixel::Model,
    texture::{ports as tex, sim::oracle as texture_oracle},
};
#[path = "support/sdram/burst.rs"]
mod burst;
#[path = "support/pixel.rs"]
mod support;
#[allow(dead_code)]
#[path = "support/texture.rs"]
mod texture_fixture;

fn actual_mc(inputs: &[support::Stimulus], pauses: bool, label: &str) {
    let context = support::context();
    let initial = support::image();
    let expected = support::golden(initial.clone(), inputs, context);
    // External host image crosses the vendor protection boundary explicitly.
    // These are initial memory bytes, not injected audited arithmetic results.
    let image =
        unsafe { OracleImage::from_host(0, initial, "J1 initial framebuffer and guards") }.unwrap();
    let mut port = burst::Adapter::new(image, 100_000).unwrap();
    let mut model = Model::new(context, 100_000).unwrap();
    let proof = support::replay(&mut model, &mut port, inputs, pauses, 100_000);
    assert!(model.complete() && port.idle());
    assert_eq!(
        port.combination.cycle, model.stats.wall_cycles,
        "MC must advance exactly once per wall edge"
    );
    assert_eq!(port.combination.bridge.pins.bytes(), expected);
    assert!(proof.mc_reads > 8 && proof.mc_writes > 8);
    assert_eq!(proof.mc_read_beats, proof.mc_reads * 16);
    assert_eq!(proof.mc_write_beats, proof.mc_writes * 16);
    assert_eq!(proof.mc_completions, proof.mc_reads + proof.mc_writes);
    assert_eq!(model.stats.peak_live, 16);
    assert!(proof.out_of_order && proof.captured_before_commit);
    if pauses {
        assert!(proof.ce_local_return && proof.ce_memory_return);
    }
    println!("J1 {label}: wall={} admitted={} dropped={} peak={} basic_r={} light_r={} sample_r={} input_stall={} output_stall={} mc_r={} mc_w={} mc_rbeats={} mc_wbeats={}",
        model.stats.wall_cycles, model.stats.admitted, model.stats.dropped, model.stats.peak_live,
        model.stats.basic_reads, model.stats.light_reads, model.stats.sample_reads,
        model.stats.input_stalls, model.stats.output_stalls, proof.mc_reads, proof.mc_writes,
        proof.mc_read_beats, proof.mc_write_beats);
}

#[test]
fn controlled_result_wrap_and_flush_through_actual_serial_mc() {
    let inputs = support::synthetic(80);
    actual_mc(&inputs, false, "synthetic-continuous");
    actual_mc(&inputs, true, "synthetic-paused");
}

#[test]
fn reference_lighting_and_sampling_results_reach_final_color_and_depth() {
    let mut inputs = support::synthetic(48);
    let slot = texture_fixture::slot(5, true);
    let mut cache = texture_oracle::Cache::new(vec![slot]).unwrap();
    let mut texture = texture_fixture::Image {
        bytes: texture_fixture::asset(slot, texture_fixture::pattern),
        requests: vec![],
    };
    let normals = [
        [0, 0, 0],
        [0, 0, 8192],
        [32767, -32768, 16384],
        [-731, 2173, -9911],
        [0, 0, -16384],
        [1200, 4700, 7133],
    ];
    let context = support::context();
    for (i, s) in inputs.iter_mut().enumerate() {
        let material = light::Material {
            specular_color: context.specular,
            shininess_code: (i % 17) as u8,
            ..Default::default()
        };
        for lane in 0..4 {
            let pixel = light::PixelInput {
                normal: normals[(i + lane) % normals.len()],
                ndc: [
                    ((i + lane) as i32 % 3 - 1) * 16384,
                    ((i + lane * 2) as i32 % 3 - 1) * 16384,
                ],
            };
            // Freeze the baseline Config::default on both paths; no mixing
            // with optimized hardware/scalar/square9 numerical profiles.
            let report = counted::evaluate(
                pixel,
                material,
                light::Light::default(),
                light::Projection::default(),
                2048,
            )
            .unwrap();
            let golden = lighting_oracle::evaluate(
                pixel,
                material,
                light::Light::default(),
                light::Projection::default(),
                lighting_oracle::Config::default(),
            )
            .unwrap();
            assert_eq!(
                (i128::from(report.output.g), i128::from(report.output.h)),
                (golden.g, golden.h)
            );
            s.light[lane] = report.output;
        }
        // Repeated seams and negative helpers remain inside the selected
        // unwrapped S(18,16) domain; out-of-domain rejection is tested separately.
        let u = (i % 17) as f64 * 0.137 - 1.0;
        let v = (i % 17) as f64 * -0.093 + 1.0;
        let q = tex::QuadInput {
            force_coarsest: false,
            quad_id: (i & 15) as u8,
            mask: s.quad.header.mask,
            uv: [[u, v], [u + 0.04, v], [u, v + 0.04], [u + 0.04, v + 0.04]],
            slot: 0,
            material_size_log2: 5,
            filter: [
                tex::Filter::Nearest,
                tex::Filter::Bilinear,
                tex::Filter::Trilinear,
            ][i % 3],
            lod_bias: 0.5,
        };
        // All four helper UVs reach preparation. Only covered results inject.
        let sampled =
            texture_oracle::sample(&q, &mut cache, &mut texture, tex::Config::counted()).unwrap();
        for p in sampled.pixels {
            s.sample[usize::from(p.lane)] = p.rgb;
        }
    }
    assert!(cache.stats.refills > 0 && !texture.requests.is_empty());
    actual_mc(&inputs, true, "reference-branches-paused");
}
