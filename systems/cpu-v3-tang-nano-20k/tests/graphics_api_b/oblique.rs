//! Independent matrix-product oracle for oblique scene preparation.
use cpu_v3::CpuV3Sim;
use std::path::Path;
type Matrix = [[f64; 3]; 3];
fn product(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|row| {
        std::array::from_fn(|col| (0..3).map(|k| a[row][k] * b[k][col]).sum())
    })
}
fn rotation([x, y, z]: [f64; 3]) -> Matrix {
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let rx = [[1., 0., 0.], [0., cx, -sx], [0., sx, cx]];
    let ry = [[cy, 0., sy], [0., 1., 0.], [-sy, 0., cy]];
    let rz = [[cz, -sz, 0.], [sz, cz, 0.], [0., 0., 1.]];
    product(product(rz, ry), rx)
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}
fn unit(v: [f64; 3]) -> [f64; 3] {
    let length = dot(v, v).sqrt();
    v.map(|n| n / length)
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn global(cpu: &CpuV3Sim, debug: &rcc::DebugInfo, name: &str, count: u16) -> Vec<u16> {
    let rcc::VarLoc::Global(address) = debug.globals.iter().find(|v| v.name == name).unwrap().loc
    else {
        panic!("missing {name}")
    };
    (0..count).map(|n| cpu.memory(address + n)).collect()
}
fn coefficient(words: &[u16], at: usize) -> i64 {
    i64::from((u32::from(words[at]) | (u32::from(words[at + 1]) << 16)) as i32)
}
fn near(words: &[u16], at: usize, value: f64, tolerance: i64, label: &str) -> i64 {
    let expected = (value * 65536.).round() as i64;
    let error = (coefficient(words, at) - expected).abs();
    assert!(
        error <= tolerance,
        "{label} word{at}: actual={}, expected={expected}, error={error}, tolerance={tolerance}",
        coefficient(words, at)
    );
    error
}
fn inspect(cpu: &CpuV3Sim, debug: &rcc::DebugInfo, folder: &Path, profile: &str) {
    let model = global(cpu, debug, "OB_MODEL", 48);
    let r = rotation([0.125, -0.375, 0.625]);
    let scale = [2., -4., 0.5];
    let position = [10., -4., -12.];
    let mut errors = vec![];
    for row in 0..3 {
        for col in 0..3 {
            errors.push(near(
                &model,
                row * 8 + col * 2,
                r[row][col] * scale[col],
                64,
                "world",
            ));
            errors.push(near(
                &model,
                24 + (row * 3 + col) * 2,
                r[row][col] / scale[col],
                64,
                "normal",
            ));
        }
        errors.push(near(&model, row * 8 + 6, position[row], 0, "translation"));
    }
    assert!(model[42..].iter().all(|&v| v == 0));
    let eye = [3., -2., 5.];
    let camera = rotation([-0.375, 0.625, -0.25]);
    let euler = std::array::from_fn(|row| std::array::from_fn(|col| camera[col][row]));
    let backward = unit([4., -2.5, 8.]);
    let right = unit(cross(unit([0.5, 1., 0.25]), backward));
    let look = [right, cross(backward, right), backward];
    for (name, view) in [("EULER", euler), ("LOOK", look)] {
        let data = global(cpu, debug, &format!("OB_{name}"), 48);
        let native = global(cpu, debug, &format!("OB_NATIVE_{name}"), 256);
        for (row, basis) in view.iter().enumerate() {
            for (col, &value) in basis.iter().enumerate() {
                errors.push(near(&data, row * 8 + col * 2, value, 64, "camera rotation"));
            }
            errors.push(near(
                &data,
                row * 8 + 6,
                -dot(*basis, eye),
                64,
                "camera translation",
            ));
        }
        assert!(data[31..].iter().all(|&v| v == 0));
        assert_eq!(&native[48..64], &[0xaced; 16]);
        assert_eq!(&native[128..], &[0xaced; 128]);
        let mut global_expected = [0u16; 32];
        global_expected[1] = 128;
        global_expected[5] = (-4267i16) as u16;
        global_expected[6] = (-2560i16) as u16;
        global_expected[7] = 8192;
        global_expected[8] = 96;
        global_expected[9] = 32;
        global_expected[10] = 35;
        let light = unit([1., 2., -3.]);
        let mut codes = [0i64; 3];
        for row in 0..3 {
            let factor = if row == 0 { -1. } else { 1. };
            let expected = (dot(view[row], light) * factor * 16384.).round() as i64;
            codes[row] = i64::from(native[2 + row] as i16);
            assert!(
                (codes[row] - expected).abs() <= 4,
                "{profile}/{name}: light {codes:?}/{expected}"
            );
            global_expected[2 + row] = native[2 + row];
            let factor = if row == 2 { -1. } else { -1.5 };
            for col in 0..3 {
                let world = std::array::from_fn(|k| r[k][col] * scale[col]);
                let normal = std::array::from_fn(|k| r[k][col] / scale[col]);
                errors.push(near(
                    &native,
                    64 + row * 8 + col * 2,
                    dot(view[row], world) * factor,
                    128,
                    "native position",
                ));
                let h = if row == 0 { -1. } else { 1. };
                errors.push(near(
                    &native,
                    88 + (row * 3 + col) * 2,
                    dot(view[row], normal) * h,
                    64,
                    "native normal",
                ));
            }
            let translated = std::array::from_fn(|k| position[k] - eye[k]);
            errors.push(near(
                &native,
                64 + row * 8 + 6,
                dot(view[row], translated) * factor,
                128,
                "native translation",
            ));
        }
        let squared: i64 = codes.into_iter().map(|v| v * v).sum();
        assert!(((1 << 28) - 32768..=(1 << 28) + 32768).contains(&squared));
        assert_eq!(&native[..32], &global_expected);
        let mut render = [0u16; 16];
        render[..5].copy_from_slice(&[32768, 32, 0x6655, 0x77, 9]);
        assert_eq!(&native[32..48], &render);
        assert_eq!(&native[106..110], &[0, 1, 0x2211, 0x4433]);
        assert!(native[110..128].iter().all(|&v| v == 0));
        for (kind, words) in [("camera", data), ("native", native)] {
            std::fs::write(
                folder.join(format!("{name}-{kind}.bin")),
                words
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
    }
    std::fs::write(folder.join("oracle.json"),serde_json::to_vec_pretty(&serde_json::json!({"profile":profile,"max_observed_q16_error":errors.iter().max(),"source_tolerance_q16":64,"position_tolerance_q16":128,"light_tolerance_q14":4})).unwrap()).unwrap();
}
#[test]
fn oblique_camera_negative_scale_and_native_outputs_match_independent_matrices() {
    let source = format!(
        "{}\n{}",
        include_str!("../fixtures/graphics_api_b_draw.rs"),
        include_str!("../fixtures/graphics_api_b_oblique.rs")
    );
    super::run_fixture_with_check("oblique", &source, inspect);
    let root = super::evidence_root().join("oblique");
    for name in [
        "EULER-camera.bin",
        "EULER-native.bin",
        "LOOK-camera.bin",
        "LOOK-native.bin",
    ] {
        assert_eq!(
            std::fs::read(root.join("release").join(name)).unwrap(),
            std::fs::read(root.join("debug").join(name)).unwrap(),
            "healthy profiles differ: {name}"
        );
    }
}
