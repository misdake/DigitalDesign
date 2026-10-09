#[path = "support/triangle.rs"]
mod fixtures;
use fixtures::*;
use gpu_v2::triangle::{ports::*, sim::oracle::*};

fn independent_coverage(v: [[i64; 2]; 3], x: u16, y: u16, bits: u8) -> bool {
    let step = 1_i128 << bits;
    let p = [
        i128::from(x) * step + step / 2,
        i128::from(y) * step + step / 2,
    ];
    (0..3).all(|i| {
        let a = v[i].map(i128::from);
        let b = v[(i + 1) % 3].map(i128::from);
        let orientation = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        orientation > 0 || orientation == 0 && (a[1] > b[1] || a[1] == b[1] && b[0] > a[0])
    })
}
fn compare_samples(actual: &Sample, expected: &Sample, tolerance: f64) {
    for (a, b) in actual
        .uv
        .iter()
        .chain(&actual.rgb)
        .chain(&actual.normal)
        .chain(&actual.beta)
        .zip(
            expected
                .uv
                .iter()
                .chain(&expected.rgb)
                .chain(&expected.normal)
                .chain(&expected.beta),
        )
    {
        assert!(
            (a - b).abs() < tolerance,
            "{a} vs {b} at {:?}",
            actual.position
        );
    }
    assert!((actual.w - expected.w).abs() < tolerance * actual.w.abs().max(1.0));
    for (a, b) in actual.quantized.uv.iter().zip(expected.quantized.uv) {
        assert!((a - b).abs() <= 1);
    }
    for (a, b) in actual
        .quantized
        .normal
        .iter()
        .zip(expected.quantized.normal)
    {
        assert!((a - b).abs() <= 1);
    }
    assert!(actual.quantized.depth.abs_diff(expected.quantized.depth) <= 1);
    for (shift, mask) in [(11, 31), (5, 63), (0, 31)] {
        assert!(
            ((actual.quantized.rgb565 >> shift) & mask)
                .abs_diff((expected.quantized.rgb565 >> shift) & mask)
                <= 1
        );
    }
}
#[test]
fn five_pressure_scenes_have_independent_coverage_and_stage_references() {
    for (name, input) in pressure() {
        let c = config();
        let r = run(&input, c).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(r.polygon.len() <= 8 && r.triangles.len() <= 6);
        assert_eq!(r.work.source_packages, 1);
        assert_eq!(r.work.projected_vertices, r.polygon.len());
        let samples = r.rasterize().unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!samples.is_empty(), "{name}");
        let points = samples
            .iter()
            .map(|(p, _, _)| *p)
            .collect::<std::collections::BTreeSet<_>>();
        for y in 0..c.height {
            for x in 0..c.width {
                let fans = r
                    .triangles
                    .iter()
                    .filter(|t| independent_coverage(t.vertices, x, y, c.subpixel_bits))
                    .count();
                assert!(fans <= 1, "{name}: fan overlap");
                assert_eq!(points.contains(&[x, y]), fans == 1);
                for t in &r.triangles {
                    assert_eq!(
                        t.contains(x, y, c.subpixel_bits),
                        independent_coverage(t.vertices, x, y, c.subpixel_bits)
                    );
                }
            }
        }
        for (_, _, sample) in samples {
            compare_samples(&sample, &r.reference(sample.position).unwrap(), 1e-8);
        }
        for stage in &r.clip_stages {
            for p in &stage.polygon {
                assert!(p[2] >= f64::from(c.near_raw));
                let guard = (1_u32 << c.guard_log2) as f64 * p[2];
                if stage.plane >= 1 {
                    assert!(p[0] <= guard);
                }
                if stage.plane >= 2 {
                    assert!(p[0] >= -guard);
                }
                if stage.plane >= 3 {
                    assert!(p[1] <= guard);
                }
                if stage.plane >= 4 {
                    assert!(p[1] >= -guard);
                }
            }
        }
        for t in &r.triangles {
            assert_eq!(t.edges.iter().map(|e| e.c).sum::<i128>(), t.determinant);
        }
    }
}
#[test]
fn shared_edges_are_waterproof_including_reversed_clip_intersections() {
    let c = config();
    let corners = [[4.5, 4.5], [58.5, 4.5], [58.5, 34.5], [4.5, 34.5]];
    let a = input([corners[0], corners[1], corners[2]], [64.0; 3], c);
    let mut b = input([corners[0], corners[2], corners[3]], [64.0; 3], c);
    b.vertices[0] = a.vertices[0].clone();
    b.vertices[1] = a.vertices[2].clone();
    let ra = run(&a, c).unwrap();
    let rb = run(&b, c).unwrap();
    for y in 0..c.height {
        for x in 0..c.width {
            let count = ra
                .triangles
                .iter()
                .chain(&rb.triangles)
                .filter(|t| t.contains(x, y, c.subpixel_bits))
                .count();
            assert!(count <= 1);
            assert_eq!(count == 1, (4..58).contains(&x) && (4..34).contains(&y));
        }
    }
    // Two triangles share an edge crossing BOTH near and +x guard, reversed in
    // the input. Canonical clip order must yield equal floating point bits.
    let mut a = input(
        [[32.0, 4.0], [400.0, 24.0], [18.0, 35.0]],
        [0.05, 1.0, 1.0],
        c,
    );
    a.vertices[0].clip[1] = 6000;
    let b = Input {
        id: 18,
        vertices: [
            a.vertices[1].clone(),
            a.vertices[0].clone(),
            vertex([45.0, 35.0], 1.0, 2, c),
        ],
    };
    let ra = run(&a, c).unwrap();
    let rb = run(&b, c).unwrap();
    let shared = ra.polygon.iter().filter(|p| rb.polygon.contains(p)).count();
    assert!(
        shared >= 2,
        "canonical common edge missing: {:?} {:?}",
        ra.polygon,
        rb.polygon
    );
}
#[test]
fn source_fields_survive_clip_fans_negative_w_and_z_is_dead() {
    let c = config();
    let mut v = pressure().pop().unwrap().1;
    v.vertices[0].clip[3] = -3000;
    let a = run(&v, c).unwrap();
    assert!(a.work.intersections >= 2);
    assert!(a.triangles.len() > 1);
    for (_, _, sample) in a.rasterize().unwrap() {
        compare_samples(&sample, &a.reference(sample.position).unwrap(), 1e-8);
    }
    for vertex in &mut v.vertices {
        vertex.clip[2] = i32::MIN;
    }
    let b = run(&v, c).unwrap();
    assert_eq!(a.polygon, b.polygon);
    assert_eq!(
        a.source.as_ref().unwrap().fields,
        b.source.as_ref().unwrap().fields
    );
    assert_eq!(
        a.rasterize()
            .unwrap()
            .iter()
            .map(|(p, _, s)| (*p, s.quantized.clone()))
            .collect::<Vec<_>>(),
        b.rasterize()
            .unwrap()
            .iter()
            .map(|(p, _, s)| (*p, s.quantized.clone()))
            .collect::<Vec<_>>()
    );
}
#[test]
fn nine_planes_and_basis_share_exact_perspective_contract() {
    for (_, input) in pressure() {
        let basis = run(&input, config()).unwrap();
        let planes = run(
            &input,
            Config {
                interpolation: Interpolation::Planes,
                ..config()
            },
        )
        .unwrap();
        let source = basis.source.as_ref().unwrap();
        for (i, position) in source.positions.iter().enumerate() {
            let evaluate =
                |field: &IntegerField| (0..3).map(|k| field[k] * position[k]).sum::<i128>();
            assert_eq!(evaluate(&source.fields[0]), source.determinant);
            assert_eq!(
                evaluate(&source.fields[1]),
                if i == 1 { source.determinant } else { 0 }
            );
            assert_eq!(
                evaluate(&source.fields[2]),
                if i == 2 { source.determinant } else { 0 }
            );
        }
        for (_, _, s) in basis.rasterize().unwrap() {
            compare_samples(&planes.evaluate(s.position).unwrap(), &s, 1e-8);
        }
        assert_eq!(
            planes.work.setup_products(Interpolation::Planes, true)
                - basis.work.setup_products(Interpolation::Basis, true),
            72
        );
    }
    assert_eq!(AlgorithmWork::pixel_products(Interpolation::Basis), 19);
    assert_eq!(AlgorithmWork::pixel_products(Interpolation::Planes), 9);
    let fast = run(&pressure()[0].1, config()).unwrap();
    assert_eq!(fast.work.setup_products(Interpolation::Basis, true), 35);
    assert_eq!(fast.work.setup_products(Interpolation::Planes, true), 107);
}
#[test]
fn quantization_knobs_are_bounded_and_retain_plane_halfspaces() {
    for (_, input) in pressure() {
        for fraction in [16, 20, 24] {
            let r = run(
                &input,
                Config {
                    intersection_fraction: Some(fraction),
                    field_bits: Some(36),
                    ..config()
                },
            )
            .unwrap();
            for p in &r.polygon {
                let g = 8.0 * p[2];
                assert!(p[2] >= 8199.0 && p[0].abs() <= g && p[1].abs() <= g);
            }
            for (_, _, s) in r.rasterize().unwrap() {
                let reference = r.reference(s.position).unwrap();
                assert!((s.uv[0] - reference.uv[0]).abs() < 1e-5);
                assert!(s.quantized.depth.abs_diff(reference.quantized.depth) <= 1);
            }
        }
    }
    for c in [
        Config {
            width: 401,
            ..config()
        },
        Config {
            near_raw: 0,
            ..config()
        },
        Config {
            field_bits: Some(7),
            ..config()
        },
        Config {
            subpixel_bits: 0,
            ..config()
        },
        Config {
            attribute_fraction: 25,
            ..config()
        },
        Config {
            max_samples: 0,
            ..config()
        },
    ] {
        assert!(run(&pressure()[0].1, c).is_err());
    }
    let r = run(
        &pressure()[0].1,
        Config {
            max_samples: 1,
            ..config()
        },
    )
    .unwrap();
    assert!(r.rasterize().unwrap_err().contains("sample count"));
}
#[test]
fn far_depth_rejects_q13_invw_and_affine_color_is_a_real_approximation() {
    let c = config();
    let wall = run(&pressure()[0].1, c).unwrap();
    let s = wall.evaluate([32.5, 20.5]).unwrap();
    assert!((s.w - 200.0).abs() < 1e-10);
    assert_eq!(s.quantized.depth, 65535);
    let bad_w = 1.0 / ((1.0 / 200.0 * 8192.0_f64).round_ties_even() / 8192.0);
    let bad_code = ((bad_w * 65536.0 - f64::from(c.near_raw)) / f64::from(c.far_raw - c.near_raw)
        * 65535.0)
        .round_ties_even() as u16;
    assert_eq!(s.quantized.depth.abs_diff(bad_code), 64);
    let input = &pressure()[2].1;
    let p = run(input, c).unwrap();
    let affine = run(
        input,
        Config {
            rgb_affine: true,
            ..c
        },
    )
    .unwrap();
    let mut worst = 0.0_f64;
    for (_, _, s) in p.rasterize().unwrap() {
        let a = affine.evaluate(s.position).unwrap();
        compare_samples(&a, &affine.reference(s.position).unwrap(), 1e-8);
        for (a, b) in a.rgb.into_iter().zip(s.rgb) {
            worst = worst.max((a - b).abs());
        }
        assert_eq!(a.quantized.uv, s.quantized.uv);
    }
    assert!(
        worst > 0.5,
        "affine color experiment should expose grazing error"
    );
}
#[test]
fn near_animation_is_bounded_and_does_not_clamp_original_w() {
    let c = config();
    let mut input = pressure().pop().unwrap().1;
    let mut previous: Option<std::collections::BTreeSet<[u16; 2]>> = None;
    for raw in (8100..=8296).step_by(4) {
        input.vertices[0].clip[3] = raw;
        let r = run(&input, c).unwrap();
        assert_eq!(r.input.vertices[0].clip[3], raw);
        let set = r
            .rasterize()
            .unwrap()
            .into_iter()
            .map(|(p, _, _)| p)
            .collect::<std::collections::BTreeSet<_>>();
        if let Some(prior) = previous {
            assert!(set.symmetric_difference(&prior).count() <= 64);
        }
        previous = Some(set);
    }
}
#[test]
fn thin_triangles_culling_unorm_and_unwrapped_lod_contracts() {
    let c = config();
    let thin = input([[1.0, 19.51], [62.0, 19.51], [32.0, 19.57]], [1.0; 3], c);
    let r = run(&thin, c).unwrap();
    assert!(!r.triangles.is_empty());
    assert!(!r.rasterize().unwrap().is_empty());
    let s = r.evaluate([32.5, 19.5]).unwrap();
    compare_samples(&s, &r.reference(s.position).unwrap(), 1e-7);
    assert!(
        s.beta[2] < 0.0,
        "coverage snap extrapolation must not clamp source weights"
    );
    let mut reversed = thin.clone();
    reversed.vertices.swap(1, 2);
    assert!(run(
        &reversed,
        Config {
            cull_back: true,
            ..c
        }
    )
    .unwrap()
    .triangles
    .is_empty());
    let flat = input([[0.0, 0.0], [64.0, 0.0], [0.0, 40.0]], [1.0; 3], c);
    let mut flat = flat;
    for v in &mut flat.vertices {
        v.normal = [-16384, 8192, 0];
        v.uv = [4095, 4095];
        v.rgb565 = 0xffff;
    }
    let r = run(&flat, c).unwrap();
    let s = r.evaluate([12.5, 12.5]).unwrap();
    assert_eq!(s.quantized.uv, [1 << 17; 2]);
    assert_eq!(s.quantized.normal, [-16384, 8192, 0]);
    assert_eq!(s.quantized.rgb565, 0xffff);
    assert_eq!(r.quad_lod(12, 12, 1024).unwrap(), 0.0);
    assert!(r.quad_lod(12, 12, 1023).is_err());
    let r = run(
        &input([[0.0, 0.0], [64.0, 0.0], [0.0, 40.0]], [1.0; 3], c),
        c,
    )
    .unwrap();
    assert!((r.quad_lod(12, 12, 1024).unwrap() - (1024.0_f64 / 40.0).log2()).abs() < 0.001);
}
#[test]
fn rejected_geometry_does_no_dead_source_preparation_and_big_inputs_are_safe() {
    let c = config();
    let mut a = input([[0.0, 0.0], [64.0, 0.0], [0.0, 40.0]], [1.0; 3], c);
    for v in &mut a.vertices {
        v.clip[3] = -100;
    }
    let r = run(&a, c).unwrap();
    assert_eq!(r.work.projected_vertices, 0);
    assert_eq!(r.work.source_packages, 0);
    let a = input([[1.0, 1.0], [2.0, 2.0], [3.0, 3.0]], [1.0; 3], c);
    let r = run(&a, c).unwrap();
    assert_eq!(r.work.source_packages, 0);
    let mut a = input([[1.0, 1.0], [60.0, 1.0], [1.0, 38.0]], [1.0; 3], c);
    a.vertices[0].clip = [i32::MIN, i32::MIN, i32::MAX, i32::MIN];
    a.vertices[1].clip = [i32::MAX, i32::MIN, i32::MIN, i32::MAX];
    a.vertices[2].clip = [i32::MIN, i32::MAX, 0, i32::MAX];
    let r = run(&a, c).unwrap();
    let samples = r.rasterize().unwrap();
    assert!(!samples.is_empty());
    for (_, _, s) in samples {
        compare_samples(&s, &r.reference(s.position).unwrap(), 1e-6);
    }
}

