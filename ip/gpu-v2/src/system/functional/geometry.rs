//! Selected V96/F16 -> N27 -> h30 arithmetic. No clipping or floating vertices.
use super::{shading, Report};
use rayon::prelude::*;
use std::sync::OnceLock;

#[cfg(feature = "functional-batch")]
#[path = "geometry/batch.rs"]
pub(super) mod batch;

pub(super) fn signed(value: i128, bits: u32) -> i128 {
    let shift = 128 - bits;
    (value << shift) >> shift
}
pub(super) fn rne(value: i128, divisor: i128) -> i128 {
    let magnitude = value.abs();
    let (q, r) = (magnitude / divisor, magnitude % divisor);
    let rounded = q + i128::from(r * 2 > divisor || r * 2 == divisor && q & 1 != 0);
    if value < 0 {
        -rounded
    } else {
        rounded
    }
}
fn bits(value: i128) -> i32 {
    128 - value.unsigned_abs().leading_zeros() as i32
}
fn floor_scale(value: i128, right: i32) -> i128 {
    if right >= 0 {
        value >> right
    } else {
        value << -right
    }
}
fn trunc_scale(value: i128, right: i32) -> i128 {
    if value < 0 {
        -floor_scale(-value, right)
    } else {
        floor_scale(value, right)
    }
}
fn cross(a: [i128; 3], b: [i128; 3]) -> [i128; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn word(data: &[u16], index: usize) -> u32 {
    u32::from(data[index * 2]) | (u32::from(data[index * 2 + 1]) << 16)
}
fn qword(data: &[u16], index: usize) -> u64 {
    u64::from(word(data, index * 2)) | (u64::from(word(data, index * 2 + 1)) << 32)
}

#[derive(Clone)]
pub(super) struct Meshlet {
    vertices: Vec<[u32; 3]>,
    triangles: Vec<[usize; 3]>,
    origin: [i128; 3],
    shift: u32,
    normal18: bool,
    pub uniform: usize,
    pub(super) rgb: bool,
    cull: bool,
}
impl Meshlet {
    pub fn decode(header: &[u16], payload: &[u16]) -> Result<Self, String> {
        let h: [u32; 8] = std::array::from_fn(|i| word(header, i));
        let vertices = (h[0] & 127) as usize;
        let triangles = ((h[0] >> 8) & 127) as usize;
        let uniform = ((h[0] >> 16) & 63) as usize;
        let offset = ((h[0] >> 24) & 127) as usize;
        if h[0] & !0xff3f7f7f != 0
            || h[1] != 0
            || h[2] & !0xfff0 != 0
            || h[3] != 0
            || h[7] & !0x11f != 0
            || vertices > 64
            || triangles > 96
            || !(20..24).contains(&uniform)
            || (triangles != 0 && vertices == 0)
            || offset < (vertices * 3).div_ceil(2)
            || offset + triangles.div_ceil(3) > payload.len() / 4
        {
            return Err("native meshlet metadata/count/extent".into());
        }
        let rgb = h[2] & 16 != 0;
        let mut source = Vec::with_capacity(vertices);
        for i in 0..vertices {
            let v = std::array::from_fn(|j| word(payload, i * 3 + j));
            if v[0] >> 30 != 0 || v[1] >> 30 != 0 || (rgb && v[2] >> 24 != 0) {
                return Err("V96 reserved bits".into());
            }
            source.push(v);
        }
        let mut indices = Vec::with_capacity(triangles);
        for group in 0..triangles.div_ceil(3) {
            let raw = qword(payload, offset + group);
            let live = (triangles - group * 3).min(3);
            if raw >> (live * 18) != 0 {
                return Err("native triangle padding".into());
            }
            for lane in 0..live {
                let triangle =
                    std::array::from_fn(|j| ((raw >> (lane * 18 + j * 6)) & 63) as usize);
                if triangle.iter().any(|&v| v >= vertices) {
                    return Err("native triangle reference".into());
                }
                indices.push(triangle);
            }
        }
        Ok(Self {
            vertices: source,
            triangles: indices,
            origin: [h[4], h[5], h[6]].map(|v| i128::from(v as i32)),
            shift: h[7] & 31,
            normal18: h[7] & 256 != 0,
            uniform: uniform - 20,
            rgb,
            cull: h[0] >> 31 != 0,
        })
    }
}

#[derive(Clone, Copy)]
struct Vertex {
    position: [i128; 3],
    normal: [i128; 3],
    attribute: u32,
}
impl Meshlet {
    fn transform(&self, uniform: &[u32; 32]) -> Result<Vec<Vertex>, String> {
        if uniform[21] != 65536 || uniform[23..].iter().any(|&v| v != 0) {
            return Err("Instance128 constant/reserved words".into());
        }
        self.vertices
            .iter()
            .map(|v| {
                let p: [i128; 3] = std::array::from_fn(|k| {
                    signed(
                        self.origin[k] + (i128::from((v[0] >> (10 * k)) & 1023) << self.shift),
                        32,
                    )
                });
                let n: [i128; 3] =
                    std::array::from_fn(|k| signed(i128::from(v[1] >> (10 * k)), 10) * 128);
                let position = std::array::from_fn(|j| {
                    signed(
                        (p.iter()
                            .enumerate()
                            .map(|(k, p)| p * i128::from(uniform[4 * j + k] as i32))
                            .sum::<i128>()
                            + 65536 * i128::from(uniform[4 * j + 3] as i32))
                            >> 16,
                        32,
                    )
                });
                if position
                    .iter()
                    .any(|&p| !(-(1 << 25)..(1 << 25)).contains(&p))
                {
                    return Err("transformed position outside signed26".into());
                }
                let normal = std::array::from_fn(|j| {
                    let raw = signed(
                        n.iter()
                            .enumerate()
                            .map(|(k, n)| n * i128::from(uniform[12 + 3 * j + k] as i32))
                            .sum::<i128>()
                            >> 16,
                        32,
                    );
                    let q = if self.normal18 {
                        signed(raw, 18)
                    } else {
                        4 * signed(raw >> 2, 16)
                    };
                    rne(q, 64).clamp(-2048, 2047)
                });
                Ok(Vertex {
                    position,
                    normal,
                    attribute: v[2],
                })
            })
            .collect()
    }
}

struct Triangle {
    vertices: [Vertex; 3],
    edges: [[i128; 3]; 3],
    exponents: [i32; 3],
    common_exponent: i32,
    depth_mantissa: i128,
    depth_shift: i32,
}
impl Triangle {
    fn new(vertices: [Vertex; 3], cull: bool) -> Result<Option<Self>, String> {
        if vertices.iter().all(|v| v.position[2] <= 0) {
            return Ok(None);
        }
        let p = vertices.map(|v| v.position);
        let mut normals = [cross(p[1], p[2]), cross(p[2], p[0]), cross(p[0], p[1])];
        let mut exponents = [0; 3];
        for (normal, exponent) in normals.iter_mut().zip(&mut exponents) {
            *exponent = normal
                .iter()
                .map(|&v| bits(if v < 0 { !v } else { v }) - 26)
                .max()
                .unwrap()
                .max(0);
            *normal = normal.map(|v| trunc_scale(v, *exponent));
        }
        let determinant = p[0]
            .iter()
            .zip(normals[0])
            .map(|(a, b)| a * b)
            .sum::<i128>()
            << exponents[0];
        if determinant == 0 || (determinant < 0 && cull) {
            return Ok(None);
        }
        let mut edges = [[0; 3]; 3];
        for i in 0..3 {
            let [nx, ny, nz] = normals[i];
            let coefficients = [nx, ny, -399 * nx - 239 * ny + 512 * nz];
            let shift = (bits(nx) - 17)
                .max(bits(ny) - 17)
                .max(bits(coefficients[2]) - 27)
                .max(0);
            if shift > 11 {
                return Err("triangle coefficient exceeds N27/C38 transport".into());
            }
            edges[i] = coefficients.map(|v| determinant.signum() * trunc_scale(v, shift));
            exponents[i] += shift;
        }
        let common_exponent = *exponents.iter().max().unwrap();
        let determinant = determinant.abs();
        let depth_exponent = bits(determinant) - 1;
        Ok(Some(Self {
            vertices,
            edges,
            exponents,
            common_exponent,
            depth_mantissa: floor_scale(determinant, depth_exponent - 30),
            depth_shift: common_exponent + 38 - depth_exponent,
        }))
    }

    fn span(&self, y: usize) -> Option<(usize, usize)> {
        let (mut lo, mut hi) = (0i128, 399i128);
        for &[nx, ny, c] in &self.edges {
            let a = 2 * nx;
            let tau = i128::from(!(a > 0 || a == 0 && ny > 0));
            let need = tau - c - 2 * ny * y as i128;
            if a > 0 {
                lo = lo.max(-(-need).div_euclid(a));
            } else if a < 0 {
                hi = hi.min((-need).div_euclid(-a));
            } else if need > 0 {
                return None;
            }
        }
        if lo > hi {
            None
        } else {
            Some((lo as usize, hi as usize))
        }
    }

    fn rows(&self) -> Option<std::ops::Range<usize>> {
        // Eliminate x between each lower/upper half-plane constraint. Every
        // covered integer pixel satisfies these continuous bounds; fractional
        // intersections may retain extra rows but can never remove coverage.
        let mut lower = [(0, 0, 1); 4];
        let mut upper = [(399, 0, 1); 4];
        let (mut lower_count, mut upper_count) = (1, 1);
        let (mut lo, mut hi) = (0i128, 239i128);
        let constrain = |a: i128, b: i128, lo: &mut i128, hi: &mut i128| {
            if a > 0 {
                *hi = (*hi).min(b.div_euclid(a));
            } else if a < 0 {
                *lo = (*lo).max(-b.div_euclid(-a));
            } else if b < 0 {
                *hi = -1;
            }
        };
        for &[nx, ny, c] in &self.edges {
            let a = 2 * nx;
            let tau = i128::from(!(a > 0 || a == 0 && ny > 0));
            if a > 0 {
                lower[lower_count] = (tau - c, -2 * ny, a);
                lower_count += 1;
            } else if a < 0 {
                upper[upper_count] = (c - tau, 2 * ny, -a);
                upper_count += 1;
            } else {
                constrain(-2 * ny, c - tau, &mut lo, &mut hi);
            }
        }
        for &(a, b, d) in &lower[..lower_count] {
            for &(c, e, f) in &upper[..upper_count] {
                constrain(b * f - e * d, c * d - a * f, &mut lo, &mut hi);
            }
        }
        (lo <= hi).then_some(lo as usize / 2..hi as usize / 2 + 1)
    }

    fn fragment(&self, x: usize, y: usize, far: u32) -> Result<Fragment, String> {
        let common = self.common_exponent;
        let mut h = [0; 3];
        for (i, value) in h.iter_mut().enumerate() {
            let [nx, ny, c] = self.edges[i];
            let raw = 2 * nx * x as i128 + 2 * ny * y as i128 + c;
            if !(0..(1 << 29)).contains(&raw) {
                return Err("covered h30 outside admitted domain".into());
            }
            *value = raw >> (common - self.exponents[i]);
        }
        let d: i128 = h.iter().sum();
        let e = bits(d.max(1)) - 1;
        let mantissa = floor_scale(d.max(1), e - 17);
        let table = constants()[((mantissa >> 9) & 255) as usize];
        let r = i128::from((table >> 9) & 0x3ffff)
            - rne((mantissa & 511) * i128::from(table & 511), 512);
        let w0 = if d == 0 {
            0
        } else {
            ((r * floor_scale(h[0], e - 16)) >> 17).min(65536)
        };
        let w1 = if d == 0 {
            0
        } else {
            ((r * floor_scale(h[1], e - 16)) >> 17).min(65536 - w0)
        };
        let interpolate = |values: [i128; 3]| {
            (values[2] * 65536
                + (values[0] - values[2]) * w0
                + (values[1] - values[2]) * w1
                + 32768)
                >> 16
        };
        let uv = std::array::from_fn(|c| {
            interpolate(
                self.vertices
                    .map(|v| i128::from((v.attribute >> (c * 16)) & 65535)),
            )
        });
        let rgb = std::array::from_fn(|c| {
            interpolate(
                self.vertices
                    .map(|v| i128::from((v.attribute >> (c * 8)) & 255)),
            ) as u8
        });
        let normal =
            std::array::from_fn(|c| interpolate(self.vertices.map(|v| v.normal[c])) as i16);
        let depth_full = floor_scale(r * self.depth_mantissa, e + self.depth_shift);
        Ok(Fragment {
            uv,
            rgb,
            normal,
            depth: ((depth_full >> 8) & 65535) as u16,
            valid: d != 0 && depth_full < (1 << 24) && depth_full >> 8 <= i128::from(far >> 8),
            denominator: d,
        })
    }

    fn lod(&self, anchor: &Fragment, state: shading::RenderState) -> u8 {
        if !state.texture.full_mip {
            return 0;
        }
        let common = self.common_exponent;
        let mut maximum = None;
        for channel in 0..2 {
            for axis in 0..2 {
                let derivative: i128 = (0..3)
                    .map(|i| {
                        let n = trunc_scale(self.edges[i][axis], common - self.exponents[i]);
                        (i128::from((self.vertices[i].attribute >> (channel * 16)) & 65535)
                            - anchor.uv[channel])
                            * n
                    })
                    .sum();
                if derivative != 0 {
                    let log = log_q8(derivative.abs()) + i32::from(state.texture.size_log2) * 256;
                    maximum = Some(maximum.map_or(log, |old: i32| old.max(log)));
                }
            }
        }
        let raw = maximum.map_or(0, |log| {
            log - log_q8(anchor.denominator.max(1)) - 3712 + 16 * i32::from(state.lod_bias)
        });
        (raw.clamp(0, i32::from(state.texture.size_log2) * 256) >> 5) as u8
    }
}

fn constants() -> &'static [u64; 256] {
    static TABLE: OnceLock<[u64; 256]> = OnceLock::new();
    TABLE.get_or_init(crate::frontend::interpolation_constants)
}
fn log_q8(value: i128) -> i32 {
    let e = bits(value) - 1;
    let mantissa = floor_scale(value, e - 6);
    e * 256 + ((constants()[((mantissa - 64) * 4) as usize] >> 27) & 255) as i32
}

