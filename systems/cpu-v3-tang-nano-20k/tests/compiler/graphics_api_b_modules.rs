//! Execute B production helpers through checked RCC and the CPU V3 backend.
use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
use rcc::frontend::rustc_check::RustcCheck;
use std::path::{Path, PathBuf};
#[allow(dead_code)]
#[path = "../support/checked_flat_fixture.rs"]
mod checked_flat_fixture;
#[path = "graphics_api_b/oblique.rs"]
mod oblique;

#[test]
fn material_preparation_and_final_response_quantization() {
    run_fixture(
        "material",
        include_str!("../fixtures/graphics_api_b_material.rs"),
    );
}

#[test]
fn state_texture_view_and_native_render() {
    run_fixture(
        "resources",
        include_str!("../fixtures/graphics_api_b_resources.rs"),
    );
}

#[test]
fn external_ui_rows_cross_windows_and_preserve_adjacent_bytes_and_padding() {
    run_fixture("ui", include_str!("../fixtures/graphics_api_b_ui.rs"));
}

#[test]
fn source_camera_model_and_six_argument_native_draw() {
    let source = format!(
        "{}\nfn main() {{run_cases();halt(0x600d);}}",
        include_str!("../fixtures/graphics_api_b_draw.rs")
    );
    run_fixture("draw", &source);
}

fn evidence_root() -> PathBuf {
    std::env::var_os("GRAPHICS_API_B_OUTPUT")
        .map(|root| PathBuf::from(root).join("modules"))
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/graphics-api-b-modules")
        })
}

fn run_fixture(label: &str, source: &str) {
    run_fixture_with_check(label, source, |_, _, _, _| {});
}

