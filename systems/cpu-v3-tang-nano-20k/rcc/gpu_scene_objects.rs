//! Prepared scene values. A owns slots, faults and public resource adapters.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_scene_math;
use gpu_scene_math::*;

#[derive(Copy, Clone)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Copy, Clone)]
pub struct Rgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Shininess {
    S4,
    S5,
    S6,
    S7,
    S8,
    S10,
    S12,
    S14,
    S16,
    S20,
    S24,
    S28,
    S32,
    S40,
    S48,
    S56,
    S64,
}

#[derive(Copy, Clone)]
pub struct MaterialDesc {
    pub uniform_color: Rgba8,
    pub specular: Rgb8,
    pub ambient_response: fix32,
    pub directional_response: fix32,
    pub shininess: Shininess,
    pub unlit: bool,
}

// Native Shininess codes index this table; exponent values are not wire codes.
static SHININESS_EXPONENTS: Buf<u16, 17> = Buf::new([
    4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64,
]);

pub fn gpu_scene_shininess_exponent(value: Shininess) -> u16 {
    SHININESS_EXPONENTS[value as u16]
}

fn gpu_scene_shininess_code(code: u16) -> Shininess {
    // Codes are dense; words beyond the last code select S64.
    let valid = if code < Shininess::S64 as u16 {
        code
    } else {
        Shininess::S64 as u16
    };
    unsafe { enum_from_u16_unchecked::<Shininess>(valid) }
}

pub fn gpu_scene_shininess_nearest(exponent: u16) -> Shininess {
    let mut best = Shininess::S4;
    let mut distance: u16 = 65535;
    let mut code: u16 = 0;
    while code < 17 {
        let candidate = gpu_scene_shininess_code(code);
        let value = gpu_scene_shininess_exponent(candidate);
        let next: u16 = if exponent > value {
            exponent - value
        } else {
            value - exponent
        };
        // Ascending candidates and strict comparison choose lower on a tie.
        if next < distance {
            best = candidate;
            distance = next;
        }
        code += 1;
    }
    best
}

#[cfg(gpu_debug)]
fn gpu_scene_material_response_valid(ambient: fix32, directional: fix32) -> bool {
    ambient >= fix32::zero() && directional >= fix32::zero()
}

pub fn gpu_scene_material_color(mut out: Array<u16>, color: Rgba8) -> bool {
    out[GPU_MATERIAL_RGBA] = (color.r as u16) | ((color.g as u16) << 8);
    out[GPU_MATERIAL_RGBA + 1] = (color.b as u16) | ((color.a as u16) << 8);
    true
}

pub fn gpu_scene_material_specular(mut out: Array<u16>, color: Rgb8, shininess: Shininess) -> bool {
    out[GPU_MATERIAL_SPECULAR] = (color.r as u16) | ((color.g as u16) << 8);
    out[GPU_MATERIAL_SPECULAR + 1] = color.b as u16;
    out[GPU_MATERIAL_SHININESS] = shininess as u16;
    true
}

pub fn gpu_scene_material_response(out: Array<u16>, ambient: fix32, directional: fix32) -> bool {
    #[cfg(gpu_debug)]
    {
        if !gpu_scene_material_response_valid(ambient, directional) {
            return false;
        }
    }
    gpu_scene_fix_write(out, GPU_MATERIAL_AMBIENT, ambient);
    gpu_scene_fix_write(out, GPU_MATERIAL_DIRECTIONAL, directional);
    true
}

pub fn gpu_scene_material_unlit(mut out: Array<u16>, unlit: bool) -> bool {
    out[GPU_MATERIAL_FLAGS] = unlit as u16;
    true
}

