use super::*;
use crate::system::functional::{Effects, Memory};

#[test]
fn billboard_metadata_selects_two_projected_offsets() {
    let header = [1, 2 << 8 | 20, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0];
    let payload = [0, 0, 0x03f8u16, 0x0004, 0x3210, 0x7654, 0, 0];
    let mesh = Meshlet::decode(&header, &payload).unwrap();
    let mut uniform = [0u32; 32];
    for i in [0, 5, 10, 21, 24, 27] {
        uniform[i] = 65536;
    }
    uniform[28] = (-32768i32) as u32;
    let vertex = mesh.transform(&uniform).unwrap()[0];
    assert_eq!(vertex.position, [-1024, 32768, 131584]);
    assert_eq!(vertex.normal, [0; 3]);
    assert_eq!(vertex.attribute, 0x7654_3210);
}

#[test]
fn full_depth_underflow_rejects_coplanar_coverage_but_keeps_d16_zero() {
    let vertices = |p: [[i128; 3]; 3]| {
        p.map(|position| Vertex {
            position,
            normal: [0, 0, 1024],
            attribute: 0xffffff,
        })
    };
    let coplanar = Triangle::new(
        vertices([
            [-106418, 1596, 196599],
            [106418, -532, -65533],
            [-106418, -532, -65533],
        ]),
        false,
    )
    .unwrap()
    .unwrap();
    let fragment = (0..240)
        .find_map(|y| {
            let (lo, hi) = coplanar.span(y)?;
            coplanar.fragment((lo + hi) / 2, y, 80 << 16).ok()
        })
        .expect("the rounded coplanar triangle must exercise false coverage");
    assert!(!fragment.valid);
    let close = Triangle::new(vertices([[-2, -2, 2], [2, -2, 2], [0, 2, 2]]), false)
        .unwrap()
        .unwrap();
    let fragment = close.fragment(200, 120, 80 << 16).unwrap();
    assert!(fragment.valid);
    assert_eq!(fragment.depth, 0);
}

#[test]
fn fixed_depth_preserves_unsigned_comparisons_and_write_enable() {
    struct Words([u16; 3]);
    impl shading::PixelMemory for Words {
        fn load(&self, address: usize) -> Result<u16, String> {
            Ok(self.0[address])
        }
        fn store(&mut self, address: usize, value: u16) -> Result<(), String> {
            self.0[address] = value;
            Ok(())
        }
    }
    let mut global = [0u8; 64];
    for (i, word) in [
        31u32 << 16,
        0,
        16384 | (8192 << 16),
        4915 | (10486 << 16),
        256,
        0,
    ]
    .into_iter()
    .enumerate()
    {
        global[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    let global = shading::Global::decode(global).unwrap();
    for function in 0..8 {
        for write in [false, true] {
            let mut bytes = [0u8; 32];
            bytes[8] = function | if write { 8 } else { 0 };
            let state = shading::DrawState {
                global,
                render: shading::RenderState::decode(bytes).unwrap(),
                target: [0, 2],
                color: 0xffffffff,
                rgb: true,
            };
            for (old, incoming) in [
                (0u16, 65534u16),
                (32767, 32768),
                (65535, 0),
                (60000, 59999),
                (1234, 1234),
            ] {
                let pass = [
                    false,
                    incoming < old,
                    incoming == old,
                    incoming <= old,
                    incoming > old,
                    incoming != old,
                    incoming >= old,
                    true,
                ][usize::from(function)];
                let mut memory = Words([0x1234, 0, old]);
                shading::rop(
                    &mut memory,
                    [0, 0],
                    incoming,
                    [255, 0, 0, (state.color >> 24) as u8],
                    &state,
                )
                .unwrap();
                assert_eq!(memory.0[0], if pass { 0xf800 } else { 0x1234 });
                assert_eq!(memory.0[2], if pass && write { incoming } else { old });
            }
        }
    }
}

#[test]
fn fixed_depth_far_is_inclusive_and_retired_tags_are_rejected() {
    let vertices = [(-65536, -65536), (65536, -65536), (0, 65536)].map(|(x, y)| Vertex {
        position: [x, y, 196675],
        normal: [0; 3],
        attribute: 0,
    });
    let triangle = Triangle::new(vertices, false).unwrap().unwrap();
    for (x, y) in [(199, 119), (200, 120), (190, 115)] {
        let fragment = triangle.fragment(x, y, 31 << 16).unwrap();
        assert!(fragment.valid);
        let same_bin_far = u32::from(fragment.depth) << 8;
        assert!(triangle.fragment(x, y, same_bin_far).unwrap().valid);
        assert!(!triangle.fragment(x, y, same_bin_far - 256).unwrap().valid);
    }
    for far in [
        0,
        200 << 16,
        (31 << 16) | (1 << 24),
        (31 << 16) | (2 << 24),
        (31 << 16) | (3 << 24),
    ] {
        let mut bytes = [0u8; 64];
        bytes[..4].copy_from_slice(&u32::to_le_bytes(far));
        assert!(shading::Global::decode(bytes).is_err());
    }
}

#[test]
fn row_bounds_keep_every_original_span() {
    eprintln!(
        "workspace bytes: quad={}, prepared_quad={}, spans={}",
        std::mem::size_of::<Quad>(),
        std::mem::size_of::<PreparedQuad>(),
        std::mem::size_of::<[Spans; 120]>()
    );
    let vertex = Vertex {
        position: [0; 3],
        normal: [0; 3],
        attribute: 0,
    };
    let mut seed = 0x517cc1b727220a95u64;
    for sample in 0..4096 {
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed >> 32) as i32 as i128
        };
        let edges = std::array::from_fn(|edge| {
            let nx = if sample % 7 == edge {
                0
            } else {
                next() % 65536
            };
            let ny = if sample % 11 == edge {
                0
            } else {
                next() % 65536
            };
            [nx, ny, next() % (1 << 27)]
        });
        let triangle = Triangle {
            vertices: [vertex; 3],
            edges,
            exponents: [0; 3],
            common_exponent: 0,
            depth_mantissa: 0,
            depth_shift: 0,
        };
        let rows = triangle.rows();
        for y in 0..240 {
            if triangle.span(y).is_some() {
                assert!(
                    rows.as_ref().is_some_and(|r| r.contains(&(y / 2))),
                    "sample {sample}, y {y}, edges {edges:?}, rows {rows:?}"
                );
            }
        }
    }
    for (edges, expected) in [
        ([[0, 0, 1]; 3], Some(0..120)),
        ([[0, 0, 0]; 3], None),
        ([[0, 1, -200], [0, -1, 204], [0, 0, 1]], Some(50..51)),
        ([[1, 0, -800], [0, 0, 1], [0, 0, 1]], None),
    ] {
        let triangle = Triangle {
            vertices: [vertex; 3],
            edges,
            exponents: [0; 3],
            common_exponent: 0,
            depth_mantissa: 0,
            depth_shift: 0,
        };
        assert_eq!(triangle.rows(), expected);
    }
}

