//! Pure functional GPU chain. Owns bounded transport, never invokes counted,
//! timed, emu or a host-rendered framebuffer golden. Numerical kernels are the
//! production component oracles; the browser adapter only exposes this module.
pub mod comparison;
pub mod fifo;
#[cfg(feature = "parallel")]
pub mod parallel;
pub mod ports;
pub mod scene;
use crate::{
    framebuffer::{
        ports as fb,
        sim::{functional, oracle as rop},
    },
    lighting::{ports as lp, sim::oracle as lo},
    system::pixel::final_rgb,
    texture::{ports as tex, sim::oracle as sampler},
    triangle::{ports as tp, sim::oracle as to},
    vertex::{ports as vp, sim::oracle as vo},
};
use fifo::Fifo;
use ports::*;

fn byte(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8
}
fn raw16(v: f64) -> Result<i32, String> {
    let q = (v * 65536.0).round_ties_even();
    if !q.is_finite() || q < i32::MIN as f64 || q > i32::MAX as f64 {
        return Err("S16F16 overflow".into());
    }
    Ok(q as i32)
}
fn context(scene: &Scene) -> Result<vp::Context, String> {
    let mut mvp = [[0; 4]; 4];
    for (row, out) in scene.mvp.iter().zip(&mut mvp) {
        for (&v, q) in row.iter().zip(out) {
            *q = raw16(v)?;
        }
    }
    let mut normal_matrix = [[0; 3]; 3];
    for (row, out) in scene.normal_matrix.iter().zip(&mut normal_matrix) {
        for (&v, q) in row.iter().zip(out) {
            let raw = (v * 16384.0).round_ties_even();
            if !raw.is_finite() || !(-32768.0..=32767.0).contains(&raw) {
                return Err("normal matrix Q14 overflow".into());
            }
            *q = raw as i16;
        }
    }
    let c = vp::Context {
        mvp,
        normal_matrix,
        base: scene.compact_base,
        grid_shift: scene.compact_grid_shift,
    };
    c.validate()?;
    Ok(c)
}
pub fn fetch_vertex(v: MeshVertex, c: &vp::Context, mode: Fetch) -> Result<MeshVertex, String> {
    if mode == Fetch::Ideal {
        return Ok(v);
    }
    c.validate()?;
    let mut xyz = [0; 3];
    for (i, coordinate) in xyz.iter_mut().enumerate() {
        let raw = ((v.position[i] * 65536.0 - c.base[i] as f64) / (1_u64 << c.grid_shift) as f64)
            .round_ties_even();
        if !(0.0..=1023.0).contains(&raw) {
            return Err("vertex outside compact meshlet grid".into());
        }
        *coordinate = raw as u16;
    }
    let packed = vp::PackedVertex::encode(
        xyz,
        v.normal
            .map(|x| (x * 128.0).round_ties_even().clamp(-128.0, 127.0) as i8),
        v.uv.map(|x| (x.clamp(0.0, 1.0) * 4095.0).round_ties_even() as u16),
        rop::quantize(v.tint.map(byte)),
    )?;
    let (position, normal, uv, color) = vo::unpack(c, packed)?;
    // RGB565 attributes interpolate mathematical UNORM codes, as triangle oracle.
    Ok(MeshVertex {
        position: position.map(|x| x as f64 / 65536.0),
        normal: normal.map(|x| x as f64 / 16384.0),
        uv: uv.map(|x| x as f64 / 4095.0),
        tint: [
            (color >> 11) as f64 / 31.0,
            ((color >> 5) & 63) as f64 / 63.0,
            (color & 31) as f64 / 31.0,
        ],
    })
}
fn transform_vertex(
    v: MeshVertex,
    scene: &Scene,
    c: &vp::Context,
    config: Config,
) -> Result<([f64; 4], [f64; 8], usize), String> {
    let (position, normal) = if config.vertex == VertexMath::Continuous {
        vo::transform_continuous(scene.mvp, scene.normal_matrix, v.position, v.normal)?
    } else {
        let mut raw = [0; 3];
        for (k, coordinate) in raw.iter_mut().enumerate() {
            *coordinate = raw16(v.position[k])?;
        }
        let nr = v
            .normal
            .map(|x| (x * 16384.0).round_ties_even().clamp(-32768.0, 32767.0) as i16);
        let report = vo::transform_quantized(c, raw, nr, [0; 2], 0, &vo::Config::default())?;
        (
            report.output.clip.map(|x| x as f64 / 65536.0),
            report.output.normal.map(|x| x as f64 / 1024.0),
        )
    };
    let clipped = usize::from(config.vertex_normal.clipped(normal));
    let normal = config.vertex_normal.quantize(normal)?;
    Ok((
        position,
        [
            v.uv[0], v.uv[1], v.tint[0], v.tint[1], v.tint[2], normal[0], normal[1], normal[2],
        ],
        clipped,
    ))
}

