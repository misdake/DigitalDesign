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
            normal_fraction: 14,
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
    if config.clip_fraction > 16 || config.normal_fraction > 14 {
        return Err("oracle precision bounds".into());
    }
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
            rne(normal_sum[i], 28 - config.normal_fraction) << (14 - config.normal_fraction),
        )
        .map_err(|_| "normal overflow")?;
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
            uv: [((bits >> 56) & 4095) as u16, ((bits >> 68) & 4095) as u16],
            rgb565: (bits >> 80) as u16,
        },
    })
}
