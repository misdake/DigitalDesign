//! Current native GPU ABI. RCC represents each u32 as low16/high16.
use crate::dsl_rt::*;
mod gpu_layout;
use gpu_layout::*;
mod gpu_memory;
use gpu_memory::*;
mod gpu_render_profile;
use gpu_render_profile::*;
mod gpu_memcpy;
use gpu_memcpy::*;

pub struct GpuGlobal {
    pub far_low: u16,
    pub far_high: u16,
    pub light_x: i16,
    pub light_y: i16,
    pub light_z: i16,
    pub ray_x: i16,
    pub ray_y: i16,
    pub ray_k: u16,
    pub ambient: u16,
    pub directional: u16,
    pub mode: u16,
    pub shininess: u16,
    pub epoch: u16,
}
pub struct GpuRender {
    pub texture_sector: u16,
    pub texture_sectors: u16,
    pub size_log2: u16,
    pub texture_flags: u16,
    pub filter: u16,
    pub specular_low: u16,
    pub specular_high: u16,
    pub depth_func: u16,
    pub depth_write: u16,
    pub blend: u16,
    pub unlit: u16,
    pub lod_bias: i16,
}
pub struct GpuWide {
    pub low: u16,
    pub high: u16,
}
pub struct GpuSubmission {
    pub count_minus_one: u16,
    pub base_sector: u16,
}

pub fn gpu_encode_submission(
    mut out: Array<GpuSubmission>,
    base_low: u16,
    base_high: u16,
    valid_low: u16,
    valid_high: u16,
) -> bool {
    if base_high >= 128
        || base_low & 127 != 0
        || valid_low & 3 != 0
        || valid_high > 1
        || (valid_high == 1 && valid_low != 0)
        || (valid_high == 0 && valid_low == 0)
    {
        return false;
    }
    let base = (base_high << 9) | (base_low >> 7);
    let sectors: u16 = if valid_high != 0 {
        512
    } else {
        ((valid_low - 1) >> 7) + 1
    };
    if !gpu_extent_valid(base, sectors) {
        return false;
    }
    out[0u16].base_sector = base;
    out[0u16].count_minus_one = ((valid_high << 14) | (valid_low >> 2)) - 1;
    true
}

fn gpu_square(x: i16) -> GpuWide {
    let a = if x < 0 { (-x) as u16 } else { x as u16 };
    let l = a & 255;
    let h = a >> 8;
    let p = l * l;
    let cross = l * h;
    let low = p + (cross << 9);
    let carry: u16 = if low < p { 1 } else { 0 };
    GpuWide {
        low: low,
        high: h * h + (cross >> 7) + carry,
    }
}
fn gpu_unit_light(x: i16, y: i16, z: i16) -> bool {
    if x < -16384 || x > 16384 || y < -16384 || y > 16384 || z < -16384 || z > 16384 {
        return false;
    }
    let a = gpu_square(x);
    let b = gpu_square(y);
    let c = gpu_square(z);
    let ab = a.low + b.low;
    let carry_ab: u16 = if ab < a.low { 1 } else { 0 };
    let low = ab + c.low;
    let carry_c: u16 = if low < ab { 1 } else { 0 };
    let high = a.high + b.high + c.high + carry_ab + carry_c;
    (high == 4095 && low >= 32768) || (high == 4096 && low <= 32768)
}

fn gpu_global_valid(g: Array<GpuGlobal>) -> bool {
    if g[0u16].far_high >= 200
        || (g[0u16].far_high == 0 && g[0u16].far_low == 0)
        || g[0u16].ambient > 256
        || g[0u16].directional > 256
        || g[0u16].mode > 3
        || g[0u16].shininess > 16
        || g[0u16].ray_x < -12288
        || g[0u16].ray_x > 12288
        || g[0u16].ray_y < -12288
        || g[0u16].ray_y > 12288
        || g[0u16].ray_k < 8192
        || g[0u16].ray_k > 12288
        || !gpu_unit_light(g[0u16].light_x, g[0u16].light_y, g[0u16].light_z)
    {
        return false;
    }
    true
}
pub fn gpu_encode_global(s: Array<GpuStorage>, word: u16, g: Array<GpuGlobal>) -> bool {
    if !gpu_storage_room(s, word, 32, 32) || !gpu_global_valid(g) {
        return false;
    }
    let p = s[0u16].cpu.add(word as i16);
    // Validation completes before destination writes. Source parameters must
    // be separate from the caller-reserved object allocation.
    unsafe {
        p.write(0, g[0u16].far_low);
        p.write(1, g[0u16].far_high);
        p.write(2, g[0u16].light_x as u16);
        p.write(3, g[0u16].light_y as u16);
        p.write(4, g[0u16].light_z as u16);
        p.write(5, g[0u16].ray_x as u16);
        p.write(6, g[0u16].ray_y as u16);
        p.write(7, g[0u16].ray_k);
        p.write(8, g[0u16].ambient);
        p.write(9, g[0u16].directional);
        p.write(10, g[0u16].mode | (g[0u16].shininess << 2));
        p.write(11, g[0u16].epoch);
    }
    gpu_zero(p.add(12), 20);
    true
}