#[test]
fn source_stage_goldens_and_additive_stepping_are_exact() {
    let c = config();
    let r = run(
        &input([[0.0, 0.0], [64.0, 0.0], [0.0, 40.0]], [1.0; 3], c),
        c,
    )
    .unwrap();
    let source = r.source.as_ref().unwrap();
    let w = 2_i128 * 65536;
    assert_eq!(
        source.positions,
        [[0, 0, w], [64 * w, 0, w], [0, 40 * w, w]]
    );
    assert_eq!(
        source.fields,
        [
            [0, 0, 64 * 40 * w * w],
            [40 * w * w, 0, 0],
            [0, 64 * w * w, 0]
        ]
    );
    assert_eq!(source.determinant, 64 * 40 * w * w * w);
    let mut row = r.integer_pixel_fields(0, 0).unwrap();
    for y in 0..c.height {
        let mut values = row;
        for x in 0..c.width {
            assert_eq!(values, r.integer_pixel_fields(x, y).unwrap());
            for (v, f) in values.iter_mut().zip(source.fields) {
                *v += 2 * f[0];
            }
        }
        for (v, f) in row.iter_mut().zip(source.fields) {
            *v += 2 * f[1];
        }
    }
    assert!(r.integer_pixel_fields(c.width, 0).is_err());
    // A mathematically collinear source can become a nonzero snapped sliver.
    // Report the inconsistent source explicitly instead of an epsilon discard.
    let mut singular = r.input.clone();
    for (i, v) in singular.vertices.iter_mut().enumerate() {
        v.clip = [
            -30000 + 20000 * i as i32,
            -15000 + 10000 * i as i32,
            0,
            65536,
        ];
    }
    assert!(run(&singular, c).unwrap_err().contains("singular"));
}

