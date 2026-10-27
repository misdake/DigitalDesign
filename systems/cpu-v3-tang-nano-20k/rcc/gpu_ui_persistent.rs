//! Internal persistent canvas. Edits never touch referenced GPU input pages.
use crate::dsl_rt::*;
mod gpu_ui;
use gpu_ui::*;
mod gpu_window;
use gpu_window::*;
mod gpu_layout;
use gpu_layout::*;

static GPU_PUI_DESIRED: Buf<u16, 375> = Buf::new([0; 375]);
// One 375-bit mask per page; the final group's upper nine bits stay zero.
static GPU_PUI_DIRTY: Buf<u16, 48> = Buf::new([0; 48]);
// Per page: reference (0=free,1=recording,2=recorded,3=inflight),
// dirty word count, content version, initialized.
static GPU_PUI_PAGES: Buf<u16, 8> = Buf::new([0; 8]);
// Registered, desired content version, immutable atlas sector/extent.
static GPU_PUI_CANVAS: Buf<u16, 4> = Buf::new([0; 4]);

fn gpu_persistent_ui_live() -> bool {
    GPU_PIPE[0u16] == 1 && !gpu_memory_faulted()
}
fn gpu_persistent_ui_page(slot: u16) -> u16 {
    if slot == 1 {
        return GPU_UI_PAGE0_SECTOR;
    }
    GPU_UI_PAGE1_SECTOR
}
fn gpu_persistent_ui_reference(slot: u16) -> u16 {
    GPU_PUI_PAGES[(slot - 1) * 4]
}
fn gpu_persistent_ui_register(atlas: Array<GpuUiAtlas>) -> bool {
    if !gpu_persistent_ui_live() || !gpu_ui_atlas_valid(atlas) {
        return false;
    }
    let mut c = GPU_PUI_CANVAS.as_array();
    if c[0u16] != 0 {
        return c[2u16] == atlas[0u16].sector && c[3u16] == atlas[0u16].sectors;
    }
    if GPU_PIPE[1u16] != 0 || GPU_PIPE[2u16] != 0 || GPU_PIPE[3u16] != 0 {
        return false;
    }
    if !gpu_ui_fill(GPU_PUI_DESIRED.as_array(), 255) {
        return false;
    }
    let mut i: u16 = 0;
    while i < 24 {
        let mut mask: u16 = 65535;
        if i == 23 {
            mask = 127;
        }
        GPU_PUI_DIRTY[i] = mask;
        GPU_PUI_DIRTY[24 + i] = mask;
        i += 1;
    }
    i = 0;
    while i < 8 {
        GPU_PUI_PAGES[i] = 0;
        i += 1;
    }
    GPU_PUI_PAGES[1u16] = 375;
    GPU_PUI_PAGES[5u16] = 375;
    c[1u16] = 0;
    c[2u16] = atlas[0u16].sector;
    c[3u16] = atlas[0u16].sectors;
    c[0u16] = 1;
    true
}
fn gpu_persistent_ui_word(word: u16, value: u16) {
    if GPU_PUI_DESIRED[word] == value {
        return;
    }
    GPU_PUI_DESIRED[word] = value;
    GPU_PUI_CANVAS[1u16] += 1;
    let mask: u16 = 1 << (word & 15);
    let group = word >> 4;
    let mut page: u16 = 0;
    while page < 2 {
        let at = page * 24 + group;
        if GPU_PUI_DIRTY[at] & mask == 0 {
            GPU_PUI_DIRTY[at] |= mask;
            GPU_PUI_PAGES[page * 4 + 1] += 1;
        }
        page += 1;
    }
}
fn gpu_persistent_ui_set(cell: u16, id: u16) -> bool {
    if !gpu_persistent_ui_live() || GPU_PUI_CANVAS[0u16] == 0 || cell >= 750 || id > 255 {
        return false;
    }
    let word = cell >> 1;
    let mut value = GPU_PUI_DESIRED[word];
    if cell & 1 == 0 {
        value = (value & 65280) | id;
    } else {
        value = (value & 255) | (id << 8);
    }
    gpu_persistent_ui_word(word, value);
    true
}
// Replace a complete field, erasing its old tail automatically. IDs are ordinary
// unpacked glyph values; a hidden field does not read its source.
fn gpu_persistent_ui_field(
    first: u16,
    width: u16,
    ids: Array<u16>,
    count: u16,
    visible: bool,
) -> bool {
    if !gpu_persistent_ui_live()
        || GPU_PUI_CANVAS[0u16] == 0
        || first > 750
        || width > 750 - first
        || count > width
    {
        return false;
    }
    let mut i: u16 = 0;
    if visible && count != 0 {
        if !gpu_window_low_valid(ids, count) {
            return false;
        }
        while i < count {
            if ids[i] > 255 {
                return false;
            }
            i += 1;
        }
    }
    i = 0;
    while i < width {
        let mut id: u16 = 255;
        if visible && i < count {
            id = ids[i];
        }
        gpu_persistent_ui_set(first + i, id);
        i += 1;
    }
    true
}
// Copy only dirty words into a CPU-owned page. Desired edits are serialized
// with this call. A sealed/inflight page cannot enter this mapping scope.
fn gpu_persistent_ui_prepare(slot: u16) -> bool {
    if !gpu_persistent_ui_live() || slot == 0 || slot > 2 || GPU_PIPE[1u16] != slot {
        return false;
    }
    let at = (slot - 1) * 4;
    let mut m = GPU_PUI_PAGES.as_array();
    if m[at] == 0 {
        return true;
    }
    if m[at] != 1 || GPU_PUI_CANVAS[0u16] == 0 {
        return false;
    }
    if m[at + 1] == 0 && m[at + 3] != 0 {
        return true;
    }
    let saved = read_dseg_page(3);
    let page = gpu_persistent_ui_page(slot);
    let dst = gpu_builder_map(page, 0);
    if !gpu_memory_faulted() {
        let mut group: u16 = 0;
        while group < 24 {
            let mut mask = GPU_PUI_DIRTY[(slot - 1) * 24 + group];
            let mut word = group * 16;
            while mask != 0 {
                if mask & 1 != 0 {
                    unsafe {
                        dst.write(word as i16, GPU_PUI_DESIRED[word]);
                    }
                }
                mask = mask >> 1;
                word += 1;
            }
            group += 1;
        }
        if m[at + 3] == 0 {
            let mut word: u16 = 375;
            while word < GPU_UI_DESCRIPTOR_WORD {
                unsafe {
                    dst.write(word as i16, 65535);
                }
                word += 1;
            }
            gpu_zero(dst.add((GPU_UI_DESCRIPTOR_WORD + 2) as i16), 14);
            gpu_store32(
                dst.add(GPU_UI_DESCRIPTOR_WORD as i16),
                page,
                GPU_PUI_CANVAS[2u16] >> 9,
            );
        }
    }
    if !gpu_builder_restore(saved, !gpu_memory_faulted()) {
        return false;
    }
    // Only after successful restoration is this page's dirty history retired.
    let mut group: u16 = 0;
    while group < 24 {
        GPU_PUI_DIRTY[(slot - 1) * 24 + group] = 0;
        group += 1;
    }
    m[at + 1] = 0;
    m[at + 2] = GPU_PUI_CANVAS[1u16];
    m[at + 3] = 1;
    true
}
fn gpu_persistent_ui_sealed(slot: u16) {
    let at = (slot - 1) * 4;
    if GPU_PUI_PAGES[at] == 1 {
        GPU_PUI_PAGES[at] = 2;
    }
}
fn gpu_persistent_ui_ready(slot: u16) -> bool {
    let at = (slot - 1) * 4;
    GPU_PUI_PAGES[at] == 0 || (GPU_PUI_PAGES[at] == 2 && GPU_PUI_PAGES[at + 3] == 1)
}
fn gpu_persistent_ui_submitted(slot: u16) {
    let at = (slot - 1) * 4;
    if GPU_PUI_PAGES[at] == 2 {
        GPU_PUI_PAGES[at] = 3;
    }
}
fn gpu_persistent_ui_retire(slot: u16) -> bool {
    let at = (slot - 1) * 4;
    if GPU_PUI_PAGES[at] == 0 {
        return true;
    }
    if GPU_PUI_PAGES[at] != 3 {
        return false;
    }
    GPU_PUI_PAGES[at] = 0;
    true
}
fn gpu_persistent_ui_cancel(slot: u16) -> bool {
    let at = (slot - 1) * 4;
    if GPU_PUI_PAGES[at] > 1 {
        return false;
    }
    GPU_PUI_PAGES[at] = 0;
    true
}
