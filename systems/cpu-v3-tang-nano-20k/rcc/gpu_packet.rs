//! Native Pass prefix assembly over the independent CPU record core.
use crate::dsl_rt::*;
mod gpu_record;
use gpu_record::*;

fn gpu_packet_invalidate_textures(slot: u16) -> bool {
    let code: Buf<u16, 4> = Buf::new([3, 0x4000, 0, 0x9000]);
    // This command neither changes binding FFs nor writes framebuffer data.
    gpu_record_append(slot, code.as_array(), 4, code.as_array(), 0, 1)
}

// Native DMA_LOCAL; all callers use legal aligned staging blocks and lengths.
fn gpu_packet_dma(mut code: Array<u16>, at: u16, c: Array<u16>, word: u16, local: u16, size: u16) {
    let source = c[GPU_RECORD_PUSH_BASE] + (word >> 6);
    code[at] = (source << 12) | ((word & 63) << 6) | (local << 3) | (size << 1);
    code[at + 1] = 0x1000 | (source >> 4);
}

fn gpu_packet_target_valid(c: Array<u16>, color: u16, depth: u16) -> bool {
    if color & 511 != 0
        || depth & 511 != 0
        || !gpu_memory_extent_valid(color, GPU_API_PLANE_SECTORS)
        || !gpu_memory_extent_valid(depth, GPU_API_PLANE_SECTORS)
        || gpu_record_overlap(color, GPU_API_PLANE_SECTORS, depth, GPU_API_PLANE_SECTORS)
    {
        return false;
    }
    let cb = c[GPU_RECORD_COMMAND_BASE];
    let cn = c[GPU_RECORD_COMMAND_CAPACITY] >> 6;
    let pb = c[GPU_RECORD_PUSH_BASE];
    let pn = c[GPU_RECORD_PUSH_CAPACITY] >> 6;
    !gpu_record_overlap(cb, cn, color, GPU_API_PLANE_SECTORS)
        && !gpu_record_overlap(cb, cn, depth, GPU_API_PLANE_SECTORS)
        && !gpu_record_overlap(pb, pn, color, GPU_API_PLANE_SECTORS)
        && !gpu_record_overlap(pb, pn, depth, GPU_API_PLANE_SECTORS)
}
// clear_mask bit0=color, bit1=depth; zero preserves both planes (Load).
fn gpu_packet_begin(slot: u16, color: u16, depth: u16, clear_mask: u16, cv: u16, dv: u16) -> bool {
    if slot >= 2 || clear_mask > 3 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_READY || !gpu_packet_target_valid(c, color, depth) {
        return false;
    }
    let mut words: u16 = 10;
    if clear_mask & 1 != 0 {
        words += 2;
    }
    if clear_mask & 2 != 0 {
        words += 2;
    }
    let target = gpu_record_push_offset(slot, 16, 16);
    if target == 65535 || !gpu_record_command_room(c, words, 6) {
        return false;
    }
    let mut native = gpu_native_scratch();
    let mut i: u16 = GPU_SCRATCH_TARGET;
    while i < GPU_SCRATCH_INSTANCE {
        native[i] = 0;
        i += 1;
    }
    native[GPU_SCRATCH_TARGET] = (color >> 2) | (depth << 12);
    native[GPU_SCRATCH_TARGET + 1] = depth >> 4;
    // Native DMA_LOCAL source/32 field; slot69 is the existing Target32 staging.
    let mut code: Buf<u16, 14> = Buf::new([
        3, 0x4000, 0, 0x7000, 0, 0, 1, 0x4000, 69, 0xa000, 0, 0, 0, 0,
    ]);
    gpu_packet_dma(code.as_array(), 4, c, target, 69, 0);
    i = 10;
    if clear_mask & 1 != 0 {
        code[i] = cv;
        code[i + 1] = 0x8000;
        i += 2;
    }
    if clear_mask & 2 != 0 {
        code[i] = dv;
        code[i + 1] = 0x8001;
    }
    if !gpu_record_begin_pass(slot) {
        return false;
    }
    let payload = unsafe {
        gpu_native_scratch()
            .as_ptr()
            .add(GPU_SCRATCH_TARGET as i16)
            .as_u16_array()
    };
    if !gpu_record_append(slot, code.as_array(), words, payload, 16, 16) {
        return false;
    }
    c[GPU_RECORD_COLOR] = color;
    c[GPU_RECORD_DEPTH] = depth;
    c[GPU_RECORD_TARGET_WORD] = target + 1;
    c[GPU_RECORD_TARGET_BLOCKS] += 1;
    // Target DMA overwrites the shared UI descriptor staging slot69.
    c[GPU_RECORD_VALID] &= 65503;
    c[GPU_RECORD_UI_CLEAN] = 0;
    true
}
