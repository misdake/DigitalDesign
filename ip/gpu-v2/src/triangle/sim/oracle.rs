//! Unconstrained CPU reference: exact integer coverage/cofactors, f64 clip and
//! configurable interpolation precision. Work is algebra, not an audited ledger.
use super::super::ports::*;
use std::cmp::Ordering;

pub type Point = [f64; 3]; // Raw Q16 XYW units. Z has no consumer.
pub type IntegerField = [i128; 3];
#[derive(Clone, Debug)]
pub struct ClipStage {
    pub plane: usize,
    pub polygon: Vec<Point>,
}
#[derive(Clone, Copy, Debug)]
pub struct Projected {
    pub screen: [f64; 2],
    pub snapped: [i64; 2],
    pub w: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub a: i128,
    pub b: i128,
    pub c: i128,
    pub top_left: bool,
}
impl Edge {
    pub fn unbiased(&self, point: [i64; 2]) -> i128 {
        self.a * i128::from(point[0]) + self.b * i128::from(point[1]) + self.c
    }
    pub fn coverage(&self, point: [i64; 2]) -> i128 {
        self.unbiased(point) - i128::from(!self.top_left)
    }
}
#[derive(Clone, Debug)]
pub struct CoverageTriangle {
    pub polygon_indices: [usize; 3],
    pub vertices: [[i64; 2]; 3],
    pub edges: [Edge; 3],
    pub origin: [i64; 2],
    /// Twice snapped area, with no top-left bias.
    pub determinant: i128,
    pub bbox: [u16; 4], // minx,miny,maxx,maxy, inclusive
    pub flip: bool,
}
impl CoverageTriangle {
    pub fn contains(&self, x: u16, y: u16, bits: u8) -> bool {
        let [minx, miny, maxx, maxy] = self.bbox;
        if x < minx || y < miny || x > maxx || y > maxy {
            return false;
        }
        let step = 1_i64 << bits;
        let p = [
            i64::from(x) * step + step / 2 - self.origin[0],
            i64::from(y) * step + step / 2 - self.origin[1],
        ];
        self.edges.iter().all(|edge| edge.coverage(p) >= 0)
    }
}
#[derive(Clone, Debug, Default)]
pub struct AlgorithmWork {
    pub plane_tests: usize,
    pub intersections: usize,
    pub endpoint_reuses: usize,
    pub projected_vertices: usize,
    pub edge_fans: usize,
    pub edge_products: usize,
    pub degenerate_fans: usize,
    pub culled_fans: usize,
    pub empty_bbox_fans: usize,
    pub source_packages: usize,
}
impl AlgorithmWork {
    /// Dynamic scalar products, excluding reciprocal implementation and constant
    /// viewport scaling. Local anchors are conservatively charged six products.
    pub fn setup_products(&self, interpolation: Interpolation, local: bool) -> usize {
        // t = d0 * reciprocal(d0-d1), followed by two free-coordinate lerps.
        3 * (self.intersections - self.endpoint_reuses)
            + 2 * self.projected_vertices
            + self.edge_products
            + self.source_packages
                * (21
                    + usize::from(local) * 6
                    + if interpolation == Interpolation::Planes {
                        72
                    } else {
                        0
                    })
    }
    pub fn reciprocal_requests(&self) -> usize {
        self.intersections - self.endpoint_reuses + self.projected_vertices
    }
    pub fn pixel_products(interpolation: Interpolation) -> usize {
        match interpolation {
            Interpolation::Basis => 19,
            Interpolation::Planes => 9,
        }
    }
}
#[derive(Clone, Debug)]
pub struct FieldCache {
    pub anchor: [f64; 2],
    pub radius: f64,
    pub scale: f64,
    pub coefficients: Vec<[f64; 3]>,
}
#[derive(Clone, Debug)]
pub struct SourceFields {
    /// Raw homogeneous source units and binary scale. Integer inputs use Q16;
    /// the continuous frontend adapter uses Q28, without changing raster math.
    pub raw_clip: [[i64; 4]; 3],
    pub raw_scale: f64,
    pub positions: [IntegerField; 3],
    /// Viewport constants are factored OUT before cross products, keeping these
    /// inputs at most 33 bits (34 after source differences), rather than 41.
    pub cofactor_positions: [IntegerField; 3],
    pub cofactor_fields: [IntegerField; 3],
    /// Denominator, beta1 numerator, beta2 numerator; source is unsnapped.
    pub fields: [IntegerField; 3],
    pub determinant: i128,
    pub attributes: [[f64; 8]; 3], // uv, rgb, unnormalized normal
    pub constant_channels: [Option<f64>; 8],
    pub cache: FieldCache,
}
#[derive(Clone, Debug)]
pub struct Report {
    pub input: Input,
    pub config: Config,
    pub clip_stages: Vec<ClipStage>,
    pub polygon: Vec<Point>,
    pub projected: Vec<Projected>,
    pub triangles: Vec<CoverageTriangle>,
    pub source: Option<SourceFields>,
    pub work: AlgorithmWork,
    /// Snapping can destroy convexity of a very thin clipped polygon. It is
    /// diagnosed here; bounded rasterization rejects overlapping fans explicitly.
    pub snap_nonconvex: bool,
}
fn distance(p: Point, plane: usize, c: &Config) -> f64 {
    let g = (1_u32 << c.guard_log2) as f64;
    match plane {
        0 => p[2] - f64::from(c.near_raw),
        1 => g * p[2] - p[0],
        2 => g * p[2] + p[0],
        3 => g * p[2] - p[1],
        4 => g * p[2] + p[1],
        _ => unreachable!(),
    }
}
fn compare(a: Point, b: Point) -> Ordering {
    for i in 0..3 {
        let order = a[i].total_cmp(&b[i]);
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}
fn intersect(mut a: Point, mut b: Point, plane: usize, c: &Config) -> Point {
    // Canonical endpoint order makes shared-edge reversal bit-identical.
    if compare(a, b) == Ordering::Greater {
        std::mem::swap(&mut a, &mut b);
    }
    let da = distance(a, plane, c);
    let db = distance(b, plane, c);
    if da == 0.0 {
        return a;
    }
    if db == 0.0 {
        return b;
    }
    let t = da / (da - db);
    let constrained = match plane {
        0 => 2,
        1 | 2 => 0,
        _ => 1,
    };
    let mut p = a;
    for i in 0..3 {
        if i != constrained {
            p[i] = a[i] + (b[i] - a[i]) * t;
        }
    }
    if let Some(fraction) = c.intersection_fraction {
        let q = 2.0_f64.powi(i32::from(fraction) - 16);
        p = p.map(|v| (v * q).round_ties_even() / q);
    }
    let g = (1_u32 << c.guard_log2) as f64;
    match plane {
        0 => p[2] = f64::from(c.near_raw),
        1 => p[0] = g * p[2],
        2 => p[0] = -g * p[2],
        3 => p[1] = g * p[2],
        4 => p[1] = -g * p[2],
        _ => unreachable!(),
    }
    // Quantization must not reopen already processed halfspaces. This is an
    // explicit geometry change in precision experiments, not an epsilon test.
    for prior in 0..plane {
        if distance(p, prior, c) < 0.0 {
            match prior {
                0 => p[2] = f64::from(c.near_raw),
                1 => p[0] = g * p[2],
                2 => p[0] = -g * p[2],
                3 => p[1] = g * p[2],
                _ => unreachable!(),
            }
        }
    }
    p
}
fn dedup(polygon: &mut Vec<Point>) {
    polygon.dedup();
    if polygon.len() > 1 && polygon.first() == polygon.last() {
        polygon.pop();
    }
}
fn cross(a: IntegerField, b: IntegerField) -> IntegerField {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: IntegerField, b: IntegerField) -> i128 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn edge(p: [i64; 2], q: [i64; 2]) -> Edge {
    let a = i128::from(p[1]) - i128::from(q[1]);
    let b = i128::from(q[0]) - i128::from(p[0]);
    edge_coefficients(
        a,
        b,
        i128::from(p[0]) * i128::from(q[1]) - i128::from(q[0]) * i128::from(p[1]),
    )
}
fn edge_coefficients(a: i128, b: i128, c: i128) -> Edge {
    Edge {
        a,
        b,
        c,
        top_left: a > 0 || a == 0 && b > 0,
    }
}
fn dyadic(v: f64) -> (i128, i32) {
    if v == 0.0 {
        return (0, 0);
    }
    let bits = v.to_bits();
    let exponent = ((bits >> 52) & 2047) as i32;
    let mut mantissa = i128::from(bits & ((1_u64 << 52) - 1));
    let mut shift = if exponent == 0 {
        -1074
    } else {
        exponent - 1023 - 52
    };
    if exponent != 0 {
        mantissa += 1_i128 << 52;
    }
    let zeros = mantissa.trailing_zeros();
    mantissa >>= zeros;
    shift += zeros as i32;
    if bits >> 63 != 0 {
        mantissa = -mantissa;
    }
    (mantissa, shift)
}
fn snap(axis: f64, w: f64, extent: u16, bits: u8, subtract: bool) -> Result<i64, String> {
    // Exact RNE of the represented clip point, including noninteger dyadic clip
    // results. Multiplying by an f64 reciprocal must not move a half-grid tie.
    let (a, ae) = dyadic(axis);
    let (b, be) = dyadic(w);
    let common = if a == 0 { be } else { ae.min(be) };
    let align = |value: i128, exponent: i32| -> Result<i128, String> {
        if value == 0 {
            return Ok(0);
        }
        let shift = u32::try_from(exponent - common).map_err(|_| "projection dyadic exponent")?;
        if shift > 126 {
            return Err("projection dyadic alignment range".into());
        }
        value
            .checked_mul(1_i128 << shift)
            .ok_or_else(|| "projection dyadic alignment overflow".into())
    };
    let a = align(a, ae)?;
    let b = align(b, be)?;
    let numerator = if subtract {
        b.checked_sub(a)
    } else {
        b.checked_add(a)
    }
    .and_then(|n| n.checked_mul(i128::from(extent)))
    .and_then(|n| n.checked_mul(1_i128 << bits))
    .ok_or("projection rational numerator overflow")?;
    let denominator = b
        .checked_mul(2)
        .ok_or("projection rational denominator overflow")?;
    if denominator <= 0 {
        return Err("projection requires positive W".into());
    }
    let floor = numerator.div_euclid(denominator);
    let remainder = numerator.rem_euclid(denominator);
    let other = denominator - remainder;
    let increment = remainder > other || remainder == other && floor & 1 != 0;
    i64::try_from(floor + i128::from(increment)).map_err(|_| "projection snap overflow".into())
}
fn source(
    input: &Input,
    c: &Config,
    triangles: &[CoverageTriangle],
    raw_clip: [[i64; 4]; 3],
    raw_scale: f64,
    attributes_override: Option<[[f64; 8]; 3]>,
) -> Result<SourceFields, String> {
    // P_i = 2*rawW_i*(screenX_i,screenY_i,1). This needs no division by original
    // W, so vertices behind or exactly on the eye plane are valid source inputs.
    let cofactor_positions = std::array::from_fn(|i| {
        let v = raw_clip[i];
        let x = i128::from(v[0]);
        let y = i128::from(v[1]);
        let w = i128::from(v[3]);
        [x + w, w - y, 2 * w]
    });
    let delta1 = std::array::from_fn(|i| cofactor_positions[1][i] - cofactor_positions[0][i]);
    let delta2 = std::array::from_fn(|i| cofactor_positions[2][i] - cofactor_positions[0][i]);
    let cofactor_fields = [
        cross(delta1, delta2),
        cross(cofactor_positions[2], cofactor_positions[0]),
        cross(cofactor_positions[0], cofactor_positions[1]),
    ];
    let width = i128::from(c.width);
    let height = i128::from(c.height);
    let positions = cofactor_positions.map(|p| [p[0] * width, p[1] * height, p[2]]);
    let fields = cofactor_fields.map(|f| [f[0] * height, f[1] * width, f[2] * width * height]);
    let determinant = dot(cofactor_positions[0], cofactor_fields[0]) * width * height;
    if determinant == 0 {
        return Err("homogeneous source is singular but snapped coverage survives".into());
    }
    let attributes = attributes_override.unwrap_or_else(|| {
        std::array::from_fn(|i| {
            let v = &input.vertices[i];
            let rgb = v.rgb565;
            [
                f64::from(v.uv[0]) / 4095.0,
                f64::from(v.uv[1]) / 4095.0,
                f64::from(rgb >> 11) / 31.0,
                f64::from((rgb >> 5) & 63) / 63.0,
                f64::from(rgb & 31) / 31.0,
                f64::from(v.normal[0]) / 16384.0,
                f64::from(v.normal[1]) / 16384.0,
                f64::from(v.normal[2]) / 16384.0,
            ]
        })
    });
    let (anchor2, radius) = if c.field_origin == FieldOrigin::Local {
        let minx = triangles.iter().map(|t| t.bbox[0]).min().unwrap();
        let miny = triangles.iter().map(|t| t.bbox[1]).min().unwrap();
        let maxx = triangles.iter().map(|t| t.bbox[2]).max().unwrap();
        let maxy = triangles.iter().map(|t| t.bbox[3]).max().unwrap();
        let span = u32::from((maxx - minx + 1).max(maxy - miny + 1));
        (
            [
                i128::from(minx) + i128::from(maxx) + 1,
                i128::from(miny) + i128::from(maxy) + 1,
            ],
            span.div_ceil(2).next_power_of_two() as f64,
        )
    } else {
        ([0, 0], 1.0)
    };
    let anchor = anchor2.map(|v| v as f64 / 2.0);
    // Exact recentering BEFORE conversion to f64 avoids large-origin cancellation.
    let basis: [[f64; 3]; 3] = fields.map(|f| {
        [
            2.0 * radius * f[0] as f64,
            2.0 * radius * f[1] as f64,
            (f[0] * anchor2[0] + f[1] * anchor2[1] + 2 * f[2]) as f64,
        ]
    });
    let mut coefficients = if c.interpolation == Interpolation::Basis {
        basis.to_vec()
    } else {
        let c0 = std::array::from_fn::<_, 3, _>(|k| basis[0][k] - basis[1][k] - basis[2][k]);
        let mut v = vec![basis[0]];
        for (channel, _) in attributes[0].iter().enumerate() {
            v.push(std::array::from_fn(|k| {
                (0..3)
                    .map(|i| {
                        let field = match i {
                            0 => c0,
                            1 => basis[1],
                            _ => basis[2],
                        };
                        attributes[i][channel] * field[k]
                    })
                    .sum()
            }));
        }
        v
    };
    let largest = coefficients
        .iter()
        .flatten()
        .map(|v| v.abs())
        .fold(0.0_f64, f64::max);
    let scale = 2.0_f64.powf(largest.log2().ceil());
    if let Some(bits) = c.field_bits {
        let q = 2.0_f64.powi(i32::from(bits) - 2);
        for field in &mut coefficients {
            for v in field {
                *v = (*v / scale * q).round_ties_even() * scale / q;
            }
        }
    }
    let constant_channels = std::array::from_fn(|k| {
        (attributes[0][k] == attributes[1][k] && attributes[0][k] == attributes[2][k])
            .then_some(attributes[0][k])
    });
    Ok(SourceFields {
        raw_clip,
        raw_scale,
        positions,
        cofactor_positions,
        cofactor_fields,
        fields,
        determinant,
        attributes,
        constant_channels,
        cache: FieldCache {
            anchor,
            radius,
            scale,
            coefficients,
        },
    })
}
pub fn run(input: &Input, config: Config) -> Result<Report, String> {
    run_inner(
        input,
        config,
        input.vertices.clone().map(|v| v.clip.map(i64::from)),
        65536.0,
        None,
    )
}

/// Continuous frontend boundary. Geometry is quantized once to Q28 by this
/// backend, then uses exactly the same clipping, coverage and field machinery.
/// Attributes remain f64. The bounded clip range keeps integer cofactors in i128.
pub fn run_continuous(
    id: u32,
    clip: [[f64; 4]; 3],
    attributes: [[f64; 8]; 3],
    config: Config,
) -> Result<Report, String> {
    if clip
        .iter()
        .flatten()
        .any(|x| !x.is_finite() || x.abs() > 32.0)
        || attributes
            .iter()
            .flatten()
            .any(|x| !x.is_finite() || x.abs() > 4096.0)
    {
        return Err("continuous source finite/range bounds".into());
    }
    let input = Input {
        id,
        vertices: std::array::from_fn(|i| crate::vertex::ports::Transformed {
            clip: clip[i].map(|v| (v * 65536.0).round_ties_even() as i32),
            normal: [0; 3],
            uv: [0; 2],
            rgb565: 0,
        }),
    };
    run_inner(
        &input,
        config,
        clip.map(|v| v.map(|x| (x * 268435456.0).round_ties_even() as i64)),
        268435456.0,
        Some(attributes),
    )
}

fn run_inner(
    input: &Input,
    config: Config,
    raw_clip: [[i64; 4]; 3],
    raw_scale: f64,
    attributes: Option<[[f64; 8]; 3]>,
) -> Result<Report, String> {
    config.validate()?;
    if input
        .vertices
        .iter()
        .any(|v| v.uv.iter().any(|&u| u > 4095))
    {
        return Err("triangle UNORM12 input width".into());
    }
    let mut polygon: Vec<_> = raw_clip
        .iter()
        .map(|v| {
            [
                v[0] as f64 * 65536.0 / raw_scale,
                v[1] as f64 * 65536.0 / raw_scale,
                v[3] as f64 * 65536.0 / raw_scale,
            ]
        })
        .collect();
    let mut work = AlgorithmWork::default();
    let mut stages = Vec::new();
    for plane in 0..5 {
        work.plane_tests += polygon.len();
        if polygon.is_empty() {
            break;
        }
        let d: Vec<_> = polygon
            .iter()
            .map(|&p| distance(p, plane, &config))
            .collect();
        if d.iter().all(|&d| d < 0.0) {
            polygon.clear();
        } else if d.iter().any(|&d| d < 0.0) {
            let mut output = Vec::new();
            for i in 0..polygon.len() {
                let j = (i + 1) % polygon.len();
                let a = polygon[i];
                let b = polygon[j];
                if d[i] >= 0.0 {
                    output.push(a);
                }
                if (d[i] < 0.0) != (d[j] < 0.0) {
                    if d[i] == 0.0 || d[j] == 0.0 {
                        work.endpoint_reuses += 1;
                    }
                    output.push(intersect(a, b, plane, &config));
                    work.intersections += 1;
                }
            }
            dedup(&mut output);
            polygon = output;
        }
        if polygon.len() > 8 {
            return Err("clip polygon exceeds eight vertices".into());
        }
        stages.push(ClipStage {
            plane,
            polygon: polygon.clone(),
        });
    }
    let mut projected = Vec::new();
    for p in &polygon {
        if p[2] < f64::from(config.near_raw) {
            return Err("clip reopened near plane".into());
        }
        let inv_w = 1.0 / p[2];
        let screen = [
            f64::from(config.width) * 0.5 * (p[0] * inv_w + 1.0),
            f64::from(config.height) * 0.5 * (1.0 - p[1] * inv_w),
        ];
        projected.push(Projected {
            screen,
            snapped: [
                snap(p[0], p[2], config.width, config.subpixel_bits, false)?,
                snap(p[1], p[2], config.height, config.subpixel_bits, true)?,
            ],
            w: p[2] / 65536.0,
        });
    }
    work.projected_vertices = projected.len();
    let mut triangles = Vec::new();
    let mut signs = Vec::new();
    for i in 0..projected.len() {
        let a = projected[i].snapped;
        let b = projected[(i + 1) % projected.len()].snapped;
        let next = projected[(i + 2) % projected.len()].snapped;
        let turn = edge(a, b).unbiased(next);
        if turn != 0 {
            signs.push(turn.signum());
        }
    }
    let snap_nonconvex = signs.iter().any(|&s| s != signs[0]);
    for i in 1..polygon.len().saturating_sub(1) {
        work.edge_fans += 1;
        let mut indices = [0, i, i + 1];
        let mut v = indices.map(|k| projected[k].snapped);
        let origin = if config.coverage_origin == CoverageOrigin::FirstVertex {
            v[0]
        } else {
            [0, 0]
        };
        let local = v.map(|p| [p[0] - origin[0], p[1] - origin[1]]);
        let mut edges = if config.coverage_origin == CoverageOrigin::FirstVertex {
            [
                edge_coefficients(-i128::from(local[1][1]), i128::from(local[1][0]), 0),
                edge(local[1], local[2]),
                edge_coefficients(i128::from(local[2][1]), -i128::from(local[2][0]), 0),
            ]
        } else {
            [
                edge(local[0], local[1]),
                edge(local[1], local[2]),
                edge(local[2], local[0]),
            ]
        };
        work.edge_products += if config.coverage_origin == CoverageOrigin::FirstVertex {
            2
        } else {
            6
        };
        let determinant = edges.iter().map(|e| e.c).sum::<i128>();
        if determinant == 0 {
            work.degenerate_fans += 1;
            continue;
        }
        let flip = determinant < 0;
        if flip && config.cull_back {
            work.culled_fans += 1;
            continue;
        }
        if flip {
            indices.swap(1, 2);
            v.swap(1, 2);
            edges.reverse();
            edges = edges.map(|e| Edge {
                a: -e.a,
                b: -e.b,
                c: -e.c,
                top_left: e.a < 0 || e.a == 0 && e.b < 0,
            });
        }
        let mut bbox = [0; 4];
        let mut empty = false;
        for k in 0..2 {
            let lo = v.iter().map(|p| p[k]).min().unwrap();
            let hi = v.iter().map(|p| p[k]).max().unwrap();
            let step = 1_i64 << config.subpixel_bits;
            let min = -(-(lo - step / 2)).div_euclid(step);
            let max = (hi - step / 2).div_euclid(step);
            let limit = i64::from(if k == 0 { config.width } else { config.height }) - 1;
            if min > limit || max < 0 || min > max {
                empty = true;
                break;
            }
            bbox[k] = min.max(0) as u16;
            bbox[k + 2] = max.min(limit) as u16;
        }
        if empty {
            work.empty_bbox_fans += 1;
            continue;
        }
        triangles.push(CoverageTriangle {
            polygon_indices: indices,
            vertices: v,
            edges,
            origin,
            determinant: determinant.abs(),
            bbox,
            flip,
        });
    }
    let source = if triangles.is_empty() {
        None
    } else {
        work.source_packages = 1;
        Some(source(
            input, &config, &triangles, raw_clip, raw_scale, attributes,
        )?)
    };
    Ok(Report {
        input: input.clone(),
        config,
        clip_stages: stages,
        polygon,
        projected,
        triangles,
        source,
        work,
        snap_nonconvex,
    })
}
fn finish(
    position: [f64; 2],
    beta: [f64; 3],
    a: [f64; 8],
    w: f64,
    c: &Config,
) -> Result<Sample, String> {
    if !w.is_finite() || w <= 0.0 || a.iter().any(|x| !x.is_finite()) {
        return Err("nonpositive/nonfinite interpolated W or attributes".into());
    }
    let q = 2.0_f64.powi(i32::from(c.attribute_fraction));
    let uv = std::array::from_fn(|i| a[i]);
    let rgb = [a[2], a[3], a[4]];
    let normal = [a[5], a[6], a[7]];
    let normal_raw = normal.map(|v| (v * 16384.0).round_ties_even());
    if normal_raw
        .iter()
        .any(|&v| v < f64::from(i32::MIN) || v > f64::from(i32::MAX))
    {
        return Err("normal oracle output overflow".into());
    }
    if uv.iter().any(|v| (v * q).abs() > i64::MAX as f64 - 4096.0) {
        return Err("UV oracle output overflow".into());
    }
    let color: [u16; 3] = std::array::from_fn(|i| {
        (rgb[i].clamp(0.0, 1.0) * [31.0, 63.0, 31.0][i]).round_ties_even() as u16
    });
    let depth =
        ((w * 65536.0 - f64::from(c.near_raw)) / f64::from(c.far_raw - c.near_raw)).clamp(0.0, 1.0);
    Ok(Sample {
        position,
        beta,
        uv,
        rgb,
        normal,
        w,
        quantized: QuantizedSample {
            uv: uv.map(|v| (v * q).round_ties_even() as i64),
            normal: normal_raw.map(|v| v as i32),
            rgb565: (color[0] << 11) | (color[1] << 5) | color[2],
            depth: (depth * 65535.0).round_ties_even() as u16,
        },
    })
}
impl Report {
    /// Evaluate source fields at a helper sample, including uncovered quad lanes.
    /// No clamp of barycentric weights; biased coverage coefficients never enter.
    pub fn evaluate(&self, position: [f64; 2]) -> Result<Sample, String> {
        if position.iter().any(|p| !p.is_finite() || p.abs() > 4096.0) {
            return Err("helper position bound".into());
        }
        let s = self
            .source
            .as_ref()
            .ok_or("no source package for discarded triangle")?;
        let cache = &s.cache;
        let local = [
            (position[0] - cache.anchor[0]) / cache.radius,
            (position[1] - cache.anchor[1]) / cache.radius,
            1.0,
        ];
        let values: Vec<_> = cache
            .coefficients
            .iter()
            .map(|f| (0..3).map(|i| f[i] * local[i]).sum::<f64>())
            .collect();
        self.evaluate_values(position, &values)
    }
    fn evaluate_values(&self, position: [f64; 2], values: &[f64]) -> Result<Sample, String> {
        let s = self.source.as_ref().ok_or("missing source")?;
        let inv = 1.0 / values[0];
        let beta = if self.config.interpolation == Interpolation::Basis {
            [
                1.0 - values[1] * inv - values[2] * inv,
                values[1] * inv,
                values[2] * inv,
            ]
        } else {
            let p = [position[0], position[1], 1.0];
            let denom = (0..3).map(|i| s.fields[0][i] as f64 * p[i]).sum::<f64>();
            let b1 = (0..3).map(|i| s.fields[1][i] as f64 * p[i]).sum::<f64>() / denom;
            let b2 = (0..3).map(|i| s.fields[2][i] as f64 * p[i]).sum::<f64>() / denom;
            [1.0 - b1 - b2, b1, b2]
        };
        let mut attrs = std::array::from_fn(|channel| {
            if self.config.interpolation == Interpolation::Planes {
                values[channel + 1] * inv
            } else {
                s.attributes[0][channel]
                    + (s.attributes[1][channel] - s.attributes[0][channel]) * beta[1]
                    + (s.attributes[2][channel] - s.attributes[0][channel]) * beta[2]
            }
        });
        // Preserve exact uniform channels even when numerator coefficients are
        // quantized independently. No approximate normal/color/UV sharing.
        for (value, constant) in attrs.iter_mut().zip(s.constant_channels) {
            if let Some(constant) = constant {
                *value = constant;
            }
        }
        if self.config.rgb_affine {
            let affine = self.affine_weights(position)?;
            for (k, attribute) in attrs.iter_mut().enumerate().take(5).skip(2) {
                *attribute = (0..3).map(|i| affine[i] * s.attributes[i][k]).sum();
            }
        }
        // Cache coefficients represent twice the original homogeneous fields.
        finish(
            position,
            beta,
            attrs,
            s.determinant as f64 * inv / s.raw_scale,
            &self.config,
        )
    }
    fn affine_weights(&self, position: [f64; 2]) -> Result<[f64; 3], String> {
        let s = self.source.as_ref().ok_or("missing source")?;
        let raw = [position[0], position[1], 1.0];
        let n1 = (0..3).map(|i| s.fields[1][i] as f64 * raw[i]).sum::<f64>();
        let n2 = (0..3).map(|i| s.fields[2][i] as f64 * raw[i]).sum::<f64>();
        let d = (0..3).map(|i| s.fields[0][i] as f64 * raw[i]).sum::<f64>();
        Ok([d - n1 - n2, n1, n2]
            .map(|n| n / s.determinant as f64)
            .iter()
            .enumerate()
            .map(|(i, &t)| t * s.raw_clip[i][3] as f64 * 2.0)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap())
    }
    /// Independent per-sample 3x3 solve, with partial pivoting. It does not read
    /// prepared cofactor fields, cache coefficients or coverage edge values.
    pub fn reference(&self, position: [f64; 2]) -> Result<Sample, String> {
        if position.iter().any(|p| !p.is_finite() || p.abs() > 4096.0) {
            return Err("reference position bound".into());
        }
        let mut m = [[0.0; 4]; 3];
        let s = self.source.as_ref().ok_or("missing source")?;
        for (i, v) in s.raw_clip.iter().enumerate() {
            let x = v[0] as f64;
            let y = v[1] as f64;
            let w = v[3] as f64;
            m[0][i] = f64::from(self.config.width) * (x + w);
            m[1][i] = f64::from(self.config.height) * (w - y);
            m[2][i] = 2.0 * w;
        }
        m[0][3] = 2.0 * position[0];
        m[1][3] = 2.0 * position[1];
        m[2][3] = 2.0;
        for k in 0..3 {
            let pivot = (k..3)
                .max_by(|&a, &b| m[a][k].abs().total_cmp(&m[b][k].abs()))
                .unwrap();
            m.swap(k, pivot);
            let divisor = m[k][k];
            if divisor == 0.0 {
                return Err("reference singular source".into());
            }
            for value in m[k].iter_mut().skip(k) {
                *value /= divisor;
            }
            let pivot_row = m[k];
            for (i, row) in m.iter_mut().enumerate() {
                if i != k {
                    let factor = row[k];
                    for (value, &pivot) in row.iter_mut().zip(&pivot_row).skip(k) {
                        *value -= factor * pivot;
                    }
                }
            }
        }
        let t = std::array::from_fn::<_, 3, _>(|i| m[i][3]);
        let sum = t.iter().sum::<f64>();
        let beta = t.map(|v| v / sum);
        let attrs = std::array::from_fn(|channel| {
            (0..3)
                .map(|i| {
                    let weight = if self.config.rgb_affine && (2..5).contains(&channel) {
                        t[i] * s.raw_clip[i][3] as f64
                    } else {
                        beta[i]
                    };
                    let attribute = s.attributes[i][channel];
                    weight * attribute
                })
                .sum()
        });
        finish(
            position,
            beta,
            attrs,
            1.0 / (sum * s.raw_scale),
            &self.config,
        )
    }
    /// Bounded scanline reference, with exact coverage increments. Querying each
    /// sample with independent contains()/orient tests can validate this path.
    pub fn rasterize(&self) -> Result<Vec<([u16; 2], usize, Sample)>, String> {
        let mut attempts = 0;
        let mut result = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let step = 1_i64 << self.config.subpixel_bits;
        for (fan, t) in self.triangles.iter().enumerate() {
            let [minx, miny, maxx, maxy] = t.bbox;
            let p = [
                i64::from(minx) * step + step / 2 - t.origin[0],
                i64::from(miny) * step + step / 2 - t.origin[1],
            ];
            let mut edge_row = t.edges.map(|e| e.coverage(p));
            let cache = &self.source.as_ref().ok_or("missing source")?.cache;
            let local = [
                (f64::from(minx) + 0.5 - cache.anchor[0]) / cache.radius,
                (f64::from(miny) + 0.5 - cache.anchor[1]) / cache.radius,
                1.0,
            ];
            let mut field_row: Vec<_> = cache
                .coefficients
                .iter()
                .map(|f| (0..3).map(|i| f[i] * local[i]).sum::<f64>())
                .collect();
            for y in miny..=maxy {
                let mut e = edge_row;
                let mut fields = field_row.clone();
                for x in minx..=maxx {
                    attempts += 1;
                    if attempts > self.config.max_samples {
                        return Err("triangle maximum sample count exceeded".into());
                    }
                    if e.iter().all(|&v| v >= 0) {
                        if !seen.insert([x, y]) {
                            return Err("snapped clip fans overlap".into());
                        }
                        result.push((
                            [x, y],
                            fan,
                            self.evaluate_values(
                                [f64::from(x) + 0.5, f64::from(y) + 0.5],
                                &fields,
                            )?,
                        ));
                    }
                    for (value, edge) in e.iter_mut().zip(t.edges) {
                        *value += edge.a * i128::from(step);
                    }
                    for (value, f) in fields.iter_mut().zip(&cache.coefficients) {
                        *value += f[0] / cache.radius;
                    }
                }
                for (value, edge) in edge_row.iter_mut().zip(t.edges) {
                    *value += edge.b * i128::from(step);
                }
                for (value, f) in field_row.iter_mut().zip(&cache.coefficients) {
                    *value += f[1] / cache.radius;
                }
            }
        }
        Ok(result)
    }
    /// UV is unwrapped before derivatives. Helper lanes are evaluated regardless
    /// of coverage, as required by quad LOD; invalid W is reported explicitly.
    pub fn quad_lod(&self, x: u16, y: u16, texture: u16) -> Result<f64, String> {
        if self.config.max_samples < 4 {
            return Err("quad helper sample budget".into());
        }
        if texture == 0 || texture > 1024 || !texture.is_power_of_two() {
            return Err("texture extent bound".into());
        }
        let p: [[f64; 2]; 4] = std::array::from_fn(|i| {
            [
                f64::from(x) + 0.5 + (i % 2) as f64,
                f64::from(y) + 0.5 + (i / 2) as f64,
            ]
        });
        let uv = p.map(|p| self.evaluate(p).map(|s| s.quantized.uv));
        let uv = uv.into_iter().collect::<Result<Vec<_>, String>>()?;
        let mut rho = 0.0_f64;
        for (a, b) in [(0, 1), (2, 3), (0, 2), (1, 3)] {
            for (&u, &v) in uv[a].iter().zip(&uv[b]) {
                rho = rho.max((u - v).abs() as f64);
            }
        }
        Ok(
            (rho * f64::from(texture) / 2.0_f64.powi(i32::from(self.config.attribute_fraction)))
                .log2()
                .max(0.0),
        )
    }
    /// Exact stage golden at an integer pixel center. Raster increments are
    /// (+2*A,+2*B) for this doubled-coordinate source representation.
    pub fn integer_pixel_fields(&self, x: u16, y: u16) -> Result<[i128; 3], String> {
        if x >= self.config.width || y >= self.config.height {
            return Err("pixel bounds".into());
        }
        let s = self.source.as_ref().ok_or("missing source")?;
        let p = [2 * i128::from(x) + 1, 2 * i128::from(y) + 1, 2];
        Ok(s.fields.map(|f| dot(f, p)))
    }
}
