//! Live-global factory exports and independent captured-sample numerical golden.
use gpu_v2::lighting::{
    dsp_sharing,
    ports::{Light, Material, PixelRows, Projection},
    sim::{counted, oracle},
};
use std::path::PathBuf;

#[test]
#[ignore = "writes the ten actual public-factory exports for bounded RTL checks"]
fn export_live_global_profiles() {
    use gpu_v2::{
        framebuffer::ports::MaterializedSurface, system::pixel::quad_ingress::rtl::Backend,
        texture::ports::Slot,
    };
    let out = PathBuf::from(
        std::env::var_os("GPU_LIVE_GLOBAL_EXPORT_DIR").expect("fresh export directory"),
    );
    std::fs::create_dir_all(&out).unwrap();
    let surface = MaterializedSurface {
        color_base_bytes: 0x100000,
        depth_base_bytes: 0x140000,
        width: 400,
        height: 240,
    };
    let slots = [Slot {
        base_address: 0,
        max_size_log2: 0,
        has_full_mip: false,
        valid: true,
    }];
    for (name, backend) in [("native", Backend::Native), ("portable", Backend::Portable)] {
        let portable = matches!(backend, Backend::Portable);
        let frontend = gpu_v2::frontend::vliw::verilog(portable);
        for (profile, source) in [
            ("frontend", frontend.clone()),
            (
                "frontend-command-ports",
                format!("`define GPU_V2_COMMAND_PORTS\n{frontend}"),
            ),
            (
                "generic",
                gpu_v2::system::vliw::verilog(&slots, surface, backend).unwrap(),
            ),
            (
                "sdram",
                gpu_v2::system::vliw_sdram::verilog(&slots, surface, backend).unwrap(),
            ),
            (
                "command",
                gpu_v2::system::vliw_command::verilog(surface, backend).unwrap(),
            ),
        ] {
            std::fs::write(out.join(format!("{profile}-{name}.v")), source).unwrap();
        }
    }
}

fn rows(text: &str) -> Vec<Vec<u64>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.split_whitespace()
                .map(|word| u64::from_str_radix(word, 16).unwrap())
                .collect()
        })
        .collect()
}

fn h9(cosine: i128, code: usize, positive: bool) -> u64 {
    let exponent: i32 = [
        4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64,
    ][code];
    let mut shift = 0;
    while (1 << shift) * exponent < 6144 {
        shift += 1;
    }
    let distance = 32768 - cosine;
    let bin = distance >> shift;
    if !positive || bin >= 32 {
        return 0;
    }
    let width = 1_i128 << shift;
    let endpoint = |i: i128| {
        (256.0 * (1.0 - (i * width) as f64 / 32768.0).max(0.0).powi(exponent)).round_ties_even()
            as u64
    };
    let base = endpoint(bin);
    let delta = base - endpoint(bin + 1);
    let fraction = ((distance << 6) >> shift) as u64 & 63;
    base - ((delta * fraction + 32) >> 6)
}

#[test]
#[ignore = "requires actual bounded direct_global input and shading captures"]
fn actual_live_global_samples_match_independent_stage_oracle() {
    let root =
        PathBuf::from(std::env::var_os("GPU_LIVE_GLOBAL_GOLD_INPUT").expect("capture directory"));
    let input = rows(&std::fs::read_to_string(root.join("inputs.txt")).unwrap());
    let output = rows(&std::fs::read_to_string(root.join("shade.txt")).unwrap());
    assert_eq!(input.len(), output.len(), "unretired Lighting sample");
    assert!(input.len() > 8);
    let (options, _) = dsp_sharing::four_cycle_shared_roms(9).unwrap();
    let config = oracle::Config::from_counted(counted::Config {
        smooth_h9: options.smooth_h9,
        h9_tail_dsp: options.h9_tail_dsp,
        rsqrt_q13: options.rsqrt_q13,
        lit_queue: options.lit_queue,
        quantization: options.quantization,
        scalar_norm: options.scalar_norm,
        weighted_view: options.weighted_view,
        scalar_normal: options.scalar_normal,
        exact_normal_gate: options.exact_normal_gate,
        block_prescale: options.block_prescale,
        direct_square: options.direct_square,
        direct_all_squares: options.direct_all_squares,
        square9: options.square9,
        dedicated_dots: options.dedicated_dots,
        ..counted::Config::architecture()
    });
    assert!(options.smooth_h9);
    let mut golden = String::new();
    let mut full = 0;
    let mut ambient = 0;
    let mut specular = 0;
    for (index, (values, actual)) in input.iter().zip(&output).enumerate() {
        assert_eq!(values.len(), 15);
        assert_eq!(actual.len(), 3);
        let pixel = PixelRows([values[11], values[12], values[13]])
            .decode()
            .unwrap();
        let expected = match values[1] {
            1 => {
                ambient += 1;
                (values[3] as i128, 0)
            }
            2 | 3 => {
                full += 1;
                let light = Light {
                    direction: [values[5] as i16, values[6] as i16, values[7] as i16],
                    ambient: values[3] as u16,
                    directional: values[4] as u16,
                };
                let result = oracle::evaluate(
                    pixel,
                    Material {
                        shininess_code: values[2] as u8,
                        specular_color: if values[1] == 3 { [255; 3] } else { [0; 3] },
                        unlit: false,
                    },
                    light,
                    Projection {
                        ray_scale: [values[8] as i16, values[9] as i16],
                        k: values[10] as i16,
                    },
                    config,
                )
                .unwrap();
                // The independent stage oracle supplies normalization and cosine.
                // Smooth H9 uses the published grid equation rather than that
                // oracle's separate legacy power-table API. Recreate endpoints
                // mathematically; never read the generated RTL/ROM as the golden.
                let cosine = result
                    .stages
                    .iter()
                    .find(|(name, _)| name == "x")
                    .unwrap()
                    .1;
                let positive = pixel
                    .normal
                    .iter()
                    .zip(light.direction)
                    .map(|(&n, l)| i64::from(n) * i64::from(l))
                    .sum::<i64>()
                    > 0;
                let power = h9(cosine, values[2] as usize, positive);
                let product = values[4] * power;
                let q = product >> 8;
                let r = product & 255;
                let h = q + u64::from(r > 128 || r == 128 && q & 1 != 0);
                (result.g, i128::from(h))
            }
            other => panic!("unexpected lit mode {other}"),
        };
        assert_eq!(actual[0], values[14] & 127, "key at sample {index}");
        assert_eq!(
            (actual[1] as i128, actual[2] as i128),
            expected,
            "G/H at sample {index}, epoch {} mode {}",
            values[0],
            values[1]
        );
        specular += usize::from(expected.1 != 0);
        golden.push_str(&format!(
            "{:02x} {:03x} {:03x}\n",
            actual[0], expected.0, expected.1
        ));
    }
    assert!(full > 0 && ambient > 0 && specular > 0);
    std::fs::write(root.join("independent-golden.txt"), golden).unwrap();
    println!("independent Lighting gold: {} samples, {full} lit, {ambient} ambient, {specular} nonzero H", input.len());
}