pub fn gpu_scene_material_init(out: Array<u16>, desc: MaterialDesc) -> bool {
    #[cfg(gpu_debug)]
    {
        if !gpu_scene_material_response_valid(desc.ambient_response, desc.directional_response) {
            return false;
        }
    }
    gpu_scene_zero(out, GPU_API_SMALL_WORDS);
    gpu_scene_material_color(out, desc.uniform_color);
    gpu_scene_material_specular(out, desc.specular, desc.shininess);
    gpu_scene_fix_write(out, GPU_MATERIAL_AMBIENT, desc.ambient_response);
    gpu_scene_fix_write(out, GPU_MATERIAL_DIRECTIONAL, desc.directional_response);
    gpu_scene_material_unlit(out, desc.unlit);
    true
}

pub fn gpu_scene_material_desc(input: Array<u16>) -> MaterialDesc {
    MaterialDesc {
        uniform_color: Rgba8 {
            r: input[GPU_MATERIAL_RGBA] as u8,
            g: (input[GPU_MATERIAL_RGBA] >> 8) as u8,
            b: input[GPU_MATERIAL_RGBA + 1] as u8,
            a: (input[GPU_MATERIAL_RGBA + 1] >> 8) as u8,
        },
        specular: Rgb8 {
            r: input[GPU_MATERIAL_SPECULAR] as u8,
            g: (input[GPU_MATERIAL_SPECULAR] >> 8) as u8,
            b: input[GPU_MATERIAL_SPECULAR + 1] as u8,
        },
        ambient_response: gpu_scene_fix_read(input, GPU_MATERIAL_AMBIENT),
        directional_response: gpu_scene_fix_read(input, GPU_MATERIAL_DIRECTIONAL),
        shininess: gpu_scene_shininess_code(input[GPU_MATERIAL_SHININESS]),
        unlit: input[GPU_MATERIAL_FLAGS] & 1 != 0,
    }
}

#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum DepthFunc {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}
#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum BlendMode {
    Replace,
    SrcOver,
}
#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TextureFilter {
    Nearest,
    Bilinear,
    Trilinear,
}

#[derive(Copy, Clone)]
pub struct Vec3 {
    pub x: fix32,
    pub y: fix32,
    pub z: fix32,
}

#[derive(Copy, Clone)]
pub struct Lighting {
    pub direction: Vec3,
    pub ambient: fix32,
    pub directional: fix32,
}

#[derive(Copy, Clone)]
pub struct RenderStateDesc {
    pub depth: DepthFunc,
    pub depth_write: bool,
    pub blend: BlendMode,
    pub filter: TextureFilter,
    pub lighting: Lighting,
}

#[cfg(gpu_debug)]
fn gpu_scene_state_intensities_valid(light: Lighting) -> bool {
    light.ambient >= fix32::zero() && light.directional >= fix32::zero()
}

pub fn gpu_scene_state_lighting(out: Array<u16>, light: Lighting) -> bool {
    if light.direction.x == fix32::zero()
        && light.direction.y == fix32::zero()
        && light.direction.z == fix32::zero()
    {
        return false;
    }
    #[cfg(gpu_debug)]
    {
        if !gpu_scene_state_intensities_valid(light) {
            return false;
        }
    }
    let direction = vec3::new(light.direction.x, light.direction.y, light.direction.z);
    gpu_scene_vec_write(out, GPU_STATE_LIGHT, gpu_scene_normalize(direction));
    gpu_scene_fix_write(out, GPU_STATE_AMBIENT, light.ambient);
    gpu_scene_fix_write(out, GPU_STATE_DIRECTIONAL, light.directional);
    true
}

pub fn gpu_scene_state_depth(mut out: Array<u16>, func: DepthFunc, write: bool) -> bool {
    out[GPU_STATE_CONTROL] = (out[GPU_STATE_CONTROL] & 16) | (func as u16) | ((write as u16) << 3);
    true
}

pub fn gpu_scene_state_blend(mut out: Array<u16>, mode: BlendMode) -> bool {
    out[GPU_STATE_CONTROL] = (out[GPU_STATE_CONTROL] & 15) | ((mode as u16) << 4);
    true
}

