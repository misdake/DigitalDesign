//! One ordered Pass. Bounded staging batches writes without removing fences.
use crate::dsl_rt::*;
mod gpu_assets;
use gpu_assets::*;

// owner, phase(open=1/ended=2/ready=3), used staging words,
// valid Global, valid Render, speculative command cursor.
static GPU_BUILD: Buf<u16, 6> = Buf::new([0; 6]);
static GPU_CODE: Buf<u16, 128> = Buf::new([0; 128]);
static GPU_GLOBAL_VALUE: Buf<u16, 32> = Buf::new([0; 32]);
static GPU_RENDER_VALUE: Buf<u16, 16> = Buf::new([0; 16]);

fn gpu_builder_write(
    cb: Array<GpuBuffer>,
    objects: bool,
    word: u16,
    data: Array<u16>,
    count: u16,
) -> bool {
    if cb[0u16].high_slot != 0 {
        return gpu_slot_upload(cb, objects, word, data, count);
    }
    let mut s: GpuStorage = cb[0u16].objects;
    if !objects {
        s = cb[0u16].commands;
    }
    if !gpu_storage_room(view_of(&mut s), word, count, 1) {
        return false;
    }
    let p = s.cpu.add(word as i16);
    let mut i: u16 = 0;
    while i < count {
        unsafe {
            p.write(i as i16, data[i]);
        }
        i += 1;
    }
    true
}
fn gpu_builder_room(cb: Array<GpuBuffer>, add: u16, tail: u16) -> bool {
    let cursor = cb[0u16].command_words;
    if cursor > 32768 || tail > 32768 - cursor || add > 32768 - cursor - tail {
        return false;
    }
    let end = cursor + add + tail;
    let padded = (end + 63) & 65472;
    if cb[0u16].high_slot != 0 {
        return padded <= 32768;
    }
    let mut s: GpuStorage = cb[0u16].commands;
    gpu_storage_room(view_of(&mut s), 0, padded, 1)
}
fn gpu_builder_open(cb: Array<GpuBuffer>) -> bool {
    let b = GPU_BUILD.as_array();
    cb[0u16].state == GPU_CB_BUILDING
        && b[0u16] == cb.as_ptr().addr()
        && b[1u16] == 1
        && !gpu_memory_faulted()
}
fn gpu_builder_flush(cb: Array<GpuBuffer>) -> bool {
    let mut b = GPU_BUILD.as_array();
    let used = b[2u16];
    if used == 0 {
        return true;
    }
    if !gpu_builder_write(cb, false, b[5u16] - used, GPU_CODE.as_array(), used) {
        return false;
    }
    b[2u16] = 0;
    true
}
fn gpu_builder_emit(cb: Array<GpuBuffer>, low: u16, high: u16) -> bool {
    let mut b = GPU_BUILD.as_array();
    let mut code = GPU_CODE.as_array();
    let used = b[2u16];
    code[used] = low;
    code[used + 1] = high;
    b[2u16] = used + 2;
    b[5u16] += 2;
    if b[2u16] == 128 {
        return gpu_builder_flush(cb);
    }
    true
}
fn gpu_builder_dma(cb: Array<GpuBuffer>, object: u16, qword: u16, bytes: u16) -> bool {
    let sector = cb[0u16].objects.sector + (object >> 6);
    let mut cmd: Buf<u16, 2> = Buf::new([0; 2]);
    if !gpu_command_dma(
        cmd.as_ptr(),
        (sector << 7) | ((object & 63) << 1),
        sector >> 9,
        qword,
        bytes,
    ) {
        return false;
    }
    gpu_builder_emit(cb, cmd[0u16], cmd[1u16])
}
fn gpu_builder_begin(mut cb: Array<GpuBuffer>, color: u16, depth: u16, cv: u16, dv: u16) -> bool {
    if cb[0u16].state != GPU_CB_BUILDING
        || cb[0u16].command_words != 0
        || cb[0u16].object_words > 32752
        || !gpu_target_valid(color, depth)
        || gpu_memory_faulted()
        || !gpu_builder_room(cb, 14, 6)
    {
        return false;
    }
    if cb[0u16].high_slot != 0
        && (!gpu_high_slot_valid(cb)
            || (color != GPU_COLOR_A_SECTOR && color != GPU_COLOR_B_SECTOR)
            || depth != GPU_DEPTH_SECTOR)
    {
        return false;
    }
    let object = (cb[0u16].object_words + 15) & 65520;
    let mut raw: Buf<u16, 127> = Buf::new([0; 127]);
    let mut s: GpuStorage = gpu_encoding_scratch(raw.as_array());
    if !gpu_encode_target(view_of(&mut s), 0, color, depth) {
        return false;
    }
    if cb[0u16].high_slot == 0 {
        let mut o: GpuStorage = cb[0u16].objects;
        let mut c: GpuStorage = cb[0u16].commands;
        if !gpu_storage_room(view_of(&mut o), object, 16, 16)
            || gpu_extents_overlap(c.sector, c.sectors, color, 1536)
            || gpu_extents_overlap(c.sector, c.sectors, depth, 1536)
            || gpu_extents_overlap(o.sector, o.sectors, color, 1536)
            || gpu_extents_overlap(o.sector, o.sectors, depth, 1536)
        {
            return false;
        }
    }
    if !gpu_builder_write(cb, true, object, unsafe { s.cpu.as_u16_array() }, 16) {
        return false;
    }
    let mut b = GPU_BUILD.as_array();
    b[0u16] = cb.as_ptr().addr();
    b[1u16] = 1;
    b[2u16] = 0;
    b[3u16] = 0;
    b[4u16] = 0;
    b[5u16] = 0;
    if !gpu_builder_emit(cb, 3, 0x4000)
        || !gpu_builder_emit(cb, 0, 0x7000)
        || !gpu_builder_dma(cb, object, 276, 32)
        || !gpu_builder_emit(cb, 1, 0x4000)
        || !gpu_builder_emit(cb, 69, 0xa000)
        || !gpu_builder_emit(cb, cv, 0x8000)
        || !gpu_builder_emit(cb, dv, 0x8001)
    {
        return false;
    }
    cb[0u16].command_words = b[5u16];
    cb[0u16].object_words = object + 16;
    cb[0u16].color = color;
    cb[0u16].depth = depth;
    true
}
fn gpu_builder_state(
    mut cb: Array<GpuBuffer>,
    encoded: Array<u16>,
    n: u16,
    index: u16,
    q: u16,
    opcode: u16,
) -> bool {
    if !gpu_builder_open(cb) {
        return false;
    }
    let mut b = GPU_BUILD.as_array();
    let mut cached = GPU_GLOBAL_VALUE.as_array();
    if index == 4 {
        cached = GPU_RENDER_VALUE.as_array();
    }
    let mut same = b[index] != 0;
    let mut i: u16 = 0;
    while i < n {
        if cached[i] != encoded[i] {
            same = false;
        }
        i += 1;
    }
    if same {
        return true;
    }
    if cb[0u16].object_words > 32768 - n || !gpu_builder_room(cb, 8, 6) {
        return false;
    }
    let word = (cb[0u16].object_words + n - 1) & (0 - n);
    if word > 32768 - n {
        return false;
    }
    if !gpu_builder_write(cb, true, word, encoded, n) {
        return false;
    }
    if !gpu_builder_emit(cb, 3, 0x4000)
        || !gpu_builder_dma(cb, word, q, n * 2)
        || !gpu_builder_emit(cb, 1, 0x4000)
        || !gpu_builder_emit(cb, q >> 2, opcode)
    {
        return false;
    }
    i = 0;
    while i < n {
        cached[i] = encoded[i];
        i += 1;
    }
    b[index] = 1;
    cb[0u16].command_words = b[5u16];
    cb[0u16].object_words = word + n;
    true
}
fn gpu_builder_global(cb: Array<GpuBuffer>, g: Array<GpuGlobal>) -> bool {
    let mut raw: Buf<u16, 127> = Buf::new([0; 127]);
    let mut s: GpuStorage = gpu_encoding_scratch(raw.as_array());
    if !gpu_encode_global(view_of(&mut s), 0, g) {
        return false;
    }
    gpu_builder_state(cb, unsafe { s.cpu.as_u16_array() }, 32, 3, 264, 0x5000)
}
fn gpu_builder_render(
    cb: Array<GpuBuffer>,
    texture: Array<GpuTexture>,
    r: Array<GpuRender>,
) -> bool {
    if !gpu_asset_extent(texture[0u16].sector, texture[0u16].sectors) {
        return false;
    }
    let mut value: GpuRender = r[0u16];
    value.texture_sector = texture[0u16].sector;
    value.texture_sectors = texture[0u16].sectors;
    value.size_log2 = texture[0u16].size_log2;
    value.full_mip = texture[0u16].full_mip;
    let mut raw: Buf<u16, 127> = Buf::new([0; 127]);
    let mut s: GpuStorage = gpu_encoding_scratch(raw.as_array());
    if !gpu_encode_render(view_of(&mut s), 0, view_of(&mut value)) {
        return false;
    }
    gpu_builder_state(cb, unsafe { s.cpu.as_u16_array() }, 16, 4, 272, 0x6000)
}
fn gpu_builder_draw(
    mut cb: Array<GpuBuffer>,
    mesh: Array<GpuMesh>,
    matrix: Array<u16>,
    rgba_low: u16,
    rgba_high: u16,
) -> bool {
    if !gpu_builder_open(cb) || !gpu_mesh_root_valid(mesh) {
        return false;
    }
    let count = mesh[0u16].count;
    if count == 0 {
        return true;
    }
    let b = GPU_BUILD.as_array();
    if b[3u16] == 0
        || b[4u16] == 0
        || cb[0u16].object_words > 32704
        || !gpu_builder_room(cb, count * 6 + 4, 6)
    {
        return false;
    }
    let word = (cb[0u16].object_words + 63) & 65472;
    if word > 32704 {
        return false;
    }
    let mut raw: Buf<u16, 127> = Buf::new([0; 127]);
    let mut s: GpuStorage = gpu_encoding_scratch(raw.as_array());
    if !gpu_encode_instance(view_of(&mut s), 0, matrix, rgba_low, rgba_high) {
        return false;
    }
    let mut entries: Buf<u16, 32> = Buf::new([0; 32]);
    let mut i: u16 = 0;
    // Preflight the entire compact directory before modifying either arena.
    while i < count {
        let n: u16 = if count - i > 16 { 16 } else { count - i };
        if !gpu_mesh_batch(mesh, i, entries.as_array(), n)
            || !gpu_mesh_entries_valid(mesh, entries.as_array(), n)
        {
            return false;
        }
        i += n;
    }
    if !gpu_builder_write(cb, true, word, unsafe { s.cpu.as_u16_array() }, 64)
        || !gpu_builder_dma(cb, word, 320, 128)
        || !gpu_builder_emit(cb, 1, 0x4000)
    {
        return false;
    }
    // Keep one entry ahead: DMA(next) overlaps DRAW(current), alternating pages.
    i = 0;
    let mut previous: u16 = 0;
    while i < count {
        let n: u16 = if count - i > 16 { 16 } else { count - i };
        if !gpu_mesh_batch(mesh, i, entries.as_array(), n) {
            return false;
        }
        let mut j: u16 = 0;
        while j < n {
            let slot = (i + j) & 1;
            let record = entries[j * 2];
            let payload = entries[j * 2 + 1];
            if !gpu_builder_emit(
                cb,
                (record << 12) | ((payload - 1) << 1) | slot,
                0x2000 | (record >> 4),
            ) {
                return false;
            }
            if i + j == 0 {
                if !gpu_builder_emit(cb, 1, 0x4000) {
                    return false;
                }
            } else {
                if !gpu_builder_emit(cb, previous, 0x3000) || !gpu_builder_emit(cb, 1, 0x4000) {
                    return false;
                }
            }
            previous = slot;
            j += 1;
        }
        i += n;
    }
    if !gpu_builder_emit(cb, previous, 0x3000) {
        return false;
    }
    cb[0u16].command_words = GPU_BUILD.as_array()[5u16];
    cb[0u16].object_words = word + 64;
    true
}
fn gpu_builder_end(mut cb: Array<GpuBuffer>) -> bool {
    if !gpu_builder_open(cb) || !gpu_builder_room(cb, 4, 2) {
        return false;
    }
    if !gpu_builder_emit(cb, 3, 0x4000) || !gpu_builder_emit(cb, 0, 0x7000) {
        return false;
    }
    GPU_BUILD.as_array()[1u16] = 2;
    cb[0u16].command_words = GPU_BUILD.as_array()[5u16];
    true
}
fn gpu_builder_finish(mut cb: Array<GpuBuffer>) -> bool {
    let mut b = GPU_BUILD.as_array();
    if cb[0u16].state != GPU_CB_BUILDING
        || b[0u16] != cb.as_ptr().addr()
        || b[1u16] != 2
        || gpu_memory_faulted()
        || !gpu_builder_room(cb, 2, 0)
    {
        return false;
    }
    if !gpu_builder_emit(cb, 0, 0xf000) {
        return false;
    }
    let valid = b[5u16];
    while b[5u16] & 63 != 0 {
        if !gpu_builder_emit(cb, 0, 0) {
            return false;
        }
    }
    if !gpu_builder_flush(cb) {
        return false;
    }
    cb[0u16].command_words = valid;
    cb[0u16].state = GPU_CB_READY;
    b[1u16] = 3;
    true
}