fn gpu_render_valid(r: Array<GpuRender>) -> bool {
    if !gpu_render_fields_valid(r) {
        return false;
    }
    let mut sectors: u16 = 0;
    let mut level: u16 = 0;
    while level <= r[0u16].size_log2 {
        let size: u16 = if level < GPU_TEXTURE_MACRO_LOG2 {
            GPU_TEXTURE_MIN_LEVEL_SECTORS
        } else {
            GPU_TEXTURE_MIN_LEVEL_SECTORS << ((level - GPU_TEXTURE_MACRO_LOG2) << 1)
        };
        if r[0u16].texture_flags & 1 != 0 {
            sectors += size;
        } else {
            sectors = size;
        }
        level += 1;
    }
    sectors <= r[0u16].texture_sectors
}
fn gpu_render_fields_valid(r: Array<GpuRender>) -> bool {
    if r[0u16].size_log2 > 10
        || r[0u16].texture_sector & (GPU_TEXTURE_ALIGNMENT_SECTORS - 1) != 0
        || r[0u16].texture_flags > 3
        || r[0u16].filter > 2
        || r[0u16].specular_high > 255
        || r[0u16].depth_func > 7
        || r[0u16].depth_write > 1
        || r[0u16].blend > 1
        || r[0u16].unlit > 1
        || r[0u16].lod_bias < -128
        || r[0u16].lod_bias > 127
        || !gpu_extent_valid(r[0u16].texture_sector, r[0u16].texture_sectors)
    {
        return false;
    }
    true
}
fn gpu_render_view_valid(r: Array<GpuRender>) -> bool {
    GPU_RENDER_PROFILE == 1
        && gpu_render_fields_valid(r)
        && r[0u16].texture_sector & 511 == 0
        && r[0u16].texture_sector <= 63488
        && r[0u16].texture_sectors >= GPU_PLANE_SECTORS
        && r[0u16].size_log2 == 9
        && r[0u16].texture_flags == 0
        && r[0u16].filter <= 1
        && r[0u16].lod_bias == 0
}
pub fn gpu_encode_render(s: Array<GpuStorage>, word: u16, r: Array<GpuRender>) -> bool {
    gpu_encode_render_kind(s, word, r, false)
}
fn gpu_encode_render_kind(
    s: Array<GpuStorage>,
    word: u16,
    r: Array<GpuRender>,
    framebuffer_view: bool,
) -> bool {
    if !gpu_storage_room(s, word, 16, 16) {
        return false;
    }
    if framebuffer_view {
        if !gpu_render_view_valid(r) {
            return false;
        }
    } else {
        if !gpu_render_valid(r) {
            return false;
        }
    }
    let p = s[0u16].cpu.add(word as i16);
    let view_bit: u16 = if framebuffer_view { 128 } else { 0 };
    unsafe {
        p.write(0, r[0u16].texture_sector);
        p.write(
            1,
            r[0u16].size_log2
                | ((r[0u16].texture_flags & 1) << 4)
                | ((r[0u16].texture_flags & 2) << 7)
                | (r[0u16].filter << 5)
                | view_bit,
        );
        p.write(2, r[0u16].specular_low);
        p.write(3, r[0u16].specular_high);
        p.write(
            4,
            r[0u16].depth_func
                | (r[0u16].depth_write << 3)
                | (r[0u16].blend << 4)
                | (r[0u16].unlit << 5),
        );
        p.write(5, 0);
        p.write(6, r[0u16].lod_bias as u16 & 255);
        p.write(7, 0);
    }
    gpu_zero(p.add(8), 8);
    true
}

pub fn gpu_target_valid(color: u16, depth: u16) -> bool {
    color & 511 == 0
        && depth & 511 == 0
        && gpu_extent_valid(color, GPU_PLANE_SECTORS)
        && gpu_extent_valid(depth, GPU_PLANE_SECTORS)
        && !gpu_extents_overlap(color, GPU_PLANE_SECTORS, depth, GPU_PLANE_SECTORS)
}
pub fn gpu_encode_target(s: Array<GpuStorage>, word: u16, color: u16, depth: u16) -> bool {
    if !gpu_storage_room(s, word, 16, 16) || !gpu_target_valid(color, depth) {
        return false;
    }
    let p = s[0u16].cpu.add(word as i16);
    gpu_zero(p, 16);
    gpu_store32(p, (color >> 2) | (depth << 12), depth >> 4);
    true
}