pub fn gpu_scene_state_filter(mut out: Array<u16>, filter: TextureFilter) -> bool {
    out[GPU_STATE_FILTER] = filter as u16;
    true
}

pub fn gpu_scene_state_init(mut out: Array<u16>, desc: RenderStateDesc) -> bool {
    if !gpu_scene_state_lighting(out, desc.lighting) {
        return false;
    }
    gpu_scene_state_depth(out, desc.depth, desc.depth_write);
    gpu_scene_state_blend(out, desc.blend);
    gpu_scene_state_filter(out, desc.filter);
    let mut at: u16 = 12;
    while at < GPU_API_SMALL_WORDS {
        out[at] = 0;
        at += 1;
    }
    true
}

#[repr(u16)]
#[derive(Copy, Clone)]
pub enum TextureFormat {
    Rgb565,
    Rgba4444,
}

/// POT address policy. Framebuffer views have their existing fixed clamp.
#[repr(u16)]
#[derive(Copy, Clone)]
pub enum TextureAddressMode {
    Repeat,
    ClampToEdge,
}

#[derive(Copy, Clone)]
pub struct TextureDesc {
    pub memory: GpuMemory,
    pub size_log2: u16,
    pub full_mip: bool,
    pub format: TextureFormat,
}

pub fn gpu_scene_texture_init(mut out: Array<u16>, desc: TextureDesc) -> bool {
    // GpuMemory registration owns the physical endpoint check. These are the
    // texture's format/alignment/capacity checks, performed only on construction.
    if desc.size_log2 > 10 || desc.memory.first_sector() & 1 != 0 {
        return false;
    }
    let mut required: u16 = 0;
    let mut level: u16 = 0;
    while level <= desc.size_log2 {
        let mut sectors: u16 = 4;
        if level >= 4 {
            sectors = 4 << ((level - 4) << 1);
        }
        if desc.full_mip {
            required += sectors;
        } else {
            required = sectors;
        }
        level += 1;
    }
    if desc.memory.sector_count() < required {
        return false;
    }
    gpu_scene_zero(out, GPU_API_SMALL_WORDS);
    out[GPU_VIEW_BASE] = desc.memory.first_sector();
    out[GPU_VIEW_COUNT] = desc.memory.sector_count();
    out[GPU_VIEW_EXPONENT] = desc.size_log2;
    out[GPU_VIEW_FLAGS] = (desc.full_mip as u16) | ((desc.format as u16) << 2);
    out[GPU_VIEW_WIDTH] = 1 << desc.size_log2;
    out[GPU_VIEW_HEIGHT] = 1 << desc.size_log2;
    true
}

pub fn gpu_scene_texture_view(mut out: Array<u16>, source: Array<u16>) -> bool {
    let mut at: u16 = 0;
    while at < GPU_API_SMALL_WORDS {
        out[at] = source[at];
        at += 1;
    }
    true
}

pub fn gpu_scene_color_view(mut out: Array<u16>, memory: GpuMemory) -> bool {
    if memory.first_sector() & 511 != 0 || memory.sector_count() < GPU_API_PLANE_SECTORS {
        return false;
    }
    gpu_scene_zero(out, GPU_API_SMALL_WORDS);
    out[GPU_VIEW_BASE] = memory.first_sector();
    out[GPU_VIEW_COUNT] = memory.sector_count();
    out[GPU_VIEW_EXPONENT] = 9;
    out[GPU_VIEW_FLAGS] = 2;
    out[GPU_VIEW_WIDTH] = 400;
    out[GPU_VIEW_HEIGHT] = 240;
    true
}

pub fn gpu_scene_view_width(source: Array<u16>) -> u16 {
    source[GPU_VIEW_WIDTH]
}
pub fn gpu_scene_view_height(source: Array<u16>) -> u16 {
    source[GPU_VIEW_HEIGHT]
}
