//! Host geometry preparation/reference. No firmware or GPU encoding is implied.
use std::f64::consts::{FRAC_PI_2, PI};

pub type Point = [f64; 2];
pub type Matrix = [[f64; 4]; 3];
pub type Triangle = [[f64; 3]; 3];

pub fn transform(m: Matrix, p: [f64; 3]) -> [f64; 3] {
    m.map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2] + r[3])
}

/// World Y points up; input is a centered unit box, rendered at 98% size.
/// Fraction is the angular fraction; easing/time ownership stays with the caller.
pub fn roll(orient: u8, dir: u8, fraction: f64, drop: f64) -> Matrix {
    assert!(orient < 3 && dir < 4 && (0.0..=1.0).contains(&fraction));
    let size = match orient {
        0 => [1.0, 2.0, 1.0],
        1 => [2.0, 1.0, 1.0],
        _ => [1.0, 1.0, 2.0],
    };
    let center = size.map(|v| v * 0.5);
    let pivot = match dir {
        0 => [size[0], 0.0, 0.5],
        1 => [0.0, 0.0, 0.5],
        2 => [0.5, 0.0, size[2]],
        _ => [0.5, 0.0, 0.0],
    };
    let sign = if dir == 0 || dir == 3 { -1.0 } else { 1.0 };
    let (s, c) = (sign * FRAC_PI_2 * fraction).sin_cos();
    let rotation = if dir < 2 {
        [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
    } else {
        [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
    };
    std::array::from_fn(|r| {
        let t = pivot[r]
            + (0..3)
                .map(|k| rotation[r][k] * (center[k] - pivot[k]))
                .sum::<f64>()
            + if r == 1 { drop } else { 0.0 };
        [
            rotation[r][0] * size[0],
            rotation[r][1] * size[1],
            rotation[r][2] * size[2],
            t,
        ]
    })
}

pub fn compose(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            (0..3).map(|k| a[r][k] * b[k][c]).sum::<f64>() + if c == 3 { a[r][3] } else { 0.0 }
        })
    })
}

/// Singular positional transform only. Never feed this to an inverse-normal helper.
pub fn shadow_matrix(model: Matrix) -> Matrix {
    compose(
        [
            [1.0, -0.5, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.006],
            [0.0, -0.5, 1.0, 0.0],
        ],
        model,
    )
}

pub fn camera(width: f64, depth: f64) -> (f64, Matrix) {
    assert!(width > 0.0 && depth > 0.0 && width <= 32.0 && depth <= 32.0);
    let yaw = PI / 12.0;
    let length = 0.85_f64.hypot(0.8);
    let u = [
        yaw.sin() * 0.85 / length,
        0.8 / length,
        yaw.cos() * 0.85 / length,
    ];
    let x = [yaw.cos(), 0.0, -yaw.sin()];
    let y = [u[1] * x[2], u[2] * x[0] - u[0] * x[2], -u[1] * x[0]];
    let tan_v = (PI / 18.0).tan();
    let mut distance = 8.0_f64;
    for px in [-width / 2.0, width / 2.0] {
        for pz in [-depth / 2.0, depth / 2.0] {
            for py in [-0.6, 1.4] {
                let p = [px, py, pz];
                let dot = |a: [f64; 3]| (0..3).map(|i| a[i] * p[i]).sum::<f64>();
                distance = distance.max(dot(x).abs() / (tan_v * 5.0 / 3.0) + dot(u));
                distance = distance.max(dot(y).abs() / tan_v + dot(u));
            }
        }
    }
    distance = (distance * 1.03).clamp(8.0, 130.0);
    // Current GPU ray uses focal length 256; bake the original 20-degree lens
    // into X/Y only. Forward Z and its depth range retain world units.
    let k = 120.0 / tan_v / 256.0;
    (
        distance,
        [
            [k * x[0], 0.0, k * x[2], 0.0],
            [-k * y[0], -k * y[1], -k * y[2], k * y[1] * 0.6],
            [-u[0], -u[1], -u[2], distance + u[1] * 0.6],
        ],
    )
}

