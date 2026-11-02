use super::*;
use crate::system::functional::{Effects, Memory};

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
