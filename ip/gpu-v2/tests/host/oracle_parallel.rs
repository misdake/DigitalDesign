#![cfg(feature = "parallel")]
use gpu_v2::system::oracle::{self, comparison::DEFAULTS, parallel, ports::*, scene};

#[test]
fn rayon_stages_preserve_pixels_boundaries_cache_and_fifo_statistics() {
    for threads in [1, 2, 4] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        for id in 0..4 {
            let scene = scene::build(scene::Parameters {
                scene: id,
                width: 200,
                frame: 30,
                ..Default::default()
            })
            .unwrap();
            for c in DEFAULTS {
                let c = Config {
                    fifo: if threads == 1 { [1; 6] } else { [8; 6] },
                    ..c
                };
                let expected = oracle::render(&scene, c).unwrap();
                let actual = pool.install(|| parallel::render(&scene, c)).unwrap();
                assert_eq!(actual.color, expected.color, "scene{id}/threads{threads}");
                assert_eq!(actual.rgba, expected.rgba);
                assert_eq!(actual.depth, expected.depth);
                assert_eq!(actual.lighting, expected.lighting);
                assert_eq!(actual.stats, expected.stats);
                for (a, b) in actual
                    .vertex_boundaries
                    .iter()
                    .zip(&expected.vertex_boundaries)
                {
                    assert_eq!(a.clip, b.clip);
                    assert_eq!(a.attributes, b.attributes);
                }
                assert_eq!(
                    actual.vertex_boundaries.len(),
                    expected.vertex_boundaries.len()
                );
            }
        }
    }
}

#[test]
fn parallel_composition_retains_budget_and_fifo_rejection() {
    let scene = scene::build(scene::Parameters {
        scene: 2,
        width: 200,
        ..Default::default()
    })
    .unwrap();
    assert!(parallel::render(
        &scene,
        Config {
            max_steps: 1,
            ..Default::default()
        }
    )
    .is_err());
    assert!(parallel::render(
        &scene,
        Config {
            fifo: [0; 6],
            ..Default::default()
        }
    )
    .is_err());
    assert!(parallel::render_pair(scene::Parameters::default(), DEFAULTS, f64::NAN).is_err());
}

#[test]
fn unreferenced_vertices_are_not_fetched_or_transformed() {
    let mut s = scene::build(scene::Parameters {
        scene: 2,
        width: 200,
        ..Default::default()
    })
    .unwrap();
    s.vertices.push(MeshVertex {
        position: [32768.; 3],
        normal: [0.; 3],
        uv: [0.; 2],
        tint: [0.; 3],
    });
    let a = oracle::render(&s, DEFAULTS[1]).unwrap();
    let b = parallel::render(&s, DEFAULTS[1]).unwrap();
    assert_eq!(a.rgba, b.rgba);
    assert_eq!(a.stats, b.stats);
}

#[test]
fn overlapping_equal_depth_triangles_keep_submission_order() {
    use gpu_v2::framebuffer::ports::DepthFunc;
    let mut s = scene::build(scene::Parameters {
        scene: 2,
        width: 200,
        ..Default::default()
    })
    .unwrap();
    let original = s.triangles.clone();
    let offset = s.vertices.len();
    let second: Vec<_> = s
        .vertices
        .iter()
        .copied()
        .map(|mut v| {
            v.tint = [0.1, 0.7, 0.2];
            v
        })
        .collect();
    s.vertices.extend(second);
    s.triangles
        .extend(original.iter().map(|t| t.map(|i| i + offset)));
    s.rop.depth = DepthFunc::LessEqual;
    let a = oracle::render(&s, DEFAULTS[0]).unwrap();
    let b = parallel::render(&s, DEFAULTS[0]).unwrap();
    assert_eq!(a.color, b.color);
    assert_eq!(a.depth, b.depth);
    assert_eq!(a.stats, b.stats);
    s.triangles.reverse();
    let reversed = oracle::render(&s, DEFAULTS[0]).unwrap();
    assert_ne!(
        a.color, reversed.color,
        "fixture must expose winner ordering"
    );
    let b = parallel::render(&s, DEFAULTS[0]).unwrap();
    assert_eq!(b.color, reversed.color);
    assert_eq!(b.lighting, reversed.lighting);
}
