use super::*;

pub(super) fn world_determinant(words: &[u16]) -> f64 {
    let m: [[f64; 3]; 3] =
        std::array::from_fn(|r| std::array::from_fn(|c| scene::fixed(words, (r * 4 + c) * 2)));
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

pub(super) fn shadow_reference_prepare(case: &serde_json::Value) -> String {
    let m = case["world"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect::<Vec<_>>();
    let a = [m[0], m[4], m[8], m[1], m[5], m[9], m[2], m[6], m[10]];
    let c = [
        a[4] * a[8] - a[5] * a[7],
        a[5] * a[6] - a[3] * a[8],
        a[3] * a[7] - a[4] * a[6],
        a[2] * a[7] - a[1] * a[8],
        a[0] * a[8] - a[2] * a[6],
        a[1] * a[6] - a[0] * a[7],
        a[1] * a[5] - a[2] * a[4],
        a[2] * a[3] - a[0] * a[5],
        a[0] * a[4] - a[1] * a[3],
    ];
    let determinant = a[0] * c[0] + a[1] * c[1] + a[2] * c[2];
    assert!(determinant.abs() > 0.1);
    let fixed = |v: f64| {
        let n = (v * 65536.).round() as i32 as u32;
        format!("fix32::from_words({},{})", n & 65535, n >> 16)
    };
    let mut world = vec![];
    let mut normal = vec![];
    for row in 0..3 {
        world.push(format!(
            "row{row}:Vec4{{x:{},y:{},z:{},w:{}}}",
            fixed(m[row]),
            fixed(m[4 + row]),
            fixed(m[8 + row]),
            fixed(m[12 + row])
        ));
        normal.push(format!(
            "row{row}:Vec3::new({},{},{})",
            fixed(c[row * 3] / determinant),
            fixed(c[row * 3 + 1] / determinant),
            fixed(c[row * 3 + 2] / determinant)
        ));
    }
    format!(
        "fn prepare_block(model:&mut Transform){{model.set_matrices(Mat3x4{{{}}},Mat3x3{{{}}});}}",
        world.join(","),
        normal.join(",")
    )
}

pub(super) fn hull_distance(point: [f64; 2], polygon: &[[f64; 2]]) -> f64 {
    polygon
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let b = polygon[(i + 1) % polygon.len()];
            let dx = b[0] - a[0];
            let dz = b[1] - a[1];
            let length = dx * dx + dz * dz;
            let t = if length > 0. {
                ((point[0] - a[0]) * dx + (point[1] - a[1]) * dz) / length
            } else {
                0.
            }
            .clamp(0., 1.);
            (point[0] - a[0] - t * dx).hypot(point[1] - a[1] - t * dz)
        })
        .fold(f64::INFINITY, f64::min)
}

pub(super) fn polygon_area(p: &[[f64; 2]]) -> f64 {
    p.iter()
        .enumerate()
        .map(|(i, a)| {
            let b = p[(i + 1) % p.len()];
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        * 0.5
}