fn cross(o: Point, a: Point, b: Point) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

fn hull(mut points: Vec<Point>) -> Vec<Point> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    if points.len() < 3 {
        return Vec::new();
    }
    let mut result = Vec::new();
    for p in &points {
        while result.len() >= 2
            && cross(result[result.len() - 2], result[result.len() - 1], *p) <= 0.0
        {
            result.pop();
        }
        result.push(*p);
    }
    let lower = result.len();
    for p in points.iter().rev().skip(1) {
        while result.len() > lower
            && cross(result[result.len() - 2], result[result.len() - 1], *p) <= 0.0
        {
            result.pop();
        }
        result.push(*p);
    }
    result.pop();
    if result.len() < 3 {
        result.clear();
    }
    result
}

/// Clip the box volume at Y=0 before projecting its silhouette. At most 20
/// candidate points (eight corners and twelve edge intersections), independent
/// of screen size. Float64 is a host reference, not a CPU performance claim.
pub fn floor_shadow(model: Matrix) -> Vec<Point> {
    let corners: [[f64; 3]; 8] = std::array::from_fn(|i| {
        transform(
            model,
            [
                if i & 4 == 0 { -0.49 } else { 0.49 },
                if i & 2 == 0 { -0.49 } else { 0.49 },
                if i & 1 == 0 { -0.49 } else { 0.49 },
            ],
        )
    });
    let mut points = Vec::with_capacity(20);
    for p in corners {
        if p[1] >= 0.0 {
            points.push([p[0] - p[1] * 0.5, p[2] - p[1] * 0.5]);
        }
    }
    for a in 0..8 {
        for bit in [1, 2, 4] {
            let b = a ^ bit;
            if a >= b || (corners[a][1] >= 0.0) == (corners[b][1] >= 0.0) {
                continue;
            }
            let t = corners[a][1] / (corners[a][1] - corners[b][1]);
            points.push([
                corners[a][0] + t * (corners[b][0] - corners[a][0]),
                corners[a][2] + t * (corners[b][2] - corners[a][2]),
            ]);
        }
    }
    hull(points)
}

pub fn clip_cell(poly: &[Point], x: f64, z: f64) -> Vec<Point> {
    let mut out = poly.to_vec();
    for (axis, limit, sign) in [
        (0, x, 1.0),
        (0, x + 1.0, -1.0),
        (1, z, 1.0),
        (1, z + 1.0, -1.0),
    ] {
        let old = std::mem::take(&mut out);
        if old.is_empty() {
            break;
        }
        for i in 0..old.len() {
            let p = old[(i + old.len() - 1) % old.len()];
            let q = old[i];
            let a = (p[axis] - limit) * sign;
            let b = (q[axis] - limit) * sign;
            if (a >= 0.0) != (b >= 0.0) {
                let t = a / (a - b);
                out.push([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]);
            }
            if b >= 0.0 {
                out.push(q);
            }
        }
    }
    out
}

pub fn area(poly: &[Point]) -> f64 {
    if poly.len() < 3 {
        return 0.0;
    }
    (1..poly.len() - 1)
        .map(|i| cross(poly[0], poly[i], poly[i + 1]))
        .sum::<f64>()
        .abs()
        * 0.5
}

/// Current receiver membership is supplied by the caller (bridges/hatches
/// included only while receiving). Failure publishes no partial triangle list.
pub fn receiver_shadow(
    model: Matrix,
    receivers: &[[i16; 2]],
    capacity: usize,
) -> Result<Vec<Triangle>, &'static str> {
    if receivers.len() > 256 {
        return Err("receiver capacity");
    }
    let poly = floor_shadow(model);
    let mut triangles = Vec::new();
    // A cell must not appear twice, or transparent blending doubles its shadow.
    for (index, cell) in receivers.iter().enumerate() {
        if receivers[..index].contains(cell) {
            return Err("duplicate receiver");
        }
        let clipped = clip_cell(&poly, f64::from(cell[0]), f64::from(cell[1]));
        for i in 1..clipped.len().saturating_sub(1) {
            if cross(clipped[0], clipped[i], clipped[i + 1]).abs() <= 1e-12 {
                continue;
            }
            if triangles.len() == capacity {
                return Err("shadow triangle capacity");
            }
            triangles.push([clipped[0], clipped[i], clipped[i + 1]].map(|p| [p[0], 0.006, p[1]]));
        }
    }
    Ok(triangles)
}

