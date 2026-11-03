//! Current native state, direct-memory Sampling, selected Lighting and Q6 ROP.
use super::{bounds, geometry::Fragment, Report, PLANE_WORDS};
use crate::lighting::ports as light;

fn words<const N: usize>(bytes: &[u8]) -> [u32; N] {
    std::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
}
#[derive(Clone, Copy)]
pub(super) struct Global {
    pub far: u32,
    pub depth_shift: u8,
    light: light::Light,
    projection: light::Projection,
    mode: u8,
    code: u8,
}
impl Global {
    pub fn mapped_far(self) -> u32 {
        self.far << self.depth_shift
    }
    pub fn decode(bytes: [u8; 64]) -> Result<Self, String> {
        let w = words::<16>(&bytes);
        let depth_shift = ((w[0] >> 24) & 3) as u8;
        let far_depth = w[0] & 0x00ff_ffff;
        let far = far_depth >> depth_shift;
        if w[0] >> 26 != 0
            || far_depth == 0
            || far >= 200 * 65536
            || far_depth & ((1 << depth_shift) - 1) != 0
            || w[4] & 0xfe00fe00 != 0
            || w[4] & 511 > 256
            || (w[4] >> 16) & 511 > 256
            || w[5] & 0xff80 != 0
            || (w[5] >> 2) & 31 > 16
            || far_depth >= 65535 * 256
            || w[6..].iter().any(|&v| v != 0)
        {
            return Err("NativeGlobal64 fields/reserved data".into());
        }
        let value = Self {
            far,
            depth_shift,
            light: light::Light {
                direction: [w[1] as i16, (w[1] >> 16) as i16, w[2] as i16],
                ambient: (w[4] & 511) as u16,
                directional: ((w[4] >> 16) & 511) as u16,
            },
            projection: light::Projection {
                ray_scale: [(w[2] >> 16) as i16, w[3] as i16],
                k: (w[3] >> 16) as i16,
            },
            mode: (w[5] & 3) as u8,
            code: ((w[5] >> 2) & 31) as u8,
        };
        light::validate(
            light::PixelInput {
                normal: [0; 3],
                ndc: [0; 2],
            },
            light::Material {
                shininess_code: value.code,
                ..Default::default()
            },
            value.light,
            value.projection,
        )
        .map_err(|error| format!("NativeGlobal64 lighting: {error:?}"))?;
        Ok(value)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Texture {
    base: usize,
    pub size_log2: u8,
    pub full_mip: bool,
    filter: u8,
    framebuffer: bool,
}
impl Texture {
    fn decode(word: u32) -> Result<Self, String> {
        let value = Self {
            base: (word as usize & 65535) * 64,
            size_log2: ((word >> 16) & 15) as u8,
            full_mip: word & (1 << 20) != 0,
            filter: ((word >> 21) & 3) as u8,
            framebuffer: word & (1 << 23) != 0,
        };
        if word >> 24 != 0 || value.base & 127 != 0 || value.size_log2 > 10 || value.filter > 2 {
            return Err("native texture descriptor".into());
        }
        if value.framebuffer
            && (value.size_log2 != 9
                || value.full_mip
                || value.filter > 1
                || value.base & 32767 != 0
                || value.base > 0x3e0000)
        {
            return Err("fixed400x240 texture descriptor".into());
        }
        Ok(value)
    }
    fn prefix(self, level: u8) -> usize {
        if !self.full_mip {
            return 0;
        }
        if level < 4 {
            usize::from(level) * 256
        } else {
            1024 + 256 * ((1usize << (2 * (level - 4))) - 1) / 3
        }
    }
    pub fn validate_extent(self, memory: &super::Memory<'_>) -> Result<(), String> {
        bounds(memory, self.base, self.word_count())?;
        Ok(())
    }
    fn word_count(self) -> usize {
        if self.framebuffer {
            PLANE_WORDS
        } else {
            self.prefix(self.size_log2) + (1usize << (2 * self.size_log2.max(4)))
        }
    }
    pub fn overlaps_target(self, target: [usize; 2]) -> bool {
        target
            .into_iter()
            .any(|base| self.base < base + PLANE_WORDS && base < self.base + self.word_count())
    }
    fn texel(
        self,
        memory: &super::Memory<'_>,
        level: u8,
        x: i128,
        y: i128,
    ) -> Result<[u8; 3], String> {
        let (x, y, pitch) = if self.framebuffer {
            (x.clamp(0, 399) as usize, y.clamp(0, 239) as usize, 4)
        } else {
            let mask = (1 << level) - 1;
            (
                (x & mask) as usize,
                (y & mask) as usize,
                level.saturating_sub(4),
            )
        };
        let offset = ((((x >> 4) << pitch) + (y >> 4)) * 256) + (y & 15) * 16 + (x & 15);
        Ok(expand(
            memory.load(self.base + self.prefix(level) + offset)?,
        ))
    }
    fn sample(
        self,
        memory: &super::Memory<'_>,
        uv: [i128; 2],
        lod: u8,
        report: &mut Report,
        maximum: usize,
    ) -> Result<[u8; 3], String> {
        let lod = if self.full_mip {
            lod.min(self.size_log2 * 8)
        } else {
            0
        };
        let fine = if self.framebuffer {
            9
        } else {
            self.size_log2 - lod / 8
        };
        let coarse_parent = if self.filter == 2 && fine != 0 {
            [0, 64, 128, 192, 256, 319, 383, 447][usize::from(lod & 7)]
        } else {
            0
        };
        let mut result = [0u32; 3];
        for (level, parent) in [
            (fine, 511 - coarse_parent),
            (fine.saturating_sub(1), coarse_parent),
        ] {
            if parent == 0 {
                continue;
            }
            let coordinates: [i128; 2] = std::array::from_fn(|axis| {
                let scaled = if self.framebuffer {
                    (uv[axis].clamp(0, 65535) * [400, 240][axis]) >> 8
                } else {
                    ((uv[axis] & 65535) * (1 << level)) >> 8
                };
                let coordinate = scaled - if self.filter == 0 { 0 } else { 128 };
                if self.framebuffer {
                    coordinate.clamp(0, [399, 239][axis] * 256)
                } else {
                    coordinate
                }
            });
            let x = coordinates[0] >> 8;
            let y = coordinates[1] >> 8;
            let fx = (coordinates[0] & 255) as u32;
            let fy = (coordinates[1] & 255) as u32;
            let weights = if self.filter == 0 {
                [parent, 0, 0, 0]
            } else {
                let bottom = (parent * fy) >> 8;
                let top = parent - bottom;
                let right_top = (top * fx) >> 8;
                let right_bottom = (bottom * fx) >> 8;
                [
                    top - right_top,
                    right_top,
                    bottom - right_bottom,
                    right_bottom,
                ]
            };
            for (tap, weight) in weights.into_iter().enumerate() {
                if weight == 0 {
                    continue;
                }
                report.charge(1, maximum)?;
                let color =
                    self.texel(memory, level, x + (tap & 1) as i128, y + (tap >> 1) as i128)?;
                for channel in 0..3 {
                    result[channel] += (u32::from(color[channel]) * weight + 256) >> 9;
                }
            }
        }
        Ok(result.map(|v| v as u8))
    }
}

#[derive(Clone, Copy)]
pub(super) struct RenderState {
    pub texture: Texture,
    pub lod_bias: i8,
    specular: [u8; 3],
    control: u8,
}
impl RenderState {
    pub fn decode(bytes: [u8; 32]) -> Result<Self, String> {
        let w = words::<8>(&bytes);
        if w[1] >> 24 != 0 || w[2] & !63 != 0 || w[3] >> 8 != 0 || w[4..].iter().any(|&v| v != 0) {
            return Err("NativeRender32 fields/reserved data".into());
        }
        Ok(Self {
            texture: Texture::decode(w[0])?,
            lod_bias: w[3] as i8,
            specular: std::array::from_fn(|c| (w[1] >> (c * 8)) as u8),
            control: w[2] as u8,
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct DrawState {
    pub global: Global,
    pub render: RenderState,
    pub target: [usize; 2],
    pub color: u32,
    pub rgb: bool,
}

fn expand(pixel: u16) -> [u8; 3] {
    let r = (pixel >> 11) as u8;
    let g = ((pixel >> 5) & 63) as u8;
    let b = (pixel & 31) as u8;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}
fn pack(rgb: [u8; 3]) -> u16 {
    let q: [u16; 3] = std::array::from_fn(|c| (u16::from(rgb[c]) * [31, 63, 31][c] + 127) / 255);
    (q[0] << 11) | (q[1] << 5) | q[2]
}

fn lighting(pixel: light::PixelInput, global: Global, render: RenderState) -> [u16; 2] {
    if render.control & 32 != 0 || global.mode == 0 {
        return [256, 0];
    }
    if global.mode == 1 || global.light.directional == 0 {
        return [global.light.ambient, 0];
    }
    super::lighting::evaluate(
        pixel.normal,
        pixel.ndc,
        global.light,
        global.projection,
        global.code,
        global.mode,
    )
}

pub(super) fn shade(
    memory: &super::Memory<'_>,
    xy: [usize; 2],
    fragment: &Fragment,
    state: &DrawState,
    lod: u8,
    report: &mut Report,
    maximum: usize,
) -> Result<[u8; 3], String> {
    let [x, y] = xy;
    let DrawState {
        global,
        render,
        color: uniform,
        rgb: rgb_mode,
        ..
    } = *state;
    report.charge(1, maximum)?;
    let source = if rgb_mode {
        fragment.rgb
    } else {
        render
            .texture
            .sample(memory, fragment.uv, lod, report, maximum)?
    };
    let pixel = light::PixelInput {
        normal: fragment.normal.map(|v| v * 16),
        ndc: light::pixel_center_ndc(x as u16, y as u16, 400, 240)
            .map_err(|e| format!("pixel NDC: {e:?}"))?,
    };
    let [g, h] = lighting(pixel, global, render);
    let color: [u8; 3] = std::array::from_fn(|c| {
        let p = u32::from((uniform >> (c * 8)) as u8) * u32::from(source[c]);
        let t = p + 128;
        let base = (t + (t >> 8)) >> 8;
        (((base * u32::from(g)) >> 8) + ((u32::from(render.specular[c]) * u32::from(h)) >> 8))
            .min(255) as u8
    });
    Ok(color)
}

pub(super) fn fragment(
    memory: &mut super::Memory<'_>,
    xy: [usize; 2],
    fragment: Fragment,
    state: &DrawState,
    lod: u8,
    report: &mut Report,
    maximum: usize,
) -> Result<(), String> {
    let color = shade(memory, xy, &fragment, state, lod, report, maximum)?;
    rop(memory, xy, fragment.depth, color, state)
}

// Workers only read immutable memory. Work charges and failures are retired by
// the caller in the original quad/lane order, before each actual ROP operation.
pub(super) struct PreparedFragment {
    work: usize,
    result: Result<([u8; 3], u16), String>,
}
#[cfg(feature = "functional-batch")]
impl PreparedFragment {
    pub(super) fn finish(self) -> Result<(usize, [u8; 3], u16), String> {
        let (color, depth) = self.result?;
        Ok((self.work, color, depth))
    }
}
pub(super) fn prepare(
    memory: &super::Memory<'_>,
    xy: [usize; 2],
    fragment: Fragment,
    state: &DrawState,
    lod: u8,
) -> PreparedFragment {
    let mut report = Report::default();
    let result = shade(memory, xy, &fragment, state, lod, &mut report, usize::MAX)
        .map(|color| (color, fragment.depth));
    PreparedFragment {
        work: report.work,
        result,
    }
}
pub(super) fn commit(
    memory: &mut super::Memory<'_>,
    xy: [usize; 2],
    prepared: PreparedFragment,
    state: &DrawState,
    report: &mut Report,
    maximum: usize,
) -> Result<(), String> {
    report.charge(prepared.work, maximum)?;
    let (color, depth) = prepared.result?;
    rop(memory, xy, depth, color, state)
}

pub(super) trait PixelMemory {
    fn load(&self, address: usize) -> Result<u16, String>;
    fn store(&mut self, address: usize, value: u16) -> Result<(), String>;
}
impl PixelMemory for super::Memory<'_> {
    fn load(&self, address: usize) -> Result<u16, String> {
        super::Memory::load(self, address)
    }
    fn store(&mut self, address: usize, value: u16) -> Result<(), String> {
        super::Memory::store(self, address, value)
    }
}

pub(super) fn rop<M: PixelMemory>(
    memory: &mut M,
    [x, y]: [usize; 2],
    incoming_depth: u16,
    color: [u8; 3],
    state: &DrawState,
) -> Result<(), String> {
    let target = state.target;
    let render = state.render;
    let uniform = state.color;
    let offset = super::column_offset(x, y);
    let depth = memory.load(target[1] + offset)?;
    let passing = match render.control & 7 {
        0 => false,
        1 => incoming_depth < depth,
        2 => incoming_depth == depth,
        3 => incoming_depth <= depth,
        4 => incoming_depth > depth,
        5 => incoming_depth != depth,
        6 => incoming_depth >= depth,
        _ => true,
    };
    if !passing {
        return Ok(());
    }
    if render.control & 8 != 0 {
        memory.store(target[1] + offset, incoming_depth)?;
    }
    let alpha = if render.control & 16 != 0 {
        ((uniform >> 24) + 2) / 4
    } else {
        64
    };
    let old = expand(memory.load(target[0] + offset)?);
    let blended = std::array::from_fn(|c| {
        ((64 - alpha) * u32::from(old[c]) + alpha * u32::from(color[c]) + 32) / 64
    });
    memory.store(target[0] + offset, pack(blended.map(|v| v as u8)))?;
    Ok(())
}
