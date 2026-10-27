//! Prepared scene matrices against an independent floating TRS/view reference.
use super::*;
use cpu_v3::StepOutcome;
type Matrix = [[f64; 4]; 3];
#[derive(Clone, Copy)]
pub(super) struct Case {
    pub(super) eye: [f64; 3],
    pub(super) camera: [f64; 3],
    pub(super) position: [f64; 3],
    pub(super) angles: [f64; 3],
    pub(super) scale: [f64; 3],
    pub(super) focal: f64,
}
fn scalar(x: f64) -> String {
    let raw = (x * 65536.0).round() as i32 as u32;
    format!("fix32::from_words({},{})", raw & 65535, raw >> 16)
}
fn vector(v: [f64; 3]) -> String {
    format!(
        "vec3::new({},{},{})",
        scalar(v[0]),
        scalar(v[1]),
        scalar(v[2])
    )
}
fn quant(x: f64) -> f64 {
    (x * 65536.0).round() / 65536.0
}
pub(super) fn rotation(a: [f64; 3]) -> [[f64; 3]; 3] {
    let (sx, cx) = quant(a[0]).sin_cos();
    let (sy, cy) = quant(a[1]).sin_cos();
    let (sz, cz) = quant(a[2]).sin_cos();
    [
        [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
        [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
        [-sy, cy * sx, cy * cx],
    ]
}
pub(super) fn reference(c: Case) -> (Matrix, [[f64; 3]; 3]) {
    let camera = rotation(c.camera);
    let model = rotation(c.angles);
    let view: [[f64; 3]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|k| camera[k][r] * if r == 0 { -1.0 } else { 1.0 })
    });
    let normal: [[f64; 3]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|k| (0..3).map(|n| view[r][n] * model[n][k]).sum())
    });
    let mut h = [[0.0; 4]; 3];
    for r in 0..3 {
        let factor = if r == 0 {
            c.focal / 256.0
        } else if r == 1 {
            -c.focal / 256.0
        } else {
            -1.0
        };
        for k in 0..3 {
            h[r][k] = normal[r][k] * c.scale[k] * factor;
        }
        h[r][3] = (0..3)
            .map(|k| view[r][k] * (c.position[k] - c.eye[k]))
            .sum::<f64>()
            * factor;
    }
    (h, normal)
}
pub(super) fn fixed(words: &[u16], at: usize) -> f64 {
    i32::from_le_bytes([
        words[at] as u8,
        (words[at] >> 8) as u8,
        words[at + 1] as u8,
        (words[at + 1] >> 8) as u8,
    ]) as f64
        / 65536.0
}

#[test]
fn public_camera_matches_canonical_native_projection_culling_and_light() {
    // This oracle comes from the separately authored look-at scene, not the
    // Euler/prepared-view formula used by the ordinary cache dependency tests.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scene: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("examples/gpu_demo/assets/scene.json")).unwrap(),
    )
    .unwrap();
    let expected: Vec<i64> = scene["cameras"][0]["matrix"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_i64().unwrap())
        .collect();
    let source = r#"mod gpu_pipeline;