pub fn report() {
    let (distance, camera) = camera(15.0, 10.0);
    let model = roll(0, 0, 0.5, 0.0);
    let projected = compose(camera, shadow_matrix(model));
    let triangles = receiver_shadow(model, &[[0, 0], [1, 0]], 64).unwrap();
    println!("Bloxorz host geometry: 400x240, distance {distance:.6}, {} clipped shadow triangles, projected center {:?}, shadow area {:.6}", triangles.len(), transform(projected, [0.0; 3]), area(&floor_shadow(model)));
    let receivers: Vec<_> = (-3..=4)
        .flat_map(|x| (-3..=4).map(move |z| [x, z]))
        .collect();
    let mut peak = 0;
    for orient in 0..3 {
        for dir in 0..4 {
            for fraction in [0.0, 0.125, 0.5, 0.875, 1.0] {
                for drop in [0.0, -0.7, -3.0] {
                    peak = peak.max(
                        receiver_shadow(roll(orient, dir, fraction, drop), &receivers, 256)
                            .unwrap()
                            .len(),
                    );
                }
            }
        }
    }
    println!("180 sampled poses on an 8x8 receiver grid: peak {peak} shadow triangles per caster (sample bound only)");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64, tolerance: f64) {
        assert!((a - b).abs() < tolerance, "{a} != {b}");
    }

    #[test]
    fn poses_camera_and_floor_clipping_match_original_three_helpers() {
        let mut view = [[0.0; 4]; 3];
        let mut poly = Vec::new();
        let mut poses = 0;
        let mut cameras = 0;
        for line in include_str!("assets/geometry-reference.txt").lines() {
            let mut fields = line.split_whitespace();
            let tag = fields.next().unwrap();
            if tag == "#" {
                continue;
            }
            let v: Vec<f64> = fields.map(|x| x.parse().unwrap()).collect();
            match tag {
                "camera" => {
                    let (distance, matrix) = camera(v[0], v[1]);
                    near(distance, v[2], 1e-10);
                    view = matrix;
                    cameras += 1;
                }
                "screen" => {
                    let p = transform(view, [v[0], v[1], v[2]]);
                    near(199.5 + 256.0 * p[0] / p[2], v[3], 1e-9);
                    near(119.5 + 256.0 * p[1] / p[2], v[4], 1e-9);
                }
                "pose" => {
                    let model = roll(v[0] as u8, v[1] as u8, v[2], v[3]);
                    for (a, b) in model.into_iter().flatten().zip(&v[4..16]) {
                        near(a, *b, 1e-12);
                    }
                    poly = floor_shadow(model);
                    let n = v[16] as usize;
                    let reference: Vec<Point> =
                        (0..n).map(|i| [v[17 + i * 2], v[18 + i * 2]]).collect();
                    // Original helper uses Float32 world corners. Compare geometric
                    // support and area rather than order/collinear vertex count.
                    near(area(&poly), area(&reference), 2e-6);
                    for direction in 0..32 {
                        let angle = direction as f64 * PI / 16.0;
                        let support = |p: &[Point]| {
                            p.iter()
                                .map(|p| p[0] * angle.cos() + p[1] * angle.sin())
                                .fold(-1000.0, f64::max)
                        };
                        near(support(&poly), support(&reference), 1e-6);
                    }
                    poses += 1;
                }
                "cell" => near(area(&clip_cell(&poly, 0.0, 0.0)), v[0], 2e-6),
                _ => panic!("unknown fixture row"),
            }
        }
        assert_eq!((poses, cameras), (180, 3));
    }

    #[test]
    fn receiver_holes_capacity_and_below_floor_are_explicit() {
        let model = roll(0, 0, 0.5, 0.0);
        let triangles = receiver_shadow(model, &[[0, 0], [2, 0]], 32).unwrap();
        assert!(!triangles.is_empty());
        for triangle in &triangles {
            let center = triangle.iter().map(|p| p[0]).sum::<f64>() / 3.0;
            assert!(
                !(1.0..2.0).contains(&center),
                "hole must not receive shadow"
            );
            assert!(triangle.iter().all(|p| p[2] >= 0.0 && p[2] <= 1.0));
        }
        assert!(receiver_shadow(model, &[[0, 0]], 0).is_err());
        assert!(receiver_shadow(model, &[[0, 0], [0, 0]], 32).is_err());
        assert!(receiver_shadow(roll(0, 0, 0.5, -3.0), &[[0, 0]], 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn projection_matrix_matches_point_projection_without_normal_inverse() {
        for orient in 0..3 {
            for dir in 0..4 {
                for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    let model = roll(orient, dir, fraction, 0.0);
                    let s = shadow_matrix(model);
                    for p in [[-0.49; 3], [0.49; 3], [0.0; 3]] {
                        let world = transform(model, p);
                        let projected = transform(s, p);
                        near(projected[0], world[0] - world[1] / 2.0, 1e-12);
                        near(projected[1], 0.006, 1e-12);
                        near(projected[2], world[2] - world[1] / 2.0, 1e-12);
                    }
                }
            }
        }
    }

    #[test]
    fn light_facing_box_faces_cover_once_closed_box_covers_twice() {
        let mut checked = 0;
        for dir in [0, 2] {
            for step in 0..=12 {
                let model = roll(0, dir, f64::from(step) / 12.0, 0.0);
                let shadow = shadow_matrix(model);
                let silhouette = floor_shadow(model);
                let mut faces = Vec::new();
                for axis in 0..3 {
                    for sign in [-1.0, 1.0] {
                        let dot_light =
                            sign * (model[0][axis] * 0.5 + model[1][axis] + model[2][axis] * 0.5);
                        let poly: Vec<Point> =
                            [[-0.49, -0.49], [0.49, -0.49], [0.49, 0.49], [-0.49, 0.49]]
                                .into_iter()
                                .map(|q| {
                                    let mut p = [0.0; 3];
                                    p[axis] = sign * 0.49;
                                    p[(axis + 1) % 3] = q[0];
                                    p[(axis + 2) % 3] = q[1];
                                    let p = transform(shadow, p);
                                    [p[0], p[2]]
                                })
                                .collect();
                        if area(&poly) > 1e-10 {
                            faces.push((dot_light > 0.0, poly));
                        }
                    }
                }
                assert!(faces.iter().filter(|f| f.0).count() <= 3);
                // Skip boundary samples: this verifies continuous coverage, not
                // rasterizer top-left ownership or native fixed-point edge rules.
                let inside = |poly: &[Point], p: Point| -> Option<bool> {
                    let edges: Vec<_> = (0..poly.len())
                        .map(|i| cross(poly[i], poly[(i + 1) % poly.len()], p))
                        .collect();
                    if edges.iter().any(|d| d.abs() < 1e-8) {
                        None
                    } else {
                        Some(edges.iter().all(|d| *d > 0.0) || edges.iter().all(|d| *d < 0.0))
                    }
                };
                for ix in -20..50 {
                    for iz in -20..30 {
                        let p = [f64::from(ix) * 0.061 + 0.013, f64::from(iz) * 0.067 + 0.019];
                        if inside(&silhouette, p) != Some(true) {
                            continue;
                        }
                        let coverage: Option<Vec<_>> = faces
                            .iter()
                            .map(|(front, poly)| inside(poly, p).map(|v| (*front, v)))
                            .collect();
                        let Some(coverage) = coverage else {
                            continue;
                        };
                        assert_eq!(coverage.iter().filter(|(_, hit)| *hit).count(), 2);
                        assert_eq!(
                            coverage
                                .iter()
                                .filter(|(front, hit)| *front && *hit)
                                .count(),
                            1
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 5000);
    }
}