fn run_fixture_with_check(
    label: &str,
    source: &str,
    inspect: impl Fn(&CpuV3Sim, &rcc::DebugInfo, &Path, &str),
) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_root = root.join("rcc");
    // During parallel G0 work A is the sole writer. After A integration this
    // defaults to the accepted local sources; never keep a second B schema.
    let a_root = std::env::var_os("GRAPHICS_API_A_RCC")
        .map(PathBuf::from)
        .unwrap_or_else(|| source_root.clone());
    let schema = std::fs::read_to_string(a_root.join("gpu_schema.rs"))
        .expect("A G0-2 dependency missing: set GRAPHICS_API_A_RCC to its rcc directory");
    // Freeze each actual A dependency for both profiles without copying it into
    // the B production tree. A remains the sole writer of these modules.
    let a_names = [
        "gpu_schema",
        "gpu_storage",
        "gpu_arena",
        "gpu_record",
        "gpu_packet",
        "gpu_bind",
        "gpu_mesh",
        "gpu_ui_packet",
        "gpu_execute",
        "gpu_present",
        "gpu_api_fault",
    ];
    let mut a_sources = std::collections::HashMap::new();
    a_sources.insert("gpu_schema".to_owned(), schema);
    for gpu_debug in [false, true] {
        let profile = if gpu_debug { "debug" } else { "release" };
        let folder = evidence_root().join(label).join(profile);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("input-fixture.rs"), source).unwrap();
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = gpu_debug;
        let ir = checked_flat_fixture::compile(
            label,
            source,
            &options,
            &RustcCheck::new(folder.join("checked")),
            &mut |name| {
                let mut contents = if a_names.contains(&name) {
                    if let Some(text) = a_sources.get(name) {
                        Ok(text.clone())
                    } else {
                        let text = std::fs::read_to_string(a_root.join(format!("{name}.rs")))
                            .map_err(|e| e.to_string())?;
                        a_sources.insert(name.to_owned(), text.clone());
                        Ok(text)
                    }
                } else {
                    std::fs::read_to_string(source_root.join(format!("{name}.rs")))
                        .map_err(|e| e.to_string())
                }?;
                if name == "gpu_scene_math" {
                    contents.push_str(include_str!("../support/rcc/gpu_scene_legacy.rs"));
                }
                std::fs::write(folder.join(format!("input-{name}.rs")), &contents)
                    .map_err(|e| e.to_string())?;
                Ok(contents)
            },
        )
        .unwrap_or_else(|error| panic!("{profile}: {error}"));
        let static_words = ir.debug.static_data_end;
        let code = rcc_backend::try_compile(ir, &options, "main").unwrap();
        if label == "budget" {
            assert!(
                static_words <= 8192,
                "{profile}: static budget exceeded: {} bytes",
                static_words * 2
            );
            assert!(!code
                .debug
                .globals
                .iter()
                .any(|g| g.name == "GPU_OBJECT_TAGS"));
        }
        let predicate = if label == "ui" {
            "gpu_ui_glyph_valid"
        } else {
            "gpu_scene_material_response_valid"
        };
        assert_eq!(
            code.debug.functions.iter().any(|f| f.name == predicate),
            gpu_debug
        );
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        if label == "ui" {
            cpu.physical_memory_mut()[0x98ff * 64..0x98ff * 64 + 384].fill(0xa55a);
        }
        let mut halted = false;
        let mut steps = 0;
        let mut min_sp = 0xc000;
        for _ in 0..250_000 {
            steps += 1;
            let result = cpu.step().unwrap();
            let sp = cpu.register(13).unwrap();
            if sp != 0 {
                min_sp = min_sp.min(sp);
            }
            if let StepOutcome::Halted { signal } = result {
                assert_eq!(signal, 0x600d, "{profile}: target assertion failed");
                halted = true;
                break;
            }
        }
        assert!(halted, "{profile}: exceeded 250000 target instructions");
        assert!((0x8000..=0xc000).contains(&min_sp));
        if label == "draw" || label == "budget" {
            for (name, expected) in [("Vec3", 6), ("Mat3x4", 24), ("Mat3x3", 18)] {
                assert_eq!(
                    code.debug
                        .types
                        .iter()
                        .find(|t| t.name == name)
                        .unwrap()
                        .size,
                    expected
                );
            }
            let read = |name: &str, count: u16| {
                let rcc::VarLoc::Global(address) = code
                    .debug
                    .globals
                    .iter()
                    .find(|g| g.name == name)
                    .unwrap()
                    .loc
                else {
                    panic!("missing global {name}")
                };
                (0..count)
                    .map(|n| cpu.memory(address + n))
                    .collect::<Vec<_>>()
            };
            for (name, second) in [("DRAW_A", false), ("DRAW_B", true)] {
                let mut expected = vec![0xaced; 256];
                expected[0..48].fill(0);
                expected[64..128].fill(0);
                expected[1] = 128;
                expected[7] = 8192;
                expected[8] = 96;
                expected[9] = 32;
                expected[10] = 35;
                if second {
                    expected[4] = (-16384i16) as u16;
                    expected[5] = (-3200i16) as u16;
                    expected[6] = (-1920i16) as u16;
                } else {
                    expected[2] = (-16384i16) as u16;
                    expected[5] = (-6400i16) as u16;
                    expected[6] = (-3840i16) as u16;
                }
                expected[32] = 32768;
                expected[33] = 32;
                expected[34] = 0x6655;
                expected[35] = 0x77;
                expected[36] = 9;
                let position: [f64; 12] = if second {
                    [0., 0., -16., 60., 0., -8., 0., -40., 2., 0., 0., 10.]
                } else {
                    [-2., 0., 0., -9., 0., -4., 0., -18., 0., 0., -8., 33.]
                };
                let normal: [f64; 9] = if second {
                    [0., 0., -0.125, 0., 0.25, 0., -0.5, 0., 0.]
                } else {
                    [-0.5, 0., 0., 0., 0.25, 0., 0., 0., 0.125]
                };
                for (index, value) in position.into_iter().chain(normal).enumerate() {
                    let raw = (value * 65536.).round() as i32 as u32;
                    expected[64 + index * 2] = raw as u16;
                    expected[65 + index * 2] = (raw >> 16) as u16;
                }
                expected[107] = 1;
                expected[108] = 0x2211;
                expected[109] = 0x4433;
                assert_eq!(
                    read(name, 256),
                    expected,
                    "{profile}: complete {name} native/scratch oracle"
                );
            }
            if label == "budget" {
                assert_eq!(
                    read("B_SCRATCH", 256),
                    read("DRAW_A", 256),
                    "{profile}: recorded native changed after arena free/reuse"
                );
            }
            let rotated = read("ROTATED_MODEL", 48);
            let diagonal = read("DIAGONAL_LIGHT", 3)
                .into_iter()
                .map(|v| i64::from(v as i16))
                .collect::<Vec<_>>();
            assert!(diagonal[0] < 0 && diagonal[1] > 0 && diagonal[2] < 0);
            let norm = diagonal.iter().map(|v| v * v).sum::<i64>();
            assert!(
                ((1 << 28) - 32768..=(1 << 28) + 32768).contains(&norm),
                "{profile}: quantized unit light {diagonal:?}"
            );
            let angle = 102943f64 / 65536.;
            let (sin, cos) = angle.sin_cos();
            let reference = [
                2. * cos,
                -4. * sin,
                0.,
                0.,
                2. * sin,
                4. * cos,
                0.,
                0.,
                0.,
                0.,
                8.,
                0.,
                0.5 * cos,
                -0.25 * sin,
                0.,
                0.5 * sin,
                0.25 * cos,
                0.,
                0.,
                0.,
                0.125,
            ];
            for (index, value) in reference.into_iter().enumerate() {
                let raw = u32::from(rotated[index * 2]) | (u32::from(rotated[index * 2 + 1]) << 16);
                assert!(
                    (i64::from(raw as i32) - (value * 65536.).round() as i64).abs() <= 64,
                    "{profile}: rotated coefficient {index}"
                );
            }
            assert!(rotated[42..].iter().all(|&word| word == 0));
        }
        if label == "ui" {
            let mut expected = vec![0u8; 768];
            for (index, byte) in expected[750..].iter_mut().enumerate() {
                *byte = if index & 1 == 0 { 0x5a } else { 0xa5 };
            }
            for row in 2..5 {
                expected[row * 50 + 1..row * 50 + 50].fill(255);
            }
            expected[101..104].copy_from_slice(&[1, 2, 1]);
            expected[748] = 2;
            expected[13 * 50 + 49] = 1;
            let expected_words: Vec<u16> = expected
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            assert_eq!(
                &cpu.physical_memory_mut()[0x98ff * 64..0x98ff * 64 + 384],
                expected_words.as_slice()
            );
        }
        inspect(&cpu, &code.debug, &folder, profile);
        std::fs::write(folder.join("debug.txt"), code.debug.render()).unwrap();
        std::fs::write(folder.join("program.listing"), &code.listing).unwrap();
        let report = serde_json::json!({
            "profile": profile, "static_bytes": static_words * 2,
            "code_bytes": code.words.len() * 2, "steps": steps,
            "observed_stack_bytes": (0xc000 - min_sp) * 2,
            "maximum_steps": 250000, "schema_source": a_root.join("gpu_schema.rs"),
            "max_frame_words": code.debug.functions.iter().map(|f| f.frame_size).max().unwrap_or(0),
            "fixture": label,
            "qualification": "B production numeric helpers; command binding and public resources are covered by host GPU API tests"

        });
        std::fs::write(
            folder.join("result.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("{report}");
    }
}