// The pre-optimization traversal is intentionally independent of row bounds,
// job preparation, work aggregation and ordered retirement.
fn reference_draw(
    memory: &mut Memory<'_>,
    mesh: &Meshlet,
    uniform: &[u32; 32],
    state: shading::DrawState,
    report: &mut Report,
    maximum: usize,
) -> Result<(), String> {
    report.charge(mesh.vertices.len(), maximum)?;
    let vertices = mesh.transform(uniform)?;
    for indices in &mesh.triangles {
        report.charge(1, maximum)?;
        let Some(triangle) = Triangle::new(indices.map(|i| vertices[i]), mesh.cull)? else {
            continue;
        };
        for y in (0..240).step_by(2) {
            report.charge(2, maximum)?;
            let spans = [triangle.span(y), triangle.span(y + 1)];
            let Some(lo) = spans.iter().flatten().map(|s| s.0).min() else {
                continue;
            };
            let hi = spans.iter().flatten().map(|s| s.1).max().unwrap();
            for x in ((lo & !1)..=hi).step_by(2) {
                report.charge(4, maximum)?;
                let mut fragments = Vec::new();
                for lane in 0..4 {
                    let (px, py) = (x + (lane & 1), y + (lane >> 1));
                    if spans[lane >> 1].is_some_and(|(lo, hi)| (lo..=hi).contains(&px)) {
                        fragments.push((px, py, triangle.fragment(px, py, state.global.far)?));
                    }
                }
                let Some((_, _, first)) = fragments.first() else {
                    continue;
                };
                let lod = if mesh.rgb {
                    0
                } else {
                    triangle.lod(first, state.render)
                };
                for (px, py, fragment) in fragments {
                    if fragment.valid {
                        shading::fragment(
                            memory,
                            [px, py],
                            fragment,
                            &state,
                            lod,
                            report,
                            maximum,
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn parallel_retirement_preserves_memory_errors_and_partial_effects() {
    let mut uniform = [0; 32];
    for i in [0, 5, 10, 12, 16, 20, 21] {
        uniform[i] = 65536;
    }
    uniform[22] = 0x80ffffff;
    let mut global = [0; 64];
    for (i, value) in [
        100u32 * 65536,
        0,
        16384 | (8192 << 16),
        4915 | (10486 << 16),
        256,
        0,
    ]
    .into_iter()
    .enumerate()
    {
        global[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    let global = shading::Global::decode(global).unwrap();
    let target = [0x220000, 0x240000];
    for scenario in [
        "rgb",
        "texture",
        "feedback",
        "unknown-texture",
        "unknown-depth",
        "denied-color",
    ] {
        let rgb = scenario == "rgb";
        let mesh = Meshlet {
            vertices: [(0, 0, 255), (512, 0, 255 << 8), (256, 256, 255 << 16)]
                .map(|(x, y, color)| {
                    [
                        x | (y << 10),
                        511 << 20,
                        if rgb {
                            color
                        } else {
                            (x * 127) | ((y * 255) << 16)
                        },
                    ]
                })
                .to_vec(),
            triangles: vec![[0, 1, 2]; 2],
            origin: [-65536, -32768, 4 * 65536],
            shift: 8,
            normal18: false,
            billboard: false,
            uniform: 0,
            rgb,
            cull: false,
        };
        let mut render = [0; 32];
        let texture_word = if scenario == "feedback" {
            (target[0] / 64) as u32 | (9 << 16) | (1 << 23) | (1 << 21)
        } else {
            (0x4f0000 / 128) | (4 << 16) | (1 << 21)
        };
        render[..4].copy_from_slice(&texture_word.to_le_bytes());
        render[8] = 32 | 16 | 8 | 3; // Unlit, Q6 blend, depth write, LessEqual.
        let state = shading::DrawState {
            global,
            render: shading::RenderState::decode(render).unwrap(),
            target,
            color: uniform[22],
            rgb,
        };
        let triangle = Triangle::new(
            mesh.transform(&uniform).unwrap().try_into().ok().unwrap(),
            false,
        )
        .unwrap()
        .unwrap();
        let quads: usize = (0..120)
            .filter_map(|r| span_range([triangle.span(r * 2), triangle.span(r * 2 + 1)]))
            .map(|r| (r.end() - r.start()) / 2 + 1)
            .sum();
        assert!(quads >= 256, "fixture must exercise the parallel threshold");
        for maximum in [3, 40, 1000, 10000, 64000000] {
            let mut original = vec![0u16; 0x400000];
            for i in 0..super::super::PLANE_WORDS {
                original[target[0] + i] = (i as u16).wrapping_mul(2053);
                original[target[1] + i] = 65535;
            }
            original[0x4f0000 / 2..0x4f0000 / 2 + 256].fill(0xf81f);
            let mut flags = vec![3; 0x800000];
            match scenario {
                "unknown-texture" => flags[0x4f0000..0x4f0200].fill(0),
                "unknown-depth" => {
                    flags[(target[1] + super::super::column_offset(200, 120)) * 2] = 0
                }
                "denied-color" => {
                    flags[(target[0] + super::super::column_offset(200, 120)) * 2] = 1
                }
                _ => {}
            }
            let mut expected_words = original.clone();
            let mut expected_flags = flags.clone();
            let mut expected = Memory {
                words: &mut expected_words,
                access: Some(&mut expected_flags),
                effects: Effects::default(),
            };
            let mut expected_report = Report::default();
            let result = reference_draw(
                &mut expected,
                &mesh,
                &uniform,
                state,
                &mut expected_report,
                maximum,
            );
            let effects = expected.effects;
            if maximum == 64000000 {
                assert_eq!(
                    result.is_err(),
                    scenario.starts_with("unknown-") || scenario == "denied-color",
                    "fixture must reach its intended outcome: {scenario}"
                );
                if scenario != "unknown-texture" {
                    assert!(
                        effects.written_bytes.is_some(),
                        "fixture must retain writes: {scenario}"
                    );
                }
            }
            for threads in [1, 4] {
                let mut words = original.clone();
                let mut actual_flags = flags.clone();
                let mut actual = Memory {
                    words: &mut words,
                    access: Some(&mut actual_flags),
                    effects: Effects::default(),
                };
                let mut report = Report::default();
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .unwrap();
                let actual_result = pool
                    .install(|| draw(&mut actual, &mesh, &uniform, state, &mut report, maximum));
                assert_eq!(actual_result, result, "{scenario}, {maximum}, {threads}");
                assert_eq!(actual.effects, effects, "{scenario}, {maximum}, {threads}");
                if result.is_ok() {
                    assert_eq!(report, expected_report);
                }
                assert!(
                    words == expected_words,
                    "memory: {scenario}, {maximum}, {threads}"
                );
                assert!(
                    actual_flags == expected_flags,
                    "access: {scenario}, {maximum}, {threads}"
                );
            }
        }
    }
}