pub(super) struct Fragment {
    pub uv: [i128; 2],
    pub rgb: [u8; 3],
    pub normal: [i16; 3],
    pub depth: u16,
    valid: bool,
    denominator: i128,
}

#[derive(Clone, Copy)]
struct Quad {
    x: usize,
    y: usize,
    mask: u8,
}
impl Quad {
    fn new(x: usize, y: usize, spans: [Option<(usize, usize)>; 2]) -> Self {
        let mut mask = 0;
        for lane in 0..4 {
            if spans[lane >> 1].is_some_and(|(lo, hi)| (lo..=hi).contains(&(x + (lane & 1)))) {
                mask |= 1 << lane;
            }
        }
        Self { x, y, mask }
    }
    fn xy(self, lane: usize) -> [usize; 2] {
        [self.x + (lane & 1), self.y + (lane >> 1)]
    }
    fn fragments(self, triangle: &Triangle, far: u32) -> Result<[Option<Fragment>; 4], String> {
        let mut result = std::array::from_fn(|_| None);
        for (lane, slot) in result.iter_mut().enumerate() {
            if self.mask & (1 << lane) != 0 {
                let [x, y] = self.xy(lane);
                *slot = Some(triangle.fragment(x, y, far)?);
            }
        }
        Ok(result)
    }
}
type Spans = [Option<(usize, usize)>; 2];
type PreparedQuad = Result<[Option<shading::PreparedFragment>; 4], String>;

