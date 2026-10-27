//! In-place native UI object construction; no mapped pointer escapes a call.
use crate::dsl_rt::*;
mod gpu_builder;
use gpu_builder::*;

// Final object word, first cell, exclusive end cell.
static GPU_UI_POSITION: Buf<u16, 3> = Buf::new([0; 3]);

fn gpu_builder_ui_active(cb: Array<GpuBuffer>) -> bool {
    cb[0u16].state == GPU_CB_BUILDING
        && GPU_BUILD[0u16] == cb.as_ptr().addr()
        && GPU_BUILD[1u16] == 4
        && !gpu_memory_faulted()
        && gpu_high_slot_valid(cb)
}

// Store-only kernel: native grid/padding values are generated at destination.
fn gpu_ui_fill_words(destination: Ptr, words: u16, value: u16) {
    let mut p = destination;
    let mut count = words;
    while count >= 8 {
        unsafe {
            p.write(0, value);
            p.write(1, value);
            p.write(2, value);
            p.write(3, value);
            p.write(4, value);
            p.write(5, value);
            p.write(6, value);
            p.write(7, value);
        }
        p = p.add(8);
        count -= 8;
    }
    while count != 0 {
        unsafe {
            p.write(0, value);
        }
        p = p.add(1);
        count -= 1;
    }
}
fn gpu_ui_fill_final(cb: Array<GpuBuffer>, start: u16, count: u16, value: u16) -> bool {
    let mut word = start;
    let mut left = count;
    while left != 0 {
        let p = gpu_builder_map(cb[0u16].objects.sector, word);
        if gpu_memory_faulted() {
            return false;
        }
        let mut n = 16384 - (word & 16383);
        if n > left {
            n = left;
        }
        gpu_ui_fill_words(p, n, value);
        word += n;
        left -= n;
    }
    true
}
fn gpu_builder_ui_begin_record(
    cb: Array<GpuBuffer>,
    atlas: Array<GpuUiAtlas>,
    fill: u16,
    first: u16,
    end: u16,
) -> bool {
    if !gpu_builder_open(cb)
        || first >= end
        || end > 750
        || !gpu_high_slot_valid(cb)
        || fill > 255
        || !gpu_ui_atlas_valid(atlas)
        || cb[0u16].object_words > 32368
        || !gpu_builder_room(cb, gpu_builder_ui_command_words(6), 6)
    {
        return false;
    }
    let word = (cb[0u16].object_words + 63) & 65472;
    if word > 32368 {
        return false;
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
    if first != 0 || end != 750 {
        let start = first >> 1;
        if !gpu_ui_fill_final(
            cb,
            word + start,
            ((end + 1) >> 1) - start,
            fill | (fill << 8),
        ) {
            return false;
        }
    } else if fill == 255 {
        if !gpu_ui_fill_final(cb, word, 384, 65535) {
            return false;
        }
    } else if !gpu_ui_fill_final(cb, word, 375, fill | (fill << 8))
        || !gpu_ui_fill_final(cb, word + 375, 9, 65535)
    {
        return false;
    }
    let p = gpu_builder_map(cb[0u16].objects.sector, word + 384);
    if gpu_memory_faulted() {
        return false;
    }
    unsafe {
        p.write(0, cb[0u16].objects.sector + (word >> 6));
        p.write(1, base >> 9);
    }
    gpu_zero(p.add(2), 14);
    GPU_UI_POSITION[0u16] = word;
    GPU_UI_POSITION[1u16] = first;
    GPU_UI_POSITION[2u16] = end;
    GPU_BUILD[1u16] = 4;
    true
}
fn gpu_builder_ui_write_record(
    cb: Array<GpuBuffer>,
    first: u16,
    ids: Array<u16>,
    count: u16,
) -> bool {
    if !gpu_builder_ui_active(cb)
        || first < GPU_UI_POSITION[1u16]
        || first > GPU_UI_POSITION[2u16]
        || count > GPU_UI_POSITION[2u16] - first
        || !gpu_window_low_valid(ids, count)
    {
        return false;
    }
    let mut i: u16 = 0;
    // Invalid spans/IDs leave the active object unchanged.
    while i < count {
        if ids[i] > 255 {
            return false;
        }
        i += 1;
    }
    let mut page: u16 = 65535;
    i = 0;
    while i < count {
        let cell = first + i;
        let word = GPU_UI_POSITION[0u16] + (cell >> 1);
        let next = (cb[0u16].objects.sector + (word >> 6)) >> 8;
        if page != next {
            gpu_builder_map(cb[0u16].objects.sector, word);
            if gpu_memory_faulted() {
                return false;
            }
            page = next;
        }
        let offset = ((cb[0u16].objects.sector & 255) << 6) + word;
        let p = Ptr::from_addr(0xc000 | (offset & 16383));
        let old = unsafe { p.read(0) };
        let mut value: u16 = 0;
        if cell & 1 == 0 {
            value = (old & 65280) | ids[i];
        } else {
            value = (old & 255) | (ids[i] << 8);
        }
        unsafe {
            p.write(0, value);
        }
        i += 1;
    }
    true
}
fn gpu_builder_ui_end_record(mut cb: Array<GpuBuffer>, draw: bool) -> bool {
    if !gpu_builder_ui_active(cb) {
        return false;
    }
    let word = GPU_UI_POSITION[0u16];
    if !gpu_builder_ui_commands(cb, word, GPU_UI_POSITION[1u16], GPU_UI_POSITION[2u16], draw) {
        return false;
    }
    cb[0u16].object_words = word + 400;
    cb[0u16].command_words = GPU_BUILD[5u16];
    GPU_BUILD[1u16] = 1;
    true
}
fn gpu_builder_ui_begin(
    cb: Array<GpuBuffer>,
    atlas: Array<GpuUiAtlas>,
    fill: u16,
    first: u16,
    end: u16,
) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_ui_begin_record(cb, atlas, fill, first, end);
    gpu_builder_restore(saved, result)
}
fn gpu_builder_ui_write(cb: Array<GpuBuffer>, first: u16, ids: Array<u16>, count: u16) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_ui_write_record(cb, first, ids, count);
    gpu_builder_restore(saved, result)
}
fn gpu_builder_ui_end(cb: Array<GpuBuffer>, draw: bool) -> bool {
    let saved = read_dseg_page(3);
    let result = gpu_builder_ui_end_record(cb, draw);
    gpu_builder_restore(saved, result)
}
