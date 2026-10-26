//! One ordered Pass. High-memory inputs are encoded at their final addresses.
use crate::dsl_rt::*;
mod gpu_assets;
use gpu_assets::*;
mod gpu_ui;
use gpu_ui::*;

// owner, phase(open=1/ended=2/ready=3), used staging words,
// valid Global, valid Render, speculative command cursor, clean for UI,
// installed UI first/end (end=0 means none).
static GPU_BUILD: Buf<u16, 9> = Buf::new([0; 9]);
// Recording is serialized by GPU_BUILD. Encoders fully overwrite their active
// 16/32/64-word output, including padding; do not clear the alignment slack.
static GPU_ENCODING: Buf<u16, 127> = Buf::new([0; 127]);
// Keep up to two directory batches after complete preflight. Larger meshes
// retain the bounded two-pass path; no arena is modified before validation.
static GPU_ENTRIES: Buf<u16, 64> = Buf::new([0; 64]);
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
    if cb[0u16].high_slot != 0 {
        let cursor = b[5u16];
        // The caller maps the command page once for its whole append batch.
        // Only crossing the 32-KiB logical window needs another mapping.
        if cursor & 16383 == 0 && !gpu_builder_commands(cb) {
            return false;
        }
        let p = Ptr::from_addr(0xc000 | (cursor & 16383));
        unsafe {
            p.write(0, low);
            p.write(1, high);
        }
        b[5u16] += 2;
        return true;
    }
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
fn gpu_builder_begin_record(
    mut cb: Array<GpuBuffer>,
    color: u16,
    depth: u16,
    cv: u16,
    dv: u16,
) -> bool {
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
    let mut s: GpuStorage = gpu_encoding_scratch(GPU_ENCODING.as_array());
    if cb[0u16].high_slot != 0 {
        s = gpu_builder_object(cb, object);
    }
    let local: u16 = if cb[0u16].high_slot != 0 {
        object & 63
    } else {
        0
    };
    if gpu_memory_faulted() || !gpu_encode_target(view_of(&mut s), local, color, depth) {
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
    if cb[0u16].high_slot == 0
        && !gpu_builder_write(cb, true, object, unsafe { s.cpu.as_u16_array() }, 16)
    {
        return false;
    }
    let mut b = GPU_BUILD.as_array();
    b[0u16] = cb.as_ptr().addr();
    b[1u16] = 1;
    b[2u16] = 0;
    b[3u16] = 0;
    b[4u16] = 0;
    b[5u16] = 0;
    b[6u16] = 0;
    b[7u16] = 0;
    b[8u16] = 0;
    if !gpu_builder_commands(cb)
        || !gpu_builder_emit(cb, 3, 0x4000)
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
    if cb[0u16].high_slot != 0 {
        GPU_SLOT_TARGETS.as_array()[cb[0u16].high_slot - 1] = object + 1;
    }
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
fn gpu_builder_global_record(cb: Array<GpuBuffer>, g: Array<GpuGlobal>) -> bool {
    if cb[0u16].high_slot != 0 {
        if !gpu_builder_open(cb)
            || !gpu_high_slot_valid(cb)
            || g.as_ptr().addr() >= 0xc000 - 12
            || !gpu_global_valid(g)
        {
            return false;
        }
        if GPU_BUILD[3u16] != 0
            && g[0u16].far_low == GPU_GLOBAL_PREVIOUS.far_low
            && g[0u16].far_high == GPU_GLOBAL_PREVIOUS.far_high
            && g[0u16].light_x == GPU_GLOBAL_PREVIOUS.light_x
            && g[0u16].light_y == GPU_GLOBAL_PREVIOUS.light_y
            && g[0u16].light_z == GPU_GLOBAL_PREVIOUS.light_z
            && g[0u16].ray_x == GPU_GLOBAL_PREVIOUS.ray_x
            && g[0u16].ray_y == GPU_GLOBAL_PREVIOUS.ray_y
            && g[0u16].ray_k == GPU_GLOBAL_PREVIOUS.ray_k
            && g[0u16].ambient == GPU_GLOBAL_PREVIOUS.ambient
            && g[0u16].directional == GPU_GLOBAL_PREVIOUS.directional
            && g[0u16].mode == GPU_GLOBAL_PREVIOUS.mode
            && g[0u16].shininess == GPU_GLOBAL_PREVIOUS.shininess
            && g[0u16].epoch == GPU_GLOBAL_PREVIOUS.epoch
        {
            return true;
        }
        let word = gpu_builder_state_word(cb, 32);
        if word == 65535 {
            return false;
        }
        let mut output = gpu_builder_object(cb, word);
        if gpu_memory_faulted() || !gpu_encode_global(view_of(&mut output), word & 63, g) {
            return false;
        }
        if !gpu_builder_install(cb, word, 32, 3, 264, 0x5000) {
            return false;
        }
        GPU_GLOBAL_PREVIOUS = g[0u16];
        return true;
    }

    let mut s: GpuStorage = gpu_encoding_scratch(GPU_ENCODING.as_array());
    if !gpu_encode_global(view_of(&mut s), 0, g) {
        return false;
    }
    gpu_builder_state(cb, unsafe { s.cpu.as_u16_array() }, 32, 3, 264, 0x5000)
}
fn gpu_builder_render_record(
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
    if cb[0u16].high_slot != 0 {
        if !gpu_builder_open(cb)
            || !gpu_high_slot_valid(cb)
            || !gpu_render_valid(view_of(&mut value))
        {
            return false;
        }
        if GPU_BUILD[4u16] != 0
            && value.texture_sector == GPU_RENDER_PREVIOUS.texture_sector
            && value.size_log2 == GPU_RENDER_PREVIOUS.size_log2
            && value.full_mip == GPU_RENDER_PREVIOUS.full_mip
            && value.filter == GPU_RENDER_PREVIOUS.filter
            && value.specular_low == GPU_RENDER_PREVIOUS.specular_low
            && value.specular_high == GPU_RENDER_PREVIOUS.specular_high
            && value.depth_func == GPU_RENDER_PREVIOUS.depth_func
            && value.depth_write == GPU_RENDER_PREVIOUS.depth_write
            && value.blend == GPU_RENDER_PREVIOUS.blend
            && value.unlit == GPU_RENDER_PREVIOUS.unlit
            && value.lod_bias == GPU_RENDER_PREVIOUS.lod_bias
        {
            return true;
        }
        let word = gpu_builder_state_word(cb, 16);
        if word == 65535 {
            return false;
        }
        let mut output = gpu_builder_object(cb, word);
        if gpu_memory_faulted()
            || !gpu_encode_render(view_of(&mut output), word & 63, view_of(&mut value))
        {
            return false;
        }
        if !gpu_builder_install(cb, word, 16, 4, 272, 0x6000) {
            return false;
        }
        GPU_RENDER_PREVIOUS = value;
        return true;
    }
    let mut s: GpuStorage = gpu_encoding_scratch(GPU_ENCODING.as_array());
    if !gpu_encode_render(view_of(&mut s), 0, view_of(&mut value)) {
        return false;
    }
    gpu_builder_state(cb, unsafe { s.cpu.as_u16_array() }, 16, 4, 272, 0x6000)
}
fn gpu_builder_draw_record(
    mut cb: Array<GpuBuffer>,
    mesh: Array<GpuMesh>,
    matrix: Array<u16>,
    rgba_low: u16,
    rgba_high: u16,
) -> bool {
    gpu_builder_draw_policy(cb, mesh, matrix, rgba_low, rgba_high, false)
}
fn gpu_builder_draw_policy(
    mut cb: Array<GpuBuffer>,
    mesh: Array<GpuMesh>,
    matrix: Array<u16>,
    rgba_low: u16,
    rgba_high: u16,
    known: bool,
) -> bool {
    if !gpu_builder_open(cb) || (!known && !gpu_mesh_root_valid(mesh)) {
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
    if matrix.as_ptr().addr() > 0xc000 - 42 {
        return false;
    }
    let entries = GPU_ENTRIES.as_array();
    let retained = count <= 32;
    let mut i: u16 = 0;
    // Native calls validate the whole directory before modifying either arena.
    // Registered meshes only load the retained compact directory here.
    while i < count && (!known || retained) {
        let n: u16 = if count - i > 16 { 16 } else { count - i };
        let offset: u16 = if retained { i * 2 } else { 0 };
        let batch = unsafe { entries.as_ptr().add(offset as i16).as_u16_array() };
        if !gpu_mesh_batch(mesh, i, batch, n) || (!known && !gpu_mesh_entries_valid(mesh, batch, n))
        {
            return false;
        }
        i += n;
    }
    let mut s: GpuStorage = gpu_encoding_scratch(GPU_ENCODING.as_array());
    if cb[0u16].high_slot != 0 {
        s = gpu_builder_object(cb, word);
    }
    if gpu_memory_faulted() || !gpu_encode_instance(view_of(&mut s), 0, matrix, rgba_low, rgba_high)
    {
        return false;
    }
    if (cb[0u16].high_slot == 0
        && !gpu_builder_write(cb, true, word, unsafe { s.cpu.as_u16_array() }, 64))
        || !gpu_builder_commands(cb)
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
        let offset: u16 = if retained { i * 2 } else { 0 };
        let batch = unsafe { entries.as_ptr().add(offset as i16).as_u16_array() };
        if !retained && !gpu_mesh_batch(mesh, i, batch, n) {
            return false;
        }
        if cb[0u16].high_slot != 0 {
            if !gpu_builder_mesh_commands(cb, batch, i, n) {
                return false;
            }
            previous = (i + n - 1) & 1;
            i += n;
            continue;
        }
        let mut j: u16 = 0;
        while j < n {
            let slot = (i + j) & 1;
            let record = batch[j * 2];
            let payload = batch[j * 2 + 1];
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
    GPU_BUILD[6u16] = 0;
    true
}

// Capacity and directory validation belong to draw_record. Keep the final
// command cursor local for a whole directory batch instead of spilling the
// same shared cursor and entering emit three times for every meshlet.
fn gpu_builder_mesh_commands(
    cb: Array<GpuBuffer>,
    entries: Array<u16>,
    first: u16,
    count: u16,
) -> bool {
    let mut cursor = GPU_BUILD[5u16];
    let mut j: u16 = 0;
    while j < count {
        let index = first + j;
        let slot = index & 1;
        let record = entries[j * 2];
        let payload = entries[j * 2 + 1];
        let low = (record << 12) | ((payload - 1) << 1) | slot;
        let high = 0x2000 | (record >> 4);
        let offset = cursor & 16383;
        if offset == 0 {
            gpu_builder_map(cb[0u16].commands.sector, cursor);
            if gpu_memory_faulted() {
                return false;
            }
        }
        if offset > 16378 {
            // Each command remains two aligned words. A rare batch straddling
            // the window uses emit's existing per-command page transition.
            GPU_BUILD[5u16] = cursor;
            if !gpu_builder_emit(cb, low, high) {
                return false;
            }
            if index == 0 {
                if !gpu_builder_emit(cb, 1, 0x4000) {
                    return false;
                }
            } else if !gpu_builder_emit(cb, slot ^ 1, 0x3000) || !gpu_builder_emit(cb, 1, 0x4000) {
                return false;
            }
            cursor = GPU_BUILD[5u16];
        } else {
            let p = Ptr::from_addr(0xc000 | offset);
            unsafe {
                p.write(0, low);
                p.write(1, high);
            }
            if index == 0 {
                unsafe {
                    p.write(2, 1);
                    p.write(3, 0x4000);
                }
                cursor += 4;
            } else {
                unsafe {
                    p.write(2, slot ^ 1);
                    p.write(3, 0x3000);
                    p.write(4, 1);
                    p.write(5, 0x4000);
                }
                cursor += 6;
            }
        }
        j += 1;
    }
    GPU_BUILD[5u16] = cursor;
    true
}
fn gpu_builder_ui_record(
    mut cb: Array<GpuBuffer>,
    atlas: Array<GpuUiAtlas>,
    cells: Array<u16>,
    first: u16,
    end: u16,
    draw: bool,
) -> bool {
    let commands: u16 = if draw { 6 } else { 4 };
    if !gpu_builder_open(cb)
        || first >= end
        || end > 750
        || !gpu_ui_atlas_valid(atlas)
        || !gpu_ui_cells_valid(cells)
        || cb[0u16].object_words > 32368
        || !gpu_builder_room(cb, gpu_builder_ui_command_words(commands), 6)
    {
        return false;
    }
    let word = (cb[0u16].object_words + 63) & 65472;
    if word > 32368 {
        return false;
    }
    if cb[0u16].high_slot != 0 {
        if !gpu_high_slot_valid(cb) {
            return false;
        }
    } else {
        let mut objects = cb[0u16].objects;
        if !gpu_storage_room(view_of(&mut objects), word, 400, 64) {
            return false;
        }
    }
    let base = atlas[0u16].sector;
    let count = atlas[0u16].sectors;
    if gpu_extents_overlap(
        base,
        count,
        cb[0u16].commands.sector,
        cb[0u16].commands.sectors,
    ) || gpu_extents_overlap(
        base,
        count,
        cb[0u16].objects.sector,
        cb[0u16].objects.sectors,
    ) || gpu_extents_overlap(base, count, cb[0u16].color, 1536)
        || gpu_extents_overlap(base, count, cb[0u16].depth, 1536)
    {
        return false;
    }
    // Upload the caller's packed words directly. Only the final 18 padding bytes
    // and 32-byte descriptor need a small low-memory buffer, not another grid.
    let mut tail: Buf<u16, 25> = Buf::new([0; 25]);
    let mut i: u16 = 0;
    while i < 9 {
        tail[i] = 65535;
        i += 1;
    }
    tail[9u16] = cb[0u16].objects.sector + (word >> 6);
    tail[10u16] = base >> 9;
    let mut uploaded: bool = false;
    if first != 0 || end != 750 {
        let start = first >> 1;
        let source = unsafe { cells.as_ptr().add(start as i16).as_u16_array() };
        uploaded = gpu_builder_write(cb, true, word + start, source, ((end + 1) >> 1) - start)
            && gpu_builder_write(cb, true, word + 375, tail.as_array(), 25);
    } else if cb[0u16].high_slot != 0 {
        uploaded = gpu_window_record_pair(
            cells,
            375,
            tail.as_array(),
            25,
            cb[0u16].objects.sector,
            word,
        );
    } else {
        uploaded = gpu_builder_write(cb, true, word, cells, 375)
            && gpu_builder_write(cb, true, word + 375, tail.as_array(), 25);
    }
    if !uploaded || !gpu_builder_ui_commands(cb, word, first, end, draw) {
        return false;
    }
    // UI preserves Global/Render/uniforms. Every later draw already reloads
    // its Instance and first meshlet, then prefetches each subsequent page.
    cb[0u16].object_words = word + 400;
    cb[0u16].command_words = GPU_BUILD.as_array()[5u16];
    true
}

fn gpu_builder_ui_command_words(payload: u16) -> u16 {
    if GPU_BUILD[6u16] != 0 {
        return payload;
    }
    payload + 4
}

// UI retires after its final physical write. Only a draw can dirty the ROP
// cache again; descriptor DMA still needs its own data-completion wait.
fn gpu_builder_ui_fence(cb: Array<GpuBuffer>) -> bool {
    if !gpu_builder_commands(cb) {
        return false;
    }
    if GPU_BUILD[6u16] == 0
        && (!gpu_builder_emit(cb, 3, 0x4000) || !gpu_builder_emit(cb, 0, 0x7000))
    {
        return false;
    }
    GPU_BUILD[6u16] = 1;
    true
}

fn gpu_builder_ui_range_command(cb: Array<GpuBuffer>, first: u16, end: u16) -> bool {
    let mut low: u16 = 69;
    let mut high: u16 = 0xb000;
    if first != 0 || end != 750 {
        low |= first << 7;
        high |= 0x0800 | (first >> 9) | ((end - 1) << 1);
    }
    gpu_builder_emit(cb, low, high)
}

fn gpu_builder_ui_commands(
    cb: Array<GpuBuffer>,
    word: u16,
    first: u16,
    end: u16,
    draw: bool,
) -> bool {
    if !gpu_builder_ui_fence(cb)
        || !gpu_builder_dma(cb, word + 384, 276, 32)
        || !gpu_builder_emit(cb, 1, 0x4000)
    {
        return false;
    }
    GPU_BUILD[7u16] = first;
    GPU_BUILD[8u16] = end;
    if draw {
        return gpu_builder_ui_range_command(cb, first, end);
    }
    true
}

fn gpu_builder_ui_draw_record(mut cb: Array<GpuBuffer>, first: u16, end: u16) -> bool {
    if !gpu_builder_open(cb)
        || first >= end
        || first < GPU_BUILD[7u16]
        || end > GPU_BUILD[8u16]
        || !gpu_builder_room(cb, gpu_builder_ui_command_words(2), 6)
    {
        return false;
    }
    if !gpu_builder_ui_fence(cb) || !gpu_builder_ui_range_command(cb, first, end) {
        return false;
    }
    cb[0u16].command_words = GPU_BUILD[5u16];
    true
}
fn gpu_builder_ui_page_record(
    mut cb: Array<GpuBuffer>,
    descriptor: u16,
    first: u16,
    end: u16,
    install: bool,
) -> bool {
    let words: u16 = if install { 6 } else { 2 };
    if !gpu_builder_open(cb)
        || !gpu_high_slot_valid(cb)
        || first >= end
        || end > 750
        || (descriptor != GPU_UI_PAGE0_SECTOR + (GPU_UI_DESCRIPTOR_WORD >> 6)
            && descriptor != GPU_UI_PAGE1_SECTOR + (GPU_UI_DESCRIPTOR_WORD >> 6))
        || (!install && (GPU_BUILD[7u16] != 0 || GPU_BUILD[8u16] != 750))
        || !gpu_builder_room(cb, gpu_builder_ui_command_words(words), 6)
    {
        return false;
    }
    let mut cmd: Buf<u16, 2> = Buf::new([0; 2]);
    if install && !gpu_command_dma(cmd.as_ptr(), descriptor << 7, descriptor >> 9, 276, 32) {
        return false;
    }
    if !gpu_builder_ui_fence(cb) {
        return false;
    }
    if install {
        if !gpu_builder_emit(cb, cmd[0u16], cmd[1u16]) || !gpu_builder_emit(cb, 1, 0x4000) {
            return false;
        }
        GPU_BUILD[7u16] = 0;
        GPU_BUILD[8u16] = 750;
    }
    if !gpu_builder_ui_range_command(cb, first, end) {
        return false;
    }
    cb[0u16].command_words = GPU_BUILD[5u16];
    true
}
fn gpu_builder_ui_page(
    cb: Array<GpuBuffer>,
    descriptor: u16,
    first: u16,
    end: u16,
    install: bool,
) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_ui_page_record(cb, descriptor, first, end, install);
    gpu_builder_restore(saved, result)
}
fn gpu_builder_end_record(mut cb: Array<GpuBuffer>) -> bool {
    if !gpu_builder_open(cb) || !gpu_builder_room(cb, 4, 2) {
        return false;
    }
    if !gpu_builder_commands(cb)
        || !gpu_builder_emit(cb, 3, 0x4000)
        || !gpu_builder_emit(cb, 0, 0x7000)
    {
        return false;
    }
    GPU_BUILD.as_array()[1u16] = 2;
    cb[0u16].command_words = GPU_BUILD.as_array()[5u16];
    true
}
fn gpu_builder_finish_record(mut cb: Array<GpuBuffer>) -> bool {
    let mut b = GPU_BUILD.as_array();
    if cb[0u16].state != GPU_CB_BUILDING
        || b[0u16] != cb.as_ptr().addr()
        || b[1u16] != 2
        || gpu_memory_faulted()
        || !gpu_builder_room(cb, 2, 0)
    {
        return false;
    }
    if !gpu_builder_commands(cb) || !gpu_builder_emit(cb, 0, 0xf000) {
        return false;
    }
    let valid = b[5u16];
    if cb[0u16].high_slot != 0 {
        let padding = (0 - valid) & 63;
        // The next 128-byte boundary cannot cross a 32-KiB window. END was
        // already appended; overwrite every padding word from the old frame.
        if padding != 0 {
            gpu_zero(Ptr::from_addr(0xc000 | (valid & 16383)), padding);
            b[5u16] = valid + padding;
        }
    } else {
        while b[5u16] & 63 != 0 {
            if !gpu_builder_emit(cb, 0, 0) {
                return false;
            }
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

static GPU_GLOBAL_PREVIOUS: GpuGlobal = GpuGlobal {
    far_low: 0,
    far_high: 0,
    light_x: 0,
    light_y: 0,
    light_z: 0,
    ray_x: 0,
    ray_y: 0,
    ray_k: 0,
    ambient: 0,
    directional: 0,
    mode: 0,
    shininess: 0,
    epoch: 0,
};
static GPU_RENDER_PREVIOUS: GpuRender = GpuRender {
    texture_sector: 0,
    texture_sectors: 0,
    size_log2: 0,
    full_mip: 0,
    filter: 0,
    specular_low: 0,
    specular_high: 0,
    depth_func: 0,
    depth_write: 0,
    blend: 0,
    unlit: 0,
    lod_bias: 0,
};

// Every builder entry restores DSEG3, including rejection paths. Mapping itself
// is not an ownership transfer: these writes stay private until submit's fence.
fn gpu_builder_restore(saved: u16, result: bool) -> bool {
    unsafe {
        write_dseg_page(3, saved);
    }
    if read_dseg_page(3) != saved {
        gpu_window_fail();
        return false;
    }
    result
}
fn gpu_builder_map(base: u16, word: u16) -> Ptr {
    let sector = base + (word >> 6);
    let page = sector >> 8;
    if read_dseg_page(3) != page {
        unsafe {
            write_dseg_page(3, page);
        }
    }
    if read_dseg_page(3) != page {
        gpu_window_fail();
    }
    Ptr::from_addr(0xc000 | ((sector & 255) << 6) | (word & 63))
}
fn gpu_builder_object(cb: Array<GpuBuffer>, word: u16) -> GpuStorage {
    let p = gpu_builder_map(cb[0u16].objects.sector, word);
    GpuStorage {
        cpu: p.add((0 - (word & 63)) as i16),
        sector: cb[0u16].objects.sector + (word >> 6),
        sectors: 1,
    }
}
fn gpu_builder_state_word(cb: Array<GpuBuffer>, n: u16) -> u16 {
    if !gpu_builder_open(cb)
        || !gpu_high_slot_valid(cb)
        || cb[0u16].object_words > 32768 - n
        || !gpu_builder_room(cb, 8, 6)
    {
        return 65535;
    }
    let word = (cb[0u16].object_words + n - 1) & (0 - n);
    if word > 32768 - n {
        return 65535;
    }
    word
}
fn gpu_builder_install(
    mut cb: Array<GpuBuffer>,
    word: u16,
    n: u16,
    index: u16,
    q: u16,
    opcode: u16,
) -> bool {
    if gpu_memory_faulted()
        || !gpu_builder_commands(cb)
        || !gpu_builder_emit(cb, 3, 0x4000)
        || !gpu_builder_dma(cb, word, q, n * 2)
        || !gpu_builder_emit(cb, 1, 0x4000)
        || !gpu_builder_emit(cb, q >> 2, opcode)
    {
        return false;
    }
    GPU_BUILD[index] = 1;
    cb[0u16].command_words = GPU_BUILD[5u16];
    cb[0u16].object_words = word + n;
    true
}

fn gpu_builder_begin(cb: Array<GpuBuffer>, color: u16, depth: u16, cv: u16, dv: u16) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_begin_record(cb, color, depth, cv, dv);
    gpu_builder_restore(saved, result)
}

fn gpu_builder_global(cb: Array<GpuBuffer>, g: Array<GpuGlobal>) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_global_record(cb, g);
    gpu_builder_restore(saved, result)
}

fn gpu_builder_render(
    cb: Array<GpuBuffer>,
    texture: Array<GpuTexture>,
    r: Array<GpuRender>,
) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_render_record(cb, texture, r);
    gpu_builder_restore(saved, result)
}

fn gpu_builder_draw(
    cb: Array<GpuBuffer>,
    mesh: Array<GpuMesh>,
    matrix: Array<u16>,
    rgba_low: u16,
    rgba_high: u16,
) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_draw_record(cb, mesh, matrix, rgba_low, rgba_high);
    gpu_builder_restore(saved, result)
}
fn gpu_builder_draw_known(
    cb: Array<GpuBuffer>,
    mesh: Array<GpuMesh>,
    matrix: Array<u16>,
    rgba_low: u16,
    rgba_high: u16,
) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_draw_policy(cb, mesh, matrix, rgba_low, rgba_high, true);
    gpu_builder_restore(saved, result)
}

fn gpu_builder_ui(
    cb: Array<GpuBuffer>,
    atlas: Array<GpuUiAtlas>,
    cells: Array<u16>,
    first: u16,
    end: u16,
    draw: bool,
) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_ui_record(cb, atlas, cells, first, end, draw);
    gpu_builder_restore(saved, result)
}

fn gpu_builder_ui_draw(cb: Array<GpuBuffer>, first: u16, end: u16) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_ui_draw_record(cb, first, end);
    gpu_builder_restore(saved, result)
}

fn gpu_builder_end(cb: Array<GpuBuffer>) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_end_record(cb);
    gpu_builder_restore(saved, result)
}

fn gpu_builder_finish(cb: Array<GpuBuffer>) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_finish_record(cb);
    gpu_builder_restore(saved, result)
}

fn gpu_builder_commands(cb: Array<GpuBuffer>) -> bool {
    if cb[0u16].high_slot != 0 {
        gpu_builder_map(cb[0u16].commands.sector, GPU_BUILD[5u16]);
    }
    !gpu_memory_faulted()
}
