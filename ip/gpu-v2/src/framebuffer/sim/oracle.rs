use super::super::ports::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pixel {
    pub color: u16,
    pub depth: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultPixel {
    pub pixel: Pixel,
    pub color_written: bool,
    pub depth_written: bool,
}
pub fn quantize(rgb: [u8; 3]) -> u16 {
    let q = |v: u8, m: u32| ((u32::from(v) * m + 127) / 255) as u16;
    (q(rgb[0], 31) << 11) | (q(rgb[1], 63) << 5) | q(rgb[2], 31)
}
pub fn expand(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u8;
    let g = ((c >> 5) & 63) as u8;
    let b = (c & 31) as u8;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}
pub fn pixel(old: Pixel, source: Fragment, covered: bool, context: Context) -> ResultPixel {
    let pass = covered
        && match context.depth {
            DepthFunc::Never => false,
            DepthFunc::Less => source.depth < old.depth,
            DepthFunc::Equal => source.depth == old.depth,
            DepthFunc::LessEqual => source.depth <= old.depth,
            DepthFunc::Greater => source.depth > old.depth,
            DepthFunc::NotEqual => source.depth != old.depth,
            DepthFunc::GreaterEqual => source.depth >= old.depth,
            DepthFunc::Always => true,
        };
    if !pass {
        return ResultPixel {
            pixel: old,
            color_written: false,
            depth_written: false,
        };
    }
    let dest = expand(old.color);
    let rgb = std::array::from_fn(|i| match context.blend {
        Blend::Replace => source.rgba[i],
        Blend::SrcOver => {
            ((u32::from(source.rgba[3]) * u32::from(source.rgba[i])
                + (255 - u32::from(source.rgba[3])) * u32::from(dest[i])
                + 127)
                / 255) as u8
        }
    });
    ResultPixel {
        pixel: Pixel {
            color: quantize(rgb),
            depth: if context.depth_write {
                source.depth
            } else {
                old.depth
            },
        },
        color_written: true,
        depth_written: context.depth_write,
    }
}