#[test]
fn varied_sources_check_three_fields_without_reusing_cofactors() {
    let c = config();
    let mut seed = 0xace1_1234_5678_u64;
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..128 {
        let points = [
            [-8.0 - (random() % 20) as f64, -8.0],
            [64.0 + (random() % 20) as f64, 3.0],
            [20.0 + (random() % 20) as f64, 40.0 + (random() % 20) as f64],
        ];
        let mut input = input(
            points,
            std::array::from_fn(|_| 0.2 + (random() % 10000) as f64 / 100.0),
            c,
        );
        for v in &mut input.vertices {
            v.normal = std::array::from_fn(|_| random() as i16);
            v.uv = std::array::from_fn(|_| (random() % 4096) as u16);
            v.rgb565 = random() as u16;
        }
        let r = run(&input, c).unwrap();
        let samples = r.rasterize().unwrap();
        assert!(!samples.is_empty());
        for (_, _, s) in samples.iter().step_by(19) {
            compare_samples(s, &r.reference(s.position).unwrap(), 1e-8);
            compare_samples(s, &r.evaluate(s.position).unwrap(), 1e-8);
        }
    }
}

#[test]
fn vertex_local_edges_remove_four_products_with_identical_coverage() {
    for (_, input) in pressure() {
        for bits in [1, 4, 8] {
            let c = Config {
                subpixel_bits: bits,
                ..config()
            };
            let local = run(&input, c).unwrap();
            let global = run(
                &input,
                Config {
                    coverage_origin: CoverageOrigin::Global,
                    ..c
                },
            )
            .unwrap();
            assert_eq!(local.work.edge_products, 2 * local.work.edge_fans);
            assert_eq!(global.work.edge_products, 6 * global.work.edge_fans);
            assert_eq!(
                global.work.setup_products(Interpolation::Basis, true)
                    - local.work.setup_products(Interpolation::Basis, true),
                4 * local.work.edge_fans
            );
            let a = local.rasterize().unwrap();
            let b = global.rasterize().unwrap();
            assert_eq!(
                a.iter().map(|(p, _, _)| p).collect::<Vec<_>>(),
                b.iter().map(|(p, _, _)| p).collect::<Vec<_>>()
            );
            for ((_, _, a), (_, _, b)) in a.iter().zip(b) {
                compare_samples(a, &b, 1e-8);
            }
        }
    }
}

