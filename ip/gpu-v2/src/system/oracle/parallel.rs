//! Optional host acceleration. Pure values are computed in parallel, then the
//! original bounded FIFO/cache/ROP pump commits them in exactly its original order.
//! Thread execution is not a GPU timing model or a proposed hardware buffer.
use super::*;
use rayon::prelude::*;

const MAX_CACHED_QUADS: usize = 65_536;

fn prepare(scene: &Scene, config: Config) -> Result<PreparedFrame, String> {
    let c = preflight(scene, config)?;
    let mut used = vec![false; scene.vertices.len()];
    for &i in scene.triangles.iter().flatten() {
        used[i] = true;
    }
    let fetched: Vec<_> = scene
        .vertices
        .par_iter()
        .enumerate()
        .map(|(id, &v)| {
            if used[id] {
                fetch_vertex(v, &c, config.fetch)
            } else {
                Ok(v)
            }
        })
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<Result<_, _>>()?;
    let vertices: Vec<_> = fetched
        .par_iter()
        .enumerate()
        .map(|(id, &v)| {
            if used[id] {
                transform_vertex(v, scene, &c, config)
            } else {
                Ok(([0.; 4], [0.; 8], 0))
            }
        })
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<Result<_, _>>()?;
    let transformed: Vec<_> = scene
        .triangles
        .par_iter()
        .enumerate()
        .map(|(id, triangle)| {
            (
                TransformedTriangle {
                    id: id as u32,
                    clip: triangle.map(|i| vertices[i].0),
                    attributes: triangle.map(|i| vertices[i].1),
                },
                triangle.iter().map(|&i| vertices[i].2).sum(),
            )
        })
        .collect();
    let setups: Vec<_> = transformed
        .par_iter()
        .map(|(t, _)| {
            to::run_continuous(
                t.id,
                t.clip,
                t.attributes,
                tp::Config {
                    width: scene.width,
                    height: scene.height,
                    max_samples: 1_000_000,
                    ..Default::default()
                },
            )
        })
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<Result<_, _>>()?;
    let mut candidates = Vec::new();
    for report in &setups {
        let r = Raster::new(report.clone());
        if r.done {
            continue;
        }
        let count =
            usize::from((r.end[0] - r.xy[0]) / 2 + 1) * usize::from((r.end[1] - r.xy[1]) / 2 + 1);
        if candidates.len() + count > config.max_steps.min(1_000_000) {
            return Err("parallel raster scan budget".into());
        }
        for y in (r.xy[1]..=r.end[1]).step_by(2) {
            for x in (r.xy[0]..=r.end[0]).step_by(2) {
                candidates.push((report.input.id, [x, y]));
            }
        }
    }
    let covered: Vec<_> = candidates
        .into_par_iter()
        .filter_map(|(id, xy)| match mask_at(&setups[id as usize], xy) {
            Ok(0) => None,
            Ok(_) => Some(Ok((id, xy))),
            Err(e) => Some(Err(e)),
        })
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<Result<_, _>>()?;
    if covered.len() > MAX_CACHED_QUADS {
        return Err("parallel cached quad budget".into());
    }
    let (cache, image) = texture()?;
    let quads = covered
        .into_par_iter()
        .map(|(id, xy)| {
            let raster = quad_at(&setups[id as usize], xy)?.ok_or("parallel coverage mismatch")?;
            let mut stats = Stats::default();
            let mut shaded = light_quad(&raster, scene, config, &mut stats)?;
            let texture = if scene.textured {
                let prepared = sampler::prepare(
                    &texture_input(&raster),
                    cache.slots(),
                    tex::Config::counted(),
                )?;
                let mut captured = Vec::new();
                for pixel in &prepared.pixels {
                    let mut groups = Vec::new();
                    for group in &pixel.groups {
                        let address = group.key.address(cache.slots())? as usize;
                        let mut taps = [0; 4];
                        for (tap, out) in taps.iter_mut().enumerate() {
                            let x = (usize::from(group.top_left_local[0]) + tap % 2) & 7;
                            let y = (usize::from(group.top_left_local[1]) + tap / 2) & 7;
                            let offset = address + (y * 8 + x) * 2;
                            let bytes = image
                                .0
                                .get(offset..offset + 2)
                                .ok_or("parallel texture capture address")?;
                            *out = u16::from_le_bytes(bytes.try_into().unwrap());
                        }
                        groups.push(taps);
                    }
                    captured.push(groups);
                }
                let output = sampler::resolve(prepared, captured, Vec::new())?;
                for pixel in &output.pixels {
                    shaded.texture[pixel.lane as usize] = pixel.rgb;
                }
                Some(output)
            } else {
                None
            };
            Ok((
                (id, xy[0], xy[1]),
                PreparedQuad {
                    raster,
                    shaded,
                    texture,
                    stats,
                },
            ))
        })
        .collect::<Vec<Result<_, String>>>()
        .into_iter()
        .collect::<Result<_, _>>()?;
    Ok(PreparedFrame {
        fetched,
        transformed,
        setups,
        quads,
    })
}

pub fn render(scene: &Scene, config: Config) -> Result<Frame, String> {
    let prepared = prepare(scene, config)?;
    render_impl(scene, config, Some(&prepared))
}

pub fn render_pair(
    p: scene::Parameters,
    configs: [Config; 2],
    gain: f64,
) -> Result<comparison::Pair, String> {
    if !gain.is_finite() || !(1.0..=64.0).contains(&gain) {
        return Err("diff gain".into());
    }
    let scene = scene::build(p)?;
    if configs[0] == configs[1] {
        let a = render(&scene, configs[0])?;
        return comparison::combine(&a, &a, gain);
    }
    let (a, b) = rayon::join(|| render(&scene, configs[0]), || render(&scene, configs[1]));
    comparison::combine(&a?, &b?, gain)
}
