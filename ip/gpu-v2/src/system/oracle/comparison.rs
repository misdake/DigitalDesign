//! Shared numerical A/B review. Host transports never duplicate frame arithmetic.
use super as oracle;
use super::{
    ports::*,
    scene::{self, Parameters},
};

pub const DEFAULTS: [Config; 2] = [
    Config {
        fetch: Fetch::Ideal,
        vertex: VertexMath::Continuous,
        vertex_normal: NormalFormat::Q14,
        pixel_normal: NormalFormat::Q14,
        fifo: [2; 6],
        max_steps: 1_000_000,
    },
    Config {
        fetch: Fetch::CompactV6,
        vertex: VertexMath::S16F16,
        vertex_normal: NormalFormat::S12F10,
        pixel_normal: NormalFormat::S12F10,
        fifo: [2; 6],
        max_steps: 1_000_000,
    },
];
pub struct Pair {
    pub rgba: Vec<u8>,
    pub values: Vec<u16>,
    pub stats: Vec<f64>,
}
fn stat(f: &Frame) -> Vec<f64> {
    let s = &f.stats;
    let mut v = vec![
        s.triangles as f64,
        s.quads as f64,
        s.fragments as f64,
        s.normal_clips as f64,
        s.color_saturations as f64,
        s.texture_refills as f64,
        s.framebuffer_refills as f64,
        s.framebuffer_writebacks as f64,
        s.pump_steps as f64,
    ];
    v.extend(s.fifo_peak.map(|x| x as f64));
    v
}
pub fn render(p: Parameters, configs: [Config; 2], gain: f64) -> Result<Pair, String> {
    if !gain.is_finite() || !(1.0..=64.0).contains(&gain) {
        return Err("diff gain".into());
    }
    let scene = scene::build(p)?;
    let a = oracle::render(&scene, configs[0])?;
    let b = if configs[0] != configs[1] {
        Some(oracle::render(&scene, configs[1])?)
    } else {
        None
    };
    combine(&a, b.as_ref().unwrap_or(&a), gain)
}

pub fn combine(a: &Frame, b: &Frame, gain: f64) -> Result<Pair, String> {
    if !gain.is_finite() || !(1.0..=64.0).contains(&gain) {
        return Err("diff gain".into());
    }
    let n = a.color.len();
    if n == 0
        || [a, b].iter().any(|f| {
            f.color.len() != n
                || f.rgba.len() != n * 4
                || f.depth.len() != n
                || f.lighting.len() != n
        })
    {
        return Err("comparison frame shape".into());
    }
    let mut out = Pair {
        rgba: Vec::with_capacity(n * 12),
        values: Vec::with_capacity(n * 6),
        stats: stat(a),
    };
    out.rgba.extend_from_slice(&a.rgba);
    out.rgba.extend_from_slice(&b.rgba);
    out.stats.extend(stat(b));
    let mut different = 0;
    let mut max = 0;
    let mut sum = 0;
    let mut depth = 0;
    let mut dg = 0;
    let mut dh = 0;
    for i in 0..n {
        different += usize::from(a.color[i] != b.color[i]);
        depth += usize::from(a.depth[i] != b.depth[i]);
        let deltas =
            std::array::from_fn::<_, 3, _>(|c| a.rgba[4 * i + c].abs_diff(b.rgba[4 * i + c]));
        max = max.max(*deltas.iter().max().unwrap());
        sum += deltas.iter().map(|x| *x as u64).sum::<u64>();
        out.rgba
            .extend(deltas.map(|v| (v as f64 * gain).min(255.0) as u8));
        out.rgba.push(255);
        dg = dg.max(a.lighting[i][0].abs_diff(b.lighting[i][0]));
        dh = dh.max(a.lighting[i][1].abs_diff(b.lighting[i][1]));
        out.values.extend([
            a.lighting[i][0],
            a.lighting[i][1],
            b.lighting[i][0],
            b.lighting[i][1],
            a.depth[i],
            b.depth[i],
        ]);
    }
    let mut clip_error = 0.0_f64;
    let mut normal_error = 0.0_f64;
    for (x, y) in a.vertex_boundaries.iter().zip(&b.vertex_boundaries) {
        for (&u, &v) in x.clip.iter().flatten().zip(y.clip.iter().flatten()) {
            clip_error = clip_error.max((u - v).abs());
        }
        for (u, v) in x.attributes.iter().zip(&y.attributes) {
            for k in 5..8 {
                normal_error = normal_error.max((u[k] - v[k]).abs());
            }
        }
    }
    out.stats.extend([
        different as f64,
        max as f64,
        sum as f64 / (n * 3) as f64,
        depth as f64,
        dg as f64,
        dh as f64,
        clip_error,
        normal_error,
        a.stats.helper_fallbacks as f64,
        b.stats.helper_fallbacks as f64,
    ]);
    Ok(out)
}