fn main(){let mut r=renderer_open();let zero=vec3::zero();let one=fix32::from_int(1);
let mut c=renderer_camera(&mut r,
vec3::new(fix32::zero(),fix32::from_words(14418,0),fix32::from_words(52429,2)),
vec3::new(fix32::from_words(59932,65535),fix32::zero(),fix32::zero()),
fix32::from_int(200),fix32::from_int(199));
camera_light(&mut c,vec3::new(fix32::from_words(36045,65535),fix32::from_words(52429,0),fix32::from_words(6554,1)));
let mut mesh=renderer_mesh(&mut r);
let mut model=renderer_model(&mut r,zero,zero,vec3::new(one,one,one));
let mut material=renderer_material(&mut r,255,255,255,true);
let mut f=renderer_frame(&mut r,0x1111);let mut p=frame_pass(&mut f,&mut c);
pass_draw(&mut p,&mut mesh,&mut model,&mut material);pass_complete(p);frame_complete(f);
renderer_close(r);halt(0x600d);}"#;
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let ir = resolver::compile_renderer_program(
        "canonical native camera",
        source,
        &options,
        &root.join("rcc"),
        &root.join("rcc"),
        RENDERER_METADATA,
        &mut |_| {},
    )
    .unwrap();
    let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for (address, byte) in crate::shared_support_gpu_rcc_d2::assets() {
        let word = &mut cpu.physical_memory_mut()[address as usize / 2];
        *word = (*word & !(255 << ((address % 2) * 8))) | (u16::from(byte) << ((address % 2) * 8));
    }
    let state = attach(&mut cpu, Some(vec![vec![]]));
    {
        let mut s = state.borrow_mut();
        s.automatic = true;
        s.capture_dynamic = true;
    }
    assert!(matches!(
        cpu.run(5_000_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
    let s = state.borrow();
    assert_eq!((s.starts.len(), s.swaps, s.maintenance), (1, 1, 1));
    let commands = &s.starts[0].1;
    let objects = &s.object_snapshots[0];
    let mut local = vec![0u16; 1536];
    let mut global = vec![];
    let mut instance = vec![];
    for word in commands.as_chunks::<2>().0 {
        let command = u32::from(word[0]) | (u32::from(word[1]) << 16);
        match command >> 28 {
            1 => {
                let source = (((command >> 10) & 0x3ffff) * 32) as usize;
                let destination = (((command >> 3) & 127) * 16) as usize;
                let size = ((32 << ((command >> 1) & 3)) / 2) as usize;
                let at = (source - 0x4a0000) / 2;
                local[destination..destination + size].copy_from_slice(&objects[at..at + size]);
            }
            5 => {
                let at = ((command & 127) * 16) as usize;
                global = local[at..at + 32].to_vec();
            }
            3 => instance = local[1280..1344].to_vec(),
            15 => break,
            _ => {}
        }
    }
    assert_eq!(instance.len(), 64);
    let actual: Vec<i64> = instance
        .as_chunks::<2>()
        .0
        .iter()
        .map(|w| i64::from((u32::from(w[0]) | (u32::from(w[1]) << 16)) as i32))
        .collect();
    for (at, (&a, &b)) in actual.iter().zip(&expected).enumerate() {
        assert!((a - b).abs() <= 4, "native matrix word {at}: {a} vs {b}");
    }
    for axis in 0..3 {
        let light = i64::from(global[2 + axis] as i16);
        let expected_light = scene["cameras"][0]["light"][axis].as_i64().unwrap();
        assert!(
            (light - expected_light).abs() <= 10,
            "native light axis {axis}"
        );
    }
    let transform = |matrix: &[i64], p: [i64; 3]| -> [i64; 3] {
        std::array::from_fn(|r| {
            ((0..3)
                .map(|k| i128::from(matrix[4 * r + k]) * i128::from(p[k]))
                .sum::<i128>()
                + (i128::from(matrix[4 * r + 3]) << 16)) as i64
                >> 16
        })
    };
    let determinant = |p: [[i64; 3]; 3]| -> i128 {
        let [a, b, c] = p.map(|v| v.map(i128::from));
        a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0])
    };
    let mut front = 0;
    let mut back = 0;
    for mesh in scene["meshes"].as_array().unwrap() {
        let config = mesh["config"].as_array().unwrap();
        let shift = config[3].as_u64().unwrap() & 31;
        let points: Vec<[i64; 3]> = mesh["vertices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                let packed = v[0].as_u64().unwrap();
                std::array::from_fn(|k| {
                    config[k].as_i64().unwrap() + ((((packed >> (10 * k)) & 1023) << shift) as i64)
                })
            })
            .collect();
        for triangle in mesh["triangles"].as_array().unwrap() {
            let p: [[i64; 3]; 3] =
                std::array::from_fn(|k| points[triangle[k].as_u64().unwrap() as usize]);
            let reference = determinant(p.map(|v| transform(&expected, v)));
            let observed = determinant(p.map(|v| transform(&actual, v)));
            // Exclude genuinely near-edge-on faces from a quantized sign test.
            if reference.abs() > (1i128 << 30) {
                assert_eq!(
                    observed.signum(),
                    reference.signum(),
                    "{} winding",
                    mesh["name"]
                );
                if reference > 0 {
                    front += 1;
                } else {
                    back += 1;
                }
            }
        }
    }
    assert!(
        front > 100 && back > 100,
        "both culling classes must be exercised"
    );
    // Native X grows toward screen right; the canonical source +X lies left.
    let left = transform(&actual, [65536, 0, 0]);
    let center = transform(&actual, [0, 0, 0]);
    assert!(left[2] > 0 && center[2] > 0);
    assert!(
        i128::from(left[0]) * i128::from(center[2]) < i128::from(center[0]) * i128::from(left[2])
    );
}
#[test]
fn prepared_views_follow_changes_and_normal_excludes_scale_and_projection() {
    let base = Case {
        eye: [0.0; 3],
        camera: [0.0; 3],
        position: [0.0, 0.0, -3.0],
        angles: [0.0; 3],
        scale: [1.0; 3],
        focal: 200.0,
    };
    let translated = Case {
        eye: [1.0, 2.0, 3.0],
        ..base
    };
    let scaled = Case {
        scale: [2.0, 0.5, 1.0],
        ..translated
    };
    let projection = Case {
        focal: 256.0,
        ..scaled
    };
    let rotated = Case {
        angles: [0.0, 0.0, std::f64::consts::FRAC_PI_2],
        ..projection
    };
    let turned = Case {
        eye: [0.0; 3],
        camera: [0.0, std::f64::consts::FRAC_PI_2, 0.0],
        ..rotated
    };
    let behind = Case {
        eye: [0.0, 0.0, -10.0],
        camera: [0.0; 3],
        ..base
    };
    let cases = [
        base, base, translated, scaled, projection, rotated, turned, behind,
    ];
    let mut body = format!(
        r#"{{let mut r=renderer_open();
let mut c=renderer_camera(&mut r,{},{},{},fix32::from_int(199));
let mut m=renderer_model(&mut r,{},{},{});
let mut bounds:Buf<u16,12>=Buf::new([0,65535,0,65535,0,0,0,1,0,1,0,0]);
check(!scene_bounded(vec3::new(fix32::from_words(0,32768),fix32::zero(),fix32::zero()),fix32::from_int(100)));
"#,
        vector(base.eye),
        vector(base.camera),
        scalar(base.focal),
        vector(base.position),
        vector(base.angles),
        vector(base.scale)
    );
    for (i, c) in cases.iter().enumerate() {
        body.push_str(&format!(
            r#"camera_pose(&mut c,{},{});camera_projection(&mut c,{},fix32::from_int(199));
model_pose(&mut m,{},{},{});
{{let mut f=renderer_frame(&mut r,0);let p=frame_pass(&mut f,&mut c);
check(scene_model_prepare(m.word,bounds.as_array()));pass_complete(p);signal(1,{});}}
"#,
            vector(c.eye),
            vector(c.camera),
            scalar(c.focal),
            vector(c.position),
            vector(c.angles),
            vector(c.scale),
            i + 1
        ));
    }
    body.push_str("renderer_close(r);}");
    let program = compile("", &body);
    let global = |name: &str| match program
        .debug
        .globals
        .iter()
        .find(|v| v.name == name)
        .unwrap()
        .loc
    {
        rcc::VarLoc::Global(a) => usize::from(a),
        _ => panic!("not global"),
    };
    let models = global("GPU_SCENE_MODELS");
    let view = global("GPU_SCENE_VIEW");
    let work = global("GPU_SCENE_WORK");
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    let state = attach(&mut cpu, None);
    let mut seen = 0;
    let mut snapshots = Vec::new();
    let mut halted = false;
    for _ in 0..5_000_000 {
        match cpu.step().unwrap() {
            StepOutcome::Signaled(event) if event.signal_type == 1 => {
                let memory = cpu.physical_memory_mut();
                snapshots.push((
                    memory[models + 62..models + 104].to_vec(),
                    memory[view..view + 80].to_vec(),
                    memory[work..work + 8].to_vec(),
                ));
                seen += 1;
                assert_eq!(usize::from(event.value), seen);
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d, "completed {seen} cases");
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted);
    assert_eq!(seen, cases.len());
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    assert!(state.borrow().starts.is_empty());
    for (i, c) in cases.iter().enumerate() {
        let (h, n) = reference(*c);
        let (words, view, _) = &snapshots[i];
        for r in 0..3 {
            for (k, expected) in h[r].iter().enumerate() {
                assert!(
                    (fixed(words, 2 * (r * 4 + k)) - expected).abs() < 0.001,
                    "case {i}, H[{r},{k}]"
                );
            }
            for (k, expected) in n[r].iter().enumerate() {
                assert!(
                    (fixed(words, 24 + 2 * (r * 3 + k)) - expected).abs() < 0.0005,
                    "case {i}, N[{r},{k}]"
                );
            }
        }
        assert_eq!(view[68], 8192);
        let rx = (view[66] as i16) as f64 / 16384.0;
        let ry = (view[67] as i16) as f64 / 16384.0;
        assert!((rx + 100.0 / c.focal).abs() < 0.00015);
        assert!((ry + 60.0 / c.focal).abs() < 0.00015);
        let mut expected_light = rotation(c.camera)[2];
        expected_light[0] = -expected_light[0];
        for k in 0..3 {
            assert!(((view[69 + k] as i16) as f64 / 16384.0 - expected_light[k]).abs() < 0.0003);
        }
    }
    let counters: Vec<_> = snapshots.iter().map(|s| s.2.clone()).collect();
    assert_eq!(&counters[0][..7], &[1, 1, 1, 1, 1, 1, 1]);
    assert_eq!(counters[1], counters[0]);
    assert_eq!(&counters[2][..7], &[1, 2, 1, 1, 1, 2, 1]);
    assert_eq!(&counters[3][..7], &[1, 2, 1, 1, 1, 3, 1]);
    assert_eq!(&counters[4][..7], &[1, 3, 2, 1, 1, 4, 1]);
    assert_eq!(&counters[5][..7], &[1, 3, 2, 1, 2, 5, 2]);
    assert_eq!(&counters[6][..7], &[2, 4, 2, 2, 2, 6, 3]);
    assert_eq!(&counters[7][..7], &[3, 5, 3, 3, 3, 7, 4]);
    assert!(
        fixed(&snapshots[7].0, 22) < 0.0,
        "all-behind bounds were rejected"
    );
}

#[test]
fn registered_scene_draws_three_typed_frames_with_one_prepare_and_exact_native_bytes() {
    let source = r#"mod gpu_pipeline;
fn main(){let mut renderer=renderer_open();let mut field=renderer_text(&mut renderer,0,0,8);let zero=vec3::zero();let one=fix32::from_int(1);
let mut camera=renderer_camera(&mut renderer,zero,zero,fix32::from_int(200),fix32::from_int(199));
let mut mesh=renderer_mesh(&mut renderer);
let mut model=renderer_model(&mut renderer,vec3::new(fix32::zero(),fix32::zero(),fix32::from_int(-3)),zero,vec3::new(one,one,one));
let mut material=renderer_material(&mut renderer,255,255,255,true);let mut tick:u16=0;
while tick<3 {let mut frame=renderer_frame(&mut renderer,0x1111+tick);frame_number(&mut frame,&mut field,tick);
let mut pass=frame_pass(&mut frame,&mut camera);pass_draw(&mut pass,&mut mesh,&mut model,&mut material);
pass_text(&mut pass,&mut field);pass_complete(pass);frame_complete(frame);tick+=1;}
renderer_close(renderer);halt(0x600d);}"#;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let opts = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = resolver::compile_renderer_program(
        "first typed scene",
        source,
        &opts,
        &root,
        &root,
        RENDERER_METADATA,
        &mut |_| {},
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let program = rcc_backend::try_compile(parsed, &opts, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for (address, byte) in crate::shared_support_gpu_rcc_d2::assets() {
        let w = &mut cpu.physical_memory_mut()[address as usize / 2];
        *w = (*w & !(255 << ((address % 2) * 8))) | (u16::from(byte) << ((address % 2) * 8));
    }
    let state = attach(&mut cpu, Some(vec![vec![(0, 8)]; 3]));
    {
        let mut s = state.borrow_mut();
        s.automatic = true;
        s.expected_depth = Some(65535);
        s.scene_draw = true;
    }
    assert!(matches!(
        cpu.run(5_000_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    assert_eq!(state.borrow().starts.len(), 3);
    assert_eq!(state.borrow().maintenance, 3);
    let rcc::VarLoc::Global(address) = program
        .debug
        .globals
        .iter()
        .find(|g| g.name == "GPU_SCENE_WORK")
        .unwrap()
        .loc
    else {
        panic!("not global")
    };
    assert_eq!(
        &cpu.physical_memory_mut()[usize::from(address)..usize::from(address) + 8],
        &[1, 1, 1, 1, 1, 1, 1, 1]
    );
}

#[test]
fn minimal_application_uses_actual_packed_assets_and_public_renderer_calls() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/gpu_renderer");
    let out = root.join("../../target/gpu-renderer-usage/minimal-assets");
    assert!(std::process::Command::new("python")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(example.join("tools/assets.py"))
        .arg("--output")
        .arg(&out)
        .status()
        .unwrap()
        .success());
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    let mut config =
        String::from("format 2\nrenderer\napplication examples/gpu_renderer/rcc/main.rs\n");
    for r in manifest["resources"].as_array().unwrap() {
        let name = r["file"].as_str().unwrap();
        config.push_str(&format!(
            "copy GPU_RENDERER_{} {} {}\n",
            name.trim_end_matches(".bin").to_uppercase(),
            r["address"],
            name
        ));
    }
    let project = cpu_v3_tang_nano_20k::boot::parse_boot_project(&config).unwrap();
    assert!(project.renderer);
    let constants =
        project.constants() + &std::fs::read_to_string(out.join("renderer_assets.rs")).unwrap();
    let opts = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let source = std::fs::read_to_string(example.join("rcc/main.rs")).unwrap();
    let parsed = resolver::compile_renderer_program(
        "minimal renderer app",
        &source,
        &opts,
        &example.join("rcc"),
        &root.join("rcc"),
        &constants,
        &mut |_| {},
    )
    .unwrap();
    let program = rcc_backend::try_compile(parsed, &opts, "renderer_three_frames").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for r in manifest["resources"].as_array().unwrap() {
        let start = r["address"].as_u64().unwrap() as usize;
        let data = std::fs::read(out.join(r["file"].as_str().unwrap())).unwrap();
        for (i, byte) in data.into_iter().enumerate() {
            let at = start + i;
            let word = &mut cpu.physical_memory_mut()[at / 2];
            *word = (*word & !(255 << ((at % 2) * 8))) | (u16::from(byte) << ((at % 2) * 8));
        }
    }
    let state = attach(&mut cpu, Some(vec![vec![(0, 8)]; 3]));
    {
        let mut s = state.borrow_mut();
        s.automatic = true;
        s.expected_depth = Some(65535);
        s.scene_draw = true;
    }
    let result = cpu.run(5_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    let s = state.borrow();
    assert_eq!((s.starts.len(), s.swaps, s.maintenance), (3, 3, 3));
    for (i, page) in s.ui_snapshots.iter().enumerate() {
        let text: Vec<u8> = page[..4].iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(&text, if i < 2 { b"OBS --.-" } else { b"OBS 0.0\xff" });
    }
}

#[test]
fn light_and_far_changes_preserve_prepared_geometry_and_skip_identical_light_work() {
    let lights = [
        [0., 0., 1.],
        [1., 1., 1.],
        [1., 1., 1.],
        [1., 1., 1.],
        [-1., -2., 3.],
        [100., 100., 100.],
        [1. / 65536., 2. / 65536., 3. / 65536.],
    ];
    let far = [199, 198, 198, 197, 197, 197, 197];
    let mut body = String::from(
        r#"{let mut r=renderer_open();let z=vec3::zero();let one=fix32::from_int(1);
let mut c=renderer_camera(&mut r,z,z,fix32::from_int(200),fix32::from_int(199));
let mut m=renderer_model(&mut r,vec3::new(fix32::zero(),fix32::zero(),fix32::from_int(-3)),z,vec3::new(one,one,one));
let mut bounds:Buf<u16,12>=Buf::new([0,65535,0,65535,0,0,0,1,0,1,0,0]);"#,
    );
    for (i, light) in lights.iter().enumerate() {
        body.push_str(&format!("camera_light(&mut c,{});camera_projection(&mut c,fix32::from_int(200),fix32::from_int({}));{{let mut f=renderer_frame(&mut r,0);let p=frame_pass(&mut f,&mut c);check(scene_model_prepare(m.word,bounds.as_array()));pass_complete(p);signal(1,{});}}",vector(*light),far[i],i+1));
    }
    body.push_str("renderer_close(r);}");
    let program = compile("", &body);
    let address = |name: &str| match program
        .debug
        .globals
        .iter()
        .find(|g| g.name == name)
        .unwrap()
        .loc
    {
        rcc::VarLoc::Global(a) => usize::from(a),
        _ => panic!("global"),
    };
    let view = address("GPU_SCENE_VIEW");
    let work = address("GPU_SCENE_WORK");
    let model = address("GPU_SCENE_MODELS") + 62;
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    attach(&mut cpu, None);
    let mut seen = 0;
    let mut matrix = Vec::new();
    let mut halted = false;
    for _ in 0..3_000_000 {
        match cpu.step().unwrap() {
            StepOutcome::Signaled(e) => {
                assert_eq!(usize::from(e.value), seen + 1);
                let memory = cpu.physical_memory_mut();
                let current = &memory[model..model + 42];
                if seen == 0 {
                    matrix = current.to_vec();
                } else {
                    assert_eq!(current, matrix);
                }
                let expected = [1, 1, 1, [1, 2, 2, 2, 3, 4, 5][seen], 1, 1, 1, 0];
                assert_eq!(&memory[work..work + 8], expected);
                assert_eq!(&memory[view + 16..view + 18], &[0, far[seen]]);
                let length: f64 = lights[seen].iter().map(|v| v * v).sum::<f64>().sqrt();
                for axis in 0..3 {
                    let actual = f64::from(memory[view + 69 + axis] as i16) / 16384.;
                    assert!(
                        (actual - lights[seen][axis] / length * if axis == 0 { -1.0 } else { 1.0 })
                            .abs()
                            < 0.0015,
                        "sample {seen} axis {axis}: {actual}"
                    );
                }
                seen += 1;
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d, "after {seen} samples");
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted);
    assert_eq!(seen, lights.len());
}