pub fn transform(
    input: FetchedTriangle,
    scene: &Scene,
    c: &vp::Context,
    config: Config,
    stats: &mut Stats,
) -> Result<TransformedTriangle, String> {
    let mut clip = [[0.0; 4]; 3];
    let mut attributes = [[0.0; 8]; 3];
    for (i, v) in input.vertices.into_iter().enumerate() {
        let (position, attr, clipped) = transform_vertex(v, scene, c, config)?;
        stats.normal_clips += clipped;
        clip[i] = position;
        attributes[i] = attr;
    }
    Ok(TransformedTriangle {
        id: input.id,
        clip,
        attributes,
    })
}

struct Raster {
    report: to::Report,
    xy: [u16; 2],
    end: [u16; 2],
    start_x: u16,
    done: bool,
}
impl Raster {
    fn new(report: to::Report) -> Self {
        let mut lo = [u16::MAX; 2];
        let mut hi = [0; 2];
        for t in &report.triangles {
            for k in 0..2 {
                lo[k] = lo[k].min(t.bbox[k] & !1);
                hi[k] = hi[k].max(t.bbox[k + 2] & !1);
            }
        }
        let done = report.triangles.is_empty();
        Self {
            report,
            xy: lo,
            end: hi,
            start_x: lo[0],
            done,
        }
    }
    fn advance(&mut self) -> Option<[u16; 2]> {
        if self.done {
            return None;
        }
        let xy = self.xy;
        if self.xy[0] == self.end[0] {
            self.xy[0] = self.start_x;
            if self.xy[1] == self.end[1] {
                self.done = true;
            } else {
                self.xy[1] += 2;
            }
        } else {
            self.xy[0] += 2;
        }
        Some(xy)
    }
    fn next(&mut self, prepared: Option<&PreparedFrame>) -> Result<Option<RasterQuad>, String> {
        let Some(xy) = self.advance() else {
            return Ok(None);
        };
        if let Some(prepared) = prepared {
            return Ok(prepared
                .quads
                .get(&(self.report.input.id, xy[0], xy[1]))
                .map(|q| q.raster.clone()));
        }
        quad_at(&self.report, xy)
    }
}
fn mask_at(report: &to::Report, xy: [u16; 2]) -> Result<u8, String> {
    let mut mask = 0;
    for lane in 0..4 {
        let x = xy[0] + (lane % 2) as u16;
        let y = xy[1] + (lane / 2) as u16;
        let count = report
            .triangles
            .iter()
            .filter(|t| t.contains(x, y, report.config.subpixel_bits))
            .count();
        if count > 1 {
            return Err("snapped clip fans overlap".into());
        }
        if count == 1 {
            mask |= 1 << lane;
        }
    }
    Ok(mask)
}
fn quad_at(report: &to::Report, xy: [u16; 2]) -> Result<Option<RasterQuad>, String> {
    let mask = mask_at(report, xy)?;
    if mask == 0 {
        return Ok(None);
    }
    let mut samples: [Option<tp::Sample>; 4] = std::array::from_fn(|_| None);
    let mut invalid_helpers = 0;
    for (lane, sample) in samples.iter_mut().enumerate() {
        match report.evaluate([
            xy[0] as f64 + 0.5 + (lane % 2) as f64,
            xy[1] as f64 + 0.5 + (lane / 2) as f64,
        ]) {
            Ok(value) => *sample = Some(value),
            Err(e) if mask & (1 << lane) != 0 => {
                return Err(format!(
                    "covered triangle {} quad {xy:?} lane {lane}: {e}",
                    report.input.id
                ))
            }
            Err(e) if e == "nonpositive/nonfinite interpolated W or attributes" => {
                invalid_helpers |= 1 << lane
            }
            Err(e) => return Err(e),
        }
    }
    let fallback = samples
        .iter()
        .enumerate()
        .find(|(lane, _)| mask & (1 << lane) != 0)
        .and_then(|(_, s)| s.clone())
        .ok_or("quad has no valid covered sample")?;
    let samples = samples.map(|s| s.unwrap_or_else(|| fallback.clone()));
    Ok(Some(RasterQuad {
        triangle: report.input.id,
        xy,
        mask,
        samples,
        invalid_helpers,
    }))
}

