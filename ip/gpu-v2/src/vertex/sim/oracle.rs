//! High precision reference plus independently quantized integer stage goldens.
use super::super::ports::*;
#[derive(Clone, Debug)]
pub struct Config {
    pub clip_fraction: u32,
    pub normal_fraction: u32,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            clip_fraction: 16,
            normal_fraction: 10,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Report {
    pub position: [i32; 3],
    pub input_normal: [i16; 3],
    pub clip_sum: [i128; 4],
    pub normal_sum: [i128; 3],
    pub high_clip: [f64; 4],
    pub high_normal: [f64; 3],
    pub output: Transformed,
}
pub fn rne(x: i128, shift: u32) -> i128 {
    if shift == 0 {
        return x;
    }
    let floor = x >> shift;
    let tail = x - (floor << shift);
    let half = 1_i128 << (shift - 1);
    floor + i128::from(tail > half || (tail == half && floor & 1 != 0))
}
pub fn run(context: &Context, vertex: PackedVertex, config: &Config) -> Result<Report, String> {
    context.validate()?;
    if config.clip_fraction > 16 || config.normal_fraction > 10 {
        return Err("oracle precision bounds".into());
    }
    let (position, normal, uv, rgb565) = unpack(context, vertex)?;
    transform_quantized(context, position, normal, uv, rgb565, config)
}

pub type Unpacked = ([i32; 3], [i16; 3], [u16; 2], u16);
/// Canonical v6 fetch; binary S8F7 normals, UNORM12 UV, RGB565 tint.
pub fn unpack(context: &Context, vertex: PackedVertex) -> Result<Unpacked, String> {
    context.validate()?;
    let bits = vertex.bits();
    if bits & 3 != 0 {
        return Err("record is not a v6 vertex".into());
    }
    let mut position = [0; 3];
    let mut normal = [0; 3];
    for i in 0..3 {
        let p = i128::from(context.base[i])
            + ((((bits >> (2 + 10 * i)) & 1023) as i128) << context.grid_shift);
        position[i] = i32::try_from(p).map_err(|_| "unpacked position overflow")?;
        normal[i] = ((bits >> (32 + 8 * i)) as u8 as i8 as i16) << 7;
    }
    Ok((
        position,
        normal,
        [((bits >> 56) & 4095) as u16, ((bits >> 68) & 4095) as u16],
        (bits >> 80) as u16,
    ))
}

/// The same integer transform used by compact fetch, also accepting ideal-fetch
/// inputs quantized at the S16.16 / Q14 arithmetic ingress.
pub fn transform_quantized(
    context: &Context,
    position: [i32; 3],
    normal: [i16; 3],
    uv: [u16; 2],
    rgb565: u16,
    config: &Config,
) -> Result<Report, String> {
    context.validate()?;
    if config.clip_fraction > 16 || config.normal_fraction > 10 || uv.iter().any(|&v| v > 4095) {
        return Err("vertex precision/UV bounds".into());
    }
    let clip_sum = std::array::from_fn(|row| {
        (0..3)
            .map(|k| i128::from(context.mvp[row][k]) * i128::from(position[k]))
            .sum::<i128>()
            + (i128::from(context.mvp[row][3]) << 16)
    });
    let normal_sum = std::array::from_fn(|row| {
        (0..3)
            .map(|k| i128::from(context.normal_matrix[row][k]) * i128::from(normal[k]))
            .sum::<i128>()
    });
    let mut clip = [0; 4];
    let mut out_normal = [0; 3];
    for i in 0..4 {
        clip[i] = i32::try_from(
            rne(clip_sum[i], 32 - config.clip_fraction) << (16 - config.clip_fraction),
        )
        .map_err(|_| "clip overflow")?;
    }
    for i in 0..3 {
        out_normal[i] = i16::try_from(
            rne(normal_sum[i], 28 - config.normal_fraction) << (10 - config.normal_fraction),
        )
        .map_err(|_| "normal overflow")?;
        if !(-2048..=2047).contains(&out_normal[i]) {
            return Err("S(12,10) normal overflow".into());
        }
    }
    Ok(Report {
        position,
        input_normal: normal,
        clip_sum,
        normal_sum,
        high_clip: clip_sum.map(|x| x as f64 / 4294967296.0),
        high_normal: normal_sum.map(|x| x as f64 / 268435456.0),
        output: Transformed {
            clip,
            normal: out_normal,
            uv,
            rgb565,
        },
    })
}

/// Unquantized vertex oracle boundary. Neither matrix nor input is silently
/// converted to the hardware format before the caller-selected output boundary.
pub fn transform_continuous(
    mvp: [[f64; 4]; 4],
    normal_matrix: [[f64; 3]; 3],
    position: [f64; 3],
    normal: [f64; 3],
) -> Result<([f64; 4], [f64; 3]), String> {
    if mvp
        .iter()
        .flatten()
        .chain(normal_matrix.iter().flatten())
        .chain(position.iter())
        .chain(normal.iter())
        .any(|v| !v.is_finite() || v.abs() > 32768.0)
    {
        return Err("continuous vertex finite/range bounds".into());
    }
    let clip = mvp.map(|row| (0..3).map(|k| row[k] * position[k]).sum::<f64>() + row[3]);
    let normal = normal_matrix.map(|row| (0..3).map(|k| row[k] * normal[k]).sum());
    Ok((clip, normal))
}