/// Sector units are 128 bytes. Cache ownership/fences belong to the caller.
pub fn gpu_memcpy_valid(source: u16, destination: u16, sectors: u16) -> bool {
    gpu_memcpy_native_valid(source, destination, sectors)
}

pub fn gpu_encode_memcpy(
    s: Array<GpuStorage>,
    word: u16,
    source: u16,
    destination: u16,
    sectors: u16,
) -> bool {
    if !gpu_storage_room(s, word, 16, 16) || !gpu_memcpy_valid(source, destination, sectors) {
        return false;
    }
    let out = unsafe { s[0u16].cpu.add(word as i16).as_u16_array() };
    gpu_memcpy_native(out, source, destination, sectors)
}

/// Write one command naming a 32-byte local staging slot (66 through 79).
pub fn gpu_encode_memcpy_command(s: Array<GpuStorage>, word: u16, local_slot: u16) -> bool {
    if !gpu_storage_room(s, word, 2, 2) || local_slot < 66 || local_slot >= 80 {
        return false;
    }
    gpu_store32(s[0u16].cpu.add(word as i16), local_slot, 0xc000);
    true
}

pub fn gpu_encode_instance(
    s: Array<GpuStorage>,
    word: u16,
    matrices: Array<u16>,
    rgba_low: u16,
    rgba_high: u16,
) -> bool {
    if !gpu_storage_room(s, word, 64, 64) || matrices.as_ptr().addr() > 65494 {
        return false;
    }
    let p = s[0u16].cpu.add(word as i16);
    let mut i: u16 = 0;
    while i < 42 {
        unsafe {
            p.write(i as i16, matrices[i]);
        }
        i += 1;
    }
    gpu_store32(p.add(42), 0, 1); // word21 = F16 1, never padding.
    gpu_store32(p.add(44), rgba_low, rgba_high);
    gpu_zero(p.add(46), 18);
    true
}

pub fn gpu_command_dma(
    out: Ptr,
    source_low: u16,
    source_high: u16,
    qword: u16,
    bytes: u16,
) -> bool {
    if out.addr() == 65535
        || (bytes != 32 && bytes != 64 && bytes != 128)
        || source_high >= 128
        || source_low & (bytes - 1) != 0
        || qword & 3 != 0
    {
        return false;
    }
    let block = qword >> 2;
    if !((block >= 66 && block + (bytes >> 5) <= 80)
        || (block >= 80 && block <= 92 && block & 3 == 0 && bytes == 128))
    {
        return false;
    }
    // Alignment guarantees the final burst cannot cross the 8 MiB endpoint.
    let code: u16 = if bytes == 32 {
        0
    } else {
        if bytes == 64 {
            1
        } else {
            2
        }
    };
    gpu_store32(
        out,
        (source_low << 5) | (block << 3) | (code << 1),
        0x1000 | (source_high << 5) | (source_low >> 11),
    );
    true
}
pub fn gpu_command_meshlet(out: Ptr, sector: u16, slot: u16, payload_sectors: u16) -> bool {
    if out.addr() == 65535
        || slot > 1
        || payload_sectors == 0
        || payload_sectors > 8
        || !gpu_extent_valid(sector, payload_sectors + 1)
    {
        return false;
    }
    gpu_store32(
        out,
        (sector << 12) | ((payload_sectors - 1) << 1) | slot,
        0x2000 | (sector >> 4),
    );
    true
}
pub fn gpu_command_draw(out: Ptr, slot: u16) -> bool {
    if out.addr() == 65535 || slot > 1 {
        return false;
    }
    gpu_store32(out, slot, 0x3000);
    true
}
pub fn gpu_command_set(out: Ptr, opcode: u16, qword: u16) -> bool {
    let blocks: u16 = if opcode == 5 { 2 } else { 1 };
    let block = qword >> 2;
    if out.addr() == 65535
        || (opcode != 5 && opcode != 6 && opcode != 10)
        || qword & 3 != 0
        || block < 66
        || block + blocks > 80
    {
        return false;
    }
    gpu_store32(out, block, opcode << 12);
    true
}
pub fn gpu_command_fixed(out: Ptr, opcode: u16, value: u16, flag: u16) -> bool {
    if out.addr() == 65535 {
        return false;
    }
    if opcode == 8 {
        if flag > 1 {
            return false;
        }
        gpu_store32(out, value, 0x8000 | flag);
        return true;
    }
    if opcode == 4 {
        if value > 3 || flag != 0 {
            return false;
        }
    } else {
        if (opcode != 0 && opcode != 7 && opcode != 9 && opcode != 15) || value != 0 || flag != 0 {
            return false;
        }
    }
    gpu_store32(out, value, opcode << 12);
    true
}