struct TextureImage(Vec<u8>);
impl tex::MemoryPort for TextureImage {
    fn read_dma(&mut self, address: u64, bytes: usize) -> Result<Vec<u64>, String> {
        if bytes == 0 || !bytes.is_multiple_of(8) || !address.is_multiple_of(8) {
            return Err("texture DMA shape".into());
        }
        let start = usize::try_from(address).map_err(|_| "texture address")?;
        let end = start.checked_add(bytes).ok_or("texture end")?;
        let data = self
            .0
            .get(start..end)
            .ok_or("texture address outside image")?;
        Ok(data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|v| u64::from_le_bytes(*v))
            .collect())
    }
}
fn texture() -> Result<(sampler::Cache, TextureImage), String> {
    let slot = tex::Slot {
        base_address: 0,
        has_full_mip: true,
        max_size_log2: 5,
        valid: true,
    };
    let mut bytes = vec![0; 24 * 128];
    let palette = [rop::quantize([220, 135, 64]), rop::quantize([55, 175, 210])];
    for n in 0_u8..=5 {
        let side = 1_usize << n;
        let tiles = 1_usize << n.saturating_sub(3);
        for y in 0..side {
            for x in 0..side {
                let checker = if n >= 3 {
                    (x >> (n - 3)) ^ (y >> (n - 3))
                } else {
                    0
                };
                let color = if n < 3 {
                    rop::quantize([137, 155, 137])
                } else {
                    palette[checker & 1]
                };
                let tile = tex::layer_offset(n)? as usize + (y / 8) * tiles + x / 8;
                let offset = tile * 128 + ((y & 7) * 8 + (x & 7)) * 2;
                bytes[offset..offset + 2].copy_from_slice(&color.to_le_bytes());
            }
        }
    }
    Ok((sampler::Cache::new(vec![slot])?, TextureImage(bytes)))
}
fn light_quad(
    q: &RasterQuad,
    scene: &Scene,
    config: Config,
    stats: &mut Stats,
) -> Result<ShadedQuad, String> {
    stats.helper_fallbacks += usize::from(scene.textured && tex::capture_uv(&texture_input(q))?.1);
    let mut out = ShadedQuad {
        header: fb::Header {
            x: q.xy[0],
            y: q.xy[1] as u8,
            mask: q.mask,
        },
        depth: q.samples.clone().map(|s| s.quantized.depth),
        tint: q.samples.clone().map(|s| s.rgb.map(byte)),
        texture: [[255; 3]; 4],
        lighting: [lp::LightingOutput { g: 0, h: 0 }; 4],
    };
    for (lane, s) in q.samples.iter().enumerate() {
        if q.mask & (1 << lane) == 0 {
            continue;
        }
        stats.fragments += 1;
        stats.normal_clips += usize::from(config.pixel_normal.clipped(s.normal));
        let normal = config.pixel_normal.quantize(s.normal)?;
        let ndc = lp::pixel_center_ndc(
            q.xy[0] + (lane % 2) as u16,
            q.xy[1] + (lane / 2) as u16,
            scene.width,
            scene.height,
        )
        .map_err(|e| format!("pixel center: {e:?}"))?;
        let pixel = match config.pixel_normal {
            NormalFormat::S12F10 => lp::CompactPixelInput {
                normal: normal.map(|v| (v * 1024.0).round_ties_even() as i16),
                ndc,
            }
            .expanded()
            .map_err(|e| format!("compact pixel: {e:?}"))?,
            NormalFormat::Snorm12 => lp::PixelInput {
                normal: normal.map(|v| ((v * 2047.0).round_ties_even() as i16) << 3),
                ndc,
            },
            NormalFormat::Q14 => lp::PixelInput {
                normal: normal.map(|v| (v * 16384.0).round_ties_even() as i16),
                ndc,
            },
        };
        out.lighting[lane] = lo::evaluate_output(
            pixel,
            scene.material,
            scene.light,
            scene.projection,
            lo::Config {
                rounding: lo::RoundingPolicy {
                    power: lo::Rounding::Floor,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .map_err(|e| format!("lighting: {e:?}"))?;
    }
    Ok(out)
}
fn texture_input(q: &RasterQuad) -> tex::QuadInput {
    tex::QuadInput {
        force_coarsest: q.invalid_helpers != 0,
        quad_id: 0,
        mask: q.mask,
        uv: q.samples.clone().map(|s| s.uv),
        slot: 0,
        material_size_log2: 5,
        filter: tex::Filter::Trilinear,
        lod_bias: 0.0,
    }
}
fn shade(
    q: RasterQuad,
    scene: &Scene,
    config: Config,
    cache: &mut sampler::Cache,
    image: &mut TextureImage,
    stats: &mut Stats,
    prepared: Option<&PreparedFrame>,
) -> Result<ShadedQuad, String> {
    if let Some(prepared) = prepared {
        let calc = prepared
            .quads
            .get(&(q.triangle, q.xy[0], q.xy[1]))
            .ok_or("missing parallel quad")?;
        stats.helper_fallbacks += calc.stats.helper_fallbacks;
        stats.fragments += calc.stats.fragments;
        stats.normal_clips += calc.stats.normal_clips;
        if let Some(texture) = &calc.texture {
            for pixel in &texture.pixels {
                for group in &pixel.groups {
                    let actual = cache.read_group(&group.group, image, &mut Vec::new())?;
                    if actual != group.texels {
                        return Err("parallel texture capture changed".into());
                    }
                }
            }
        }
        return Ok(calc.shaded.clone());
    }
    let mut out = light_quad(&q, scene, config, stats)?;
    if scene.textured {
        for p in sampler::sample(&texture_input(&q), cache, image, tex::Config::counted())?.pixels {
            out.texture[p.lane as usize] = p.rgb;
        }
    }
    Ok(out)
}

struct PreparedQuad {
    raster: RasterQuad,
    shaded: ShadedQuad,
    texture: Option<sampler::Output>,
    stats: Stats,
}
struct PreparedFrame {
    fetched: Vec<MeshVertex>,
    transformed: Vec<(TransformedTriangle, usize)>,
    setups: Vec<to::Report>,
    quads: std::collections::BTreeMap<(u32, u16, u16), PreparedQuad>,
}

pub fn render(scene: &Scene, config: Config) -> Result<Frame, String> {
    render_impl(scene, config, None)
}
fn preflight(scene: &Scene, config: Config) -> Result<vp::Context, String> {
    if scene.vertices.len() > 4096
        || scene.triangles.len() > 4096
        || config.max_steps == 0
        || config.max_steps > 10_000_000
        || scene.width == 0
        || scene.width > 400
        || scene.height == 0
        || scene.height > 240
        || scene.vertices.iter().any(|v| {
            v.position
                .iter()
                .chain(v.normal.iter())
                .chain(v.uv.iter())
                .chain(v.tint.iter())
                .any(|x| !x.is_finite() || x.abs() > 32768.0)
        })
        || scene
            .triangles
            .iter()
            .flatten()
            .any(|&i| i >= scene.vertices.len())
    {
        return Err("oracle scene/budget bounds".into());
    }
    let c = context(scene)?;
    lp::validate(
        lp::PixelInput {
            normal: [0; 3],
            ndc: [0; 2],
        },
        scene.material,
        scene.light,
        scene.projection,
    )
    .map_err(|e| format!("context: {e:?}"))?;
    for n in config.fifo {
        if !(1..=64).contains(&n) {
            return Err("FIFO capacity".into());
        }
    }
    Ok(c)
}
fn render_impl(
    scene: &Scene,
    config: Config,
    prepared: Option<&PreparedFrame>,
) -> Result<Frame, String> {
    let c = preflight(scene, config)?;
    let (mut texture_cache, mut image) = texture()?;
    let clear = rop::Pixel {
        color: rop::quantize([14, 21, 32]),
        depth: 65535,
    };
    let mut framebuffer = functional::Cache::new(scene.width, scene.height, clear)?;
    let mut fetched: Fifo<FetchedTriangle> = Fifo::new(config.fifo[0])?;
    let mut transformed = Fifo::new(config.fifo[1])?;
    let mut setups = Fifo::new(config.fifo[2])?;
    let mut quads = Fifo::new(config.fifo[3])?;
    let mut shaded = Fifo::new(config.fifo[4])?;
    let mut finals = Fifo::new(config.fifo[5])?;
    let mut raster: Option<Raster> = None;
    let mut next = 0;
    let mut stats = Stats::default();
    let mut boundaries = Vec::new();
    let mut lighting = vec![[0; 2]; scene.width as usize * scene.height as usize];
    let mut done = false;
    for step in 0..config.max_steps {
        stats.pump_steps = step + 1;
        // Drain downstream first: finite capacity propagates backpressure and
        // triangle order survives all six independently configured queues.
        if let Some((q, lights)) = finals.pop() {
            let q: fb::Quad = q;
            let lights: [lp::LightingOutput; 4] = lights;
            for (lane, light) in lights.iter().enumerate() {
                if q.header.mask & (1 << lane) != 0 {
                    let x = q.header.x + (lane % 2) as u16;
                    let y = q.header.y as u16 + (lane / 2) as u16;
                    if framebuffer.apply(x, y, q.pixels[lane], scene.rop)? {
                        lighting[y as usize * scene.width as usize + x as usize] =
                            [light.g, light.h];
                    }
                }
            }
        }
        if !finals.full() {
            if let Some(q) = shaded.pop() {
                let q: ShadedQuad = q;
                let mut pixels = [fb::Fragment {
                    rgba: [0; 4],
                    depth: 0,
                }; 4];
                for (lane, pixel) in pixels.iter_mut().enumerate() {
                    if q.header.mask & (1 << lane) != 0 {
                        let l = q.lighting[lane];
                        stats.color_saturations += usize::from((0..3).any(|k| {
                            let p = u32::from(q.tint[lane][k]) * u32::from(q.texture[lane][k]);
                            let t = p + 128;
                            let base = (t + (t >> 8)) >> 8;
                            base * u32::from(l.g)
                                + u32::from(scene.material.specular_color[k]) * u32::from(l.h)
                                > 255 * 256
                        }));
                        let rgb = final_rgb(
                            q.tint[lane],
                            q.texture[lane],
                            l,
                            scene.material.specular_color,
                        )?;
                        *pixel = fb::Fragment {
                            rgba: [rgb[0], rgb[1], rgb[2], 255],
                            depth: q.depth[lane],
                        };
                    }
                }
                finals.push((
                    fb::Quad {
                        header: q.header,
                        pixels,
                    },
                    q.lighting,
                ))?;
            }
        }
        if !shaded.full() {
            if let Some(q) = quads.pop() {
                shaded.push(shade(
                    q,
                    scene,
                    config,
                    &mut texture_cache,
                    &mut image,
                    &mut stats,
                    prepared,
                )?)?;
            }
        }
        if !quads.full() {
            if raster.is_none() {
                if let Some(report) = setups.pop() {
                    raster = Some(Raster::new(report));
                }
            }
            if let Some(r) = &mut raster {
                if let Some(q) = r.next(prepared)? {
                    stats.quads += 1;
                    quads.push(q)?;
                }
                if r.done {
                    raster = None;
                }
            }
        }
        if !setups.full() {
            if let Some(t) = transformed.pop() {
                let t: TransformedTriangle = t;
                let report = if let Some(p) = prepared {
                    p.setups[t.id as usize].clone()
                } else {
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
                    )?
                };
                stats.triangles += 1;
                setups.push(report)?;
            }
        }
        if !transformed.full() {
            if let Some(t) = fetched.pop() {
                let t = if let Some(p) = prepared {
                    let (result, clips) = &p.transformed[t.id as usize];
                    stats.normal_clips += clips;
                    result.clone()
                } else {
                    transform(t, scene, &c, config, &mut stats)?
                };
                boundaries.push(t.clone());
                transformed.push(t)?;
            }
        }
        if !fetched.full() && next < scene.triangles.len() {
            let mut vertices = Vec::with_capacity(3);
            for i in scene.triangles[next] {
                vertices.push(if let Some(p) = prepared {
                    p.fetched[i]
                } else {
                    fetch_vertex(scene.vertices[i], &c, config.fetch)?
                });
            }
            fetched.push(FetchedTriangle {
                id: next as u32,
                vertices: vertices.try_into().map_err(|_| "triangle vertices")?,
            })?;
            next += 1;
        }
        if next == scene.triangles.len()
            && raster.is_none()
            && fetched.empty()
            && transformed.empty()
            && setups.empty()
            && quads.empty()
            && shaded.empty()
            && finals.empty()
        {
            done = true;
            break;
        }
    }
    if !done {
        return Err("oracle maximum pump steps exceeded".into());
    }
    stats.fifo_peak = [
        fetched.peak,
        transformed.peak,
        setups.peak,
        quads.peak,
        shaded.peak,
        finals.peak,
    ];
    let pixels = framebuffer.materialize();
    stats.texture_refills = texture_cache.stats.refills;
    stats.framebuffer_refills = framebuffer.refills;
    stats.framebuffer_writebacks = framebuffer.writebacks;
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    // Presentation LUT only: ROP/framebuffer bytes stay linear RGB565.
    let display: [u8; 256] = std::array::from_fn(|i| {
        let x = i as f64 / 255.0;
        let y = if x <= 0.0031308 {
            12.92 * x
        } else {
            1.055 * x.powf(1.0 / 2.4) - 0.055
        };
        (y * 255.0).round_ties_even() as u8
    });
    for p in &pixels {
        let rgb = rop::expand(p.color);
        rgba.extend([
            display[rgb[0] as usize],
            display[rgb[1] as usize],
            display[rgb[2] as usize],
            255,
        ]);
    }
    Ok(Frame {
        rgba,
        color: pixels.iter().map(|p| p.color).collect(),
        depth: pixels.iter().map(|p| p.depth).collect(),
        lighting,
        stats,
        vertex_boundaries: boundaries,
    })
}