fn span_range(spans: Spans) -> Option<std::ops::RangeInclusive<usize>> {
    let lo = spans.iter().flatten().map(|span| span.0).min()?;
    let hi = spans.iter().flatten().map(|span| span.1).max().unwrap();
    Some((lo & !1)..=hi)
}
fn prepare_quad(
    memory: &super::Memory<'_>,
    triangle: &Triangle,
    quad: Quad,
    state: shading::DrawState,
) -> PreparedQuad {
    let fragments = quad.fragments(triangle, state.global.far)?;
    let lod = fragments.iter().flatten().next().map_or(0, |first| {
        if state.rgb {
            0
        } else {
            triangle.lod(first, state.render)
        }
    });
    let mut prepared = std::array::from_fn(|_| None);
    for (lane, fragment) in fragments.into_iter().enumerate() {
        if let Some(fragment) = fragment.filter(|f| f.valid) {
            prepared[lane] = Some(shading::prepare(
                memory,
                quad.xy(lane),
                fragment,
                &state,
                lod,
            ));
        }
    }
    Ok(prepared)
}

pub(super) fn draw(
    memory: &mut super::Memory<'_>,
    mesh: &Meshlet,
    uniform: &[u32; 32],
    state: shading::DrawState,
    report: &mut Report,
    maximum: usize,
) -> Result<(), String> {
    report.charge(mesh.vertices.len(), maximum)?;
    let vertices = mesh.transform(uniform)?;
    let independent = mesh.rgb || !state.render.texture.overlaps_target(state.target);
    for indices in &mesh.triangles {
        report.charge(1, maximum)?;
        let Some(triangle) = Triangle::new(indices.map(|i| vertices[i]), mesh.cull)? else {
            continue;
        };
        let Some(active_rows) = triangle.rows() else {
            report.charge(240, maximum)?;
            continue;
        };
        report.charge(active_rows.start * 2, maximum)?;
        let rows: [Spans; 120] = std::array::from_fn(|row| {
            if active_rows.contains(&row) {
                [triangle.span(row * 2), triangle.span(row * 2 + 1)]
            } else {
                [None, None]
            }
        });
        let mut prepared = independent
            .then(|| {
                let count: usize = rows
                    .iter()
                    .filter_map(|&spans| span_range(spans))
                    .map(|range| (*range.end() - *range.start()) / 2 + 1)
                    .sum();
                // Four fragments and at most eight nonzero texture taps per quad.
                // Small remaining budgets stay serial to retain the failure prefix.
                if count < 256
                    || count * 40 + 240 > maximum.saturating_sub(report.work)
                    || rayon::current_num_threads() == 1
                {
                    return None;
                }
                let mut jobs = Vec::with_capacity(count);
                for (row, &spans) in rows.iter().enumerate() {
                    if let Some(range) = span_range(spans) {
                        jobs.extend(range.step_by(2).map(|x| Quad::new(x, row * 2, spans)));
                    }
                }
                let shared = &*memory;
                let result: Vec<_> = jobs
                    .par_iter()
                    .with_min_len(64)
                    .map(|&quad| prepare_quad(shared, &triangle, quad, state))
                    .collect();
                Some(result.into_iter())
            })
            .flatten();
        for y in (active_rows.start * 2..active_rows.end * 2).step_by(2) {
            report.charge(2, maximum)?;
            let spans = rows[y / 2];
            let Some(range) = span_range(spans) else {
                continue;
            };
            for x in range.step_by(2) {
                report.charge(4, maximum)?;
                let quad = Quad::new(x, y, spans);
                if let Some(prepared) = &mut prepared {
                    for (lane, fragment) in prepared.next().unwrap()?.into_iter().enumerate() {
                        if let Some(fragment) = fragment {
                            shading::commit(
                                memory,
                                quad.xy(lane),
                                fragment,
                                &state,
                                report,
                                maximum,
                            )?;
                        }
                    }
                    continue;
                }
                let fragments = quad.fragments(&triangle, state.global.far)?;
                let Some(first) = fragments.iter().flatten().next() else {
                    continue;
                };
                let lod = if mesh.rgb {
                    0
                } else {
                    triangle.lod(first, state.render)
                };
                for (lane, fragment) in fragments.into_iter().enumerate() {
                    if let Some(fragment) = fragment.filter(|f| f.valid) {
                        shading::fragment(
                            memory,
                            quad.xy(lane),
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
        report.charge((120 - active_rows.end) * 2, maximum)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "geometry_tests.rs"]
mod tests;