#[test]
fn constant_channels_survive_independent_plane_quantization() {
    let c = Config {
        interpolation: Interpolation::Planes,
        field_bits: Some(18),
        ..config()
    };
    let mut input = pressure()[2].1.clone();
    for v in &mut input.vertices {
        v.uv = [3011, 3001];
        v.normal = [-13111, 12345, 777];
        v.rgb565 = 0x1249;
    }
    let r = run(&input, c).unwrap();
    for (_, _, s) in r.rasterize().unwrap() {
        assert_eq!(s.quantized.normal, [-13111, 12345, 777]);
        assert_eq!(s.quantized.rgb565, 0x1249);
        assert_eq!(
            s.quantized.uv,
            [
                (3011.0 / 4095.0 * 131072.0_f64).round_ties_even() as i64,
                (3001.0 / 4095.0 * 131072.0_f64).round_ties_even() as i64
            ]
        );
    }
}

#[test]
fn exact_projection_ties_and_vertex_stage_bridge() {
    let c = config();
    for (x, expected) in [(80, 512), (240, 514), (-80, 512), (-240, 510)] {
        let mut input = input([[0.0, 0.0], [64.0, 0.0], [0.0, 40.0]], [1.25; 3], c);
        input.vertices[0].clip = [x, 128, 0, 81920];
        let r = run(&input, c).unwrap();
        assert_eq!(r.projected[0].snapped, [expected, 320]);
    }
    use gpu_v2::vertex::{
        ports::{Context, PackedVertex},
        sim::oracle,
    };
    let packed = [
        PackedVertex::encode([0, 0, 0], [127, 0, 127], [0, 0], 0xf800).unwrap(),
        PackedVertex::encode([1023, 0, 0], [127, 0, 127], [4095, 0], 0x07e0).unwrap(),
        PackedVertex::encode([0, 1023, 0], [127, 0, 127], [0, 4095], 0x001f).unwrap(),
    ];
    let vertices = packed.map(|p| {
        oracle::run(&Context::default(), p, &oracle::Config::default())
            .unwrap()
            .output
    });
    let input = Input { id: 123, vertices };
    let r = run(&input, c).unwrap();
    assert!(!r.rasterize().unwrap().is_empty());
    let s = r.evaluate([33.5, 18.5]).unwrap();
    assert_eq!(s.quantized.normal, [16256, 0, 16256]);
}
