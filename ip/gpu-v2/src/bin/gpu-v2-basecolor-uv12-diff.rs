//! Finite dynamic study export; not whole-GPU or reciprocal RTL proof.
use gpu_v2::{
    material_color::ColorSource as Mode,
    triangle::{
        basecolor_threefield::{PreparedBasis, Profile},
        basecolor_uv12::recover,
    },
};
#[path = "../../tests/common/basecolor_uv12_fixture.rs"]
mod common;
use std::{fs, path::PathBuf};
#[derive(Clone, Debug, Default)]
struct Metrics {
    points: usize,
    uv_max: f64,
    uv_sq: f64,
    rgb_max: f64,
    rgb_sq: f64,
    angle_max: f64,
    angle_sq: f64,
    norm_max: f64,
    norm_sq: f64,
    lod_max: f64,
    lod_sq: f64,
    quads: usize,
    negative_helpers: usize,
    exponent_max: i16,
}
fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dot = a.into_iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
    let la = a.into_iter().map(|v| v * v).sum::<f64>().sqrt();
    let lb = b.into_iter().map(|v| v * v).sum::<f64>().sqrt();
    (dot / (la * lb)).clamp(-1.0, 1.0).acos().to_degrees()
}
impl Metrics {
    fn json(&self) -> String {
        let n = self.points.max(1) as f64;
        let q = self.quads.max(1) as f64;
        format!(
            "{{\"points\":{},\"uv_texels_max\":{},\"uv_texels_rms\":{},\"rgb8_max\":{},\"rgb8_rms\":{},\"normal_angle_deg_max\":{},\"normal_angle_deg_rms\":{},\"normal_length_max\":{},\"normal_length_rms\":{},\"lod_max\":{},\"lod_rms\":{},\"quads\":{},\"negative_helpers\":{},\"weight_exponent_max\":{}}}",
            self.points,
            self.uv_max,
            (self.uv_sq / n).sqrt(),
            self.rgb_max,
            (self.rgb_sq / n).sqrt(),
            self.angle_max,
            (self.angle_sq / n).sqrt(),
            self.norm_max,
            (self.norm_sq / n).sqrt(),
            self.lod_max,
            (self.lod_sq / q).sqrt(),
            self.quads,
            self.negative_helpers,
            self.exponent_max
        )
    }
}
fn main() {
    let out = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("owned web output directory"),
    );
    fs::create_dir_all(&out).unwrap();
    let names = [
        "Ordinary texture motion",
        "Large triangle",
        "Strong perspective",
        "Vertex color gradient",
        "Normal and per-pixel specular",
        "Thin surface and extrapolated helpers",
        "Clipped fan shared source",
        "Moving high frequency UV",
        "LOD threshold motion",
    ];
    let mut scenes = Vec::new();
    let mut metrics = vec![vec![Metrics::default(); 3]; 4];
    let mut cases = Vec::new();
    let mut original_invalid_helpers = 0;
    let mut exact_points = 0;
    const FRAMES: usize = 48;
    for (scene, name) in names.into_iter().enumerate() {
        let mut frames = Vec::new();
        let mut scene_metrics = vec![vec![Metrics::default(); 3]; 4];
        let mut case_points = 0;
        for frame in 0..FRAMES {
            let (r, a, mode) = common::fixture(scene, frame as f64 / FRAMES as f64);
            let b = PreparedBasis::prepare(&r, a, mode).unwrap();
            let nd = b.narrow_delta.map(|d| d.map(|v| v.value()));
            let triangles = r
                .triangles
                .iter()
                .map(|t| {
                    format!(
                        "{{\"o\":{:?},\"e\":{:?},\"bbox\":{:?}}}",
                        t.origin,
                        t.edges.map(|e| [e.a, e.b, e.c, i128::from(e.top_left)]),
                        t.bbox
                    )
                })
                .collect::<Vec<_>>();
            frames.push(format!("{{\"c\":{:?},\"anchor\":{:?},\"radius\":{},\"k\":{},\"source\":{:?},\"delta\":{:?},\"nd\":{:?},\"constant\":{:?},\"channels\":{},\"triangles\":[{}]}}",b.coefficients,b.anchor,b.radius,b.depth_k,b.source,b.delta,nd,b.constant,b.channels,triangles.join(",")));
            for y in (4..236).step_by(12) {
                for x in (4..396).step_by(12) {
                    if !r
                        .triangles
                        .iter()
                        .any(|t| t.contains(x, y, r.config.subpixel_bits))
                    {
                        continue;
                    }
                    let point = [f64::from(x) + 0.5, f64::from(y) + 0.5];
                    let g = r.reference(point).unwrap();
                    let exact = b.recover(point, Profile::Exact, false).unwrap();
                    for (u, v) in exact.beta.into_iter().zip(g.beta) {
                        assert!((u - v).abs() < 1e-8);
                    }
                    exact_points += 1;
                    let pp = std::array::from_fn::<_, 4, _>(|i| {
                        [
                            f64::from(x & !1) + 0.5 + (i % 2) as f64,
                            f64::from(y & !1) + 0.5 + (i / 2) as f64,
                        ]
                    });
                    let helpers = pp.map(|p| r.evaluate(p));
                    let original_valid = helpers.iter().all(|h| h.is_ok());
                    if !original_valid {
                        original_invalid_helpers += 1;
                    }
                    for (pi, bits) in [9u8, 10, 11, 12].into_iter().enumerate() {
                        for stage in 0..3 {
                            let p = if stage == 1 { None } else { Some(bits) };
                            let narrow = stage != 0;
                            let value = recover(&b, point, p, narrow).unwrap();
                            if let Some(integer) = value.integer_q16 {
                                for (raw, float) in
                                    integer.into_iter().zip(value.raw).take(b.channels)
                                {
                                    assert!(
                                        (raw as f64 / 65536.0 - float).abs() <= 0.5001 / 65536.0
                                    );
                                }
                                if mode == Mode::Texture {
                                    assert_eq!(
                                        value.uv_transport_q16().unwrap(),
                                        value.uv().map(|u| (u * 65536.0).round_ties_even() as i128)
                                    );
                                }
                            }
                            for m in [&mut metrics[pi][stage], &mut scene_metrics[pi][stage]] {
                                m.points += 1;
                                let ne = angle(value.normal(mode), g.normal);
                                m.angle_max = m.angle_max.max(ne);
                                m.angle_sq += ne * ne;
                                let norm =
                                    |n: [f64; 3]| n.into_iter().map(|v| v * v).sum::<f64>().sqrt();
                                let length_error =
                                    (norm(value.normal(mode)) - norm(g.normal)).abs();
                                m.norm_max = m.norm_max.max(length_error);
                                m.norm_sq += length_error * length_error;
                                if mode == Mode::Texture {
                                    let error = value
                                        .uv()
                                        .into_iter()
                                        .zip(g.uv)
                                        .map(|(u, v)| (u - v).abs() * 256.0)
                                        .fold(0.0f64, f64::max);
                                    m.uv_max = m.uv_max.max(error);
                                    m.uv_sq += error * error;
                                    if original_valid {
                                        let uv = pp.map(|point| {
                                            recover(&b, point, p, narrow).unwrap().uv()
                                        });
                                        let gold = std::array::from_fn(|i| {
                                            helpers[i].as_ref().unwrap().uv
                                        });
                                        let error = (common::lod(uv, 256.0)
                                            - common::lod(gold, 256.0))
                                        .abs();
                                        m.lod_max = m.lod_max.max(error);
                                        m.lod_sq += error * error;
                                        m.quads += 1;
                                        for point in pp {
                                            let h = recover(&b, point, p, narrow).unwrap();
                                            if h.beta.iter().any(|&v| v < 0.0) {
                                                m.negative_helpers += 1;
                                            }
                                            for w in h.weights {
                                                m.exponent_max = m.exponent_max.max(w.exponent);
                                            }
                                        }
                                    }
                                } else {
                                    let error = value.raw[..3]
                                        .iter()
                                        .zip(g.rgb)
                                        .map(|(u, v)| (u - v * 255.0).abs())
                                        .fold(0.0f64, f64::max);
                                    m.rgb_max = m.rgb_max.max(error);
                                    m.rgb_sq += error * error;
                                }
                            }
                            if frame == 0 && case_points < 3 {
                                cases.push(format!("{{\"scene\":{scene},\"point\":{:?},\"profile\":{pi},\"stage\":{stage},\"raw\":{:?},\"beta\":{:?},\"w\":{}}}",point,value.raw,value.represented,value.w));
                            }
                        }
                    }
                    if frame == 0 {
                        case_points += 1;
                    }
                }
            }
        }
        let mj = scene_metrics
            .iter()
            .map(|p| {
                format!(
                    "[{}]",
                    p.iter().map(Metrics::json).collect::<Vec<_>>().join(",")
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        scenes.push(format!(
            "{{\"name\":{name:?},\"texture\":{},\"frames\":[{}],\"metrics\":[{mj}]}}",
            scene != 3 && scene != 4,
            frames.join(",\n")
        ));
    }
    let mj = metrics
        .iter()
        .map(|p| {
            format!(
                "[{}]",
                p.iter().map(Metrics::json).collect::<Vec<_>>().join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let data = format!(
        "{{\"status\":\"finite-dynamic-UV-Q12-split-hi9-lo3-study\",\"viewport\":[400,240],\"texture_extent\":256,\"frames_per_scene\":{FRAMES},\"exact_points\":{exact_points},\"original_invalid_helpers\":{original_invalid_helpers},\"precision\":\"original source32/UV12/normal12/RGB8; no clip/project/field/reciprocal precision changed; UV Q9 rejected for LOD; UV Q10/Q11/Q12 single-round binary weights; Q12 hi9+lo3 unsigned9 products plus charged fabric correction; Color/normal independently Q9; optional delta9; all experiments unaccepted\",\"stages\":[\"UV weight bits only; normal/Color Q9\",\"delta9 only\",\"UV weight bits and delta9\"],\"metrics\":[{mj}],\"scenes\":[{}],\"browser_golden_cases\":[{}]}}",
        scenes.join(",\n"),
        cases.join(",\n")
    );
    fs::write(out.join("numeric-data.json"), &data).unwrap();
    fs::write(
        out.join("frames.js"),
        format!("window.BASECOLOR_DIFF_DATA={data};\n"),
    )
    .unwrap();
    println!(
        "PASS finite dynamic scenes={} frames/scene={FRAMES} exact-points={exact_points} original-invalid-helper-quads={original_invalid_helpers}",
        common::SCENES
    );
}
