//! Private published-input lease. The consumer receives only bounded read bridges.
use crate::dsl_rt::*;
mod gpu_layout;
use gpu_layout::*;
mod gpu_capture_abi;
use gpu_capture_abi::*;
mod gpu_capture_config;
use gpu_capture_config::*;

static GPU_CAPTURE_HEADER: Buf<u16, 64> = Buf::new([0; 64]);
static GPU_CAPTURE_RANGES: Buf<u16, 512> = Buf::new([0; 512]);
static GPU_CAPTURE_CACHE: Buf<u16, 64> = Buf::new([0; 64]);
// live,error,token lo/hi,submitted lo/hi,cache range+1,offset lo/hi,count,
// owner pointer,range count,input slot1/2.
static GPU_CAPTURE_CONTROL: Buf<u16, 13> = Buf::new([0; 13]);

fn gpu_capture_fail(code: u16) -> u16 {
    if GPU_CAPTURE_CONTROL[1u16] == 0 {
        GPU_CAPTURE_CONTROL[1u16] = code;
    }
    GPU_DRIVER[0u16] = 3;
    gpu_display_fault();
    GPU_CAPTURE_CONTROL[1u16]
}
fn gpu_capture_pair(at: u16, low: u16, high: u16) {
    GPU_CAPTURE_HEADER[at] = low;
    GPU_CAPTURE_HEADER[at + 1] = high;
}
fn gpu_capture_base(at: u16, sector: u16) {
    gpu_capture_pair(at, sector << 7, sector >> 9);
}
fn gpu_capture_range_add(tag: u16, flags: u16, sector: u16, low: u16, high: u16) -> bool {
    if (tag == GPU_CAPTURE_TAG_MESH || tag == GPU_CAPTURE_TAG_TEXTURE || tag == GPU_CAPTURE_TAG_ATLAS)
        && !gpu_capture_manifest_asset(tag, sector, low, high) {
        gpu_capture_fail(GPU_CAPTURE_IDENTITY_ERROR);
        return false;
    }
    let count = GPU_CAPTURE_CONTROL[11u16];
    let base = sector << 7;
    let end = base + low;
    let mut end_high = (sector >> 9) + high;
    if end < base {
        end_high += 1;
    }
    if count >= GPU_CAPTURE_MAX_RANGES
        || sector < GPU_DOMAIN_FIRST_SECTOR
        || (low == 0 && high == 0)
        || low & 1 != 0
        || high > 64
        || end_high > 128
        || (end_high == 128 && end != 0)
    {
        gpu_capture_fail(GPU_CAPTURE_CAPACITY);
        return false;
    }
    let at = count * GPU_CAPTURE_RANGE_WORDS;
    GPU_CAPTURE_RANGES[at] = flags;
    GPU_CAPTURE_RANGES[at + 1] = tag;
    GPU_CAPTURE_RANGES[at + 2] = base;
    GPU_CAPTURE_RANGES[at + 3] = sector >> 9;
    GPU_CAPTURE_RANGES[at + 4] = low;
    GPU_CAPTURE_RANGES[at + 5] = high;
    GPU_CAPTURE_RANGES[at + 6] = 0;
    GPU_CAPTURE_RANGES[at + 7] = 0;
    if tag <= GPU_CAPTURE_TAG_UI {
        GPU_CAPTURE_RANGES[at + 6] = GPU_CAPTURE_CONTROL[2u16];
        GPU_CAPTURE_RANGES[at + 7] = GPU_CAPTURE_CONTROL[3u16];
    }
    GPU_CAPTURE_CONTROL[11u16] = count + 1;
    true
}
fn gpu_capture_health() -> u16 {
    if GPU_CAPTURE_CONTROL[0u16] == 0 {
        return GPU_CAPTURE_NO_LEASE;
    }
    if GPU_CAPTURE_CONTROL[1u16] != 0 {
        return GPU_CAPTURE_CONTROL[1u16];
    }
    if gpu_memory_faulted() {
        return gpu_capture_fail(GPU_CAPTURE_MEMORY);
    }
    let cb = gpu_pipeline_buffer(GPU_CAPTURE_CONTROL[12u16]);
    if cb.as_ptr().addr() != GPU_CAPTURE_CONTROL[10u16]
        || gpu_driver_owned_buffer().addr() != cb.as_ptr().addr()
        || gpu_driver_poll(cb) != GPU_PREPARED
    {
        return gpu_capture_fail(GPU_CAPTURE_GPU_ERROR);
    }
    let status = gpu_display_refresh();
    let front = gpu_display_sector(GPU_DISPLAY[1u16]);
    if (status != GPU_IDLE && status != GPU_COMPLETED)
        || GPU_DISPLAY[2u16] != 0
        || front << 7 != GPU_CAPTURE_HEADER[GPU_CAPTURE_H_FRONT_BASE]
        || front >> 9 != GPU_CAPTURE_HEADER[GPU_CAPTURE_H_FRONT_BASE + 1]
        || GPU_DISPLAY[6u16] != GPU_CAPTURE_HEADER[GPU_CAPTURE_H_FRONT_FORMAT]
    {
        return gpu_capture_fail(GPU_CAPTURE_DISPLAY_ERROR);
    }
    GPU_CAPTURE_OK
}
fn gpu_capture_begin(cb: Array<GpuBuffer>, slot: u16, frame: u16) -> bool {
    if GPU_CAPTURE_CONFIG_ENABLED == 0 || GPU_CAPTURE_CONTROL[0u16] != 0 || slot == 0 || slot > 2 {
        return false;
    }
    if GPU_CAPTURE_CONTROL[1u16] != 0 {
        return false;
    }
    if (GPU_CAPTURE_CONTROL[2u16] == 65535 && GPU_CAPTURE_CONTROL[3u16] == 65535)
        || (GPU_CAPTURE_CONTROL[4u16] == 65535 && GPU_CAPTURE_CONTROL[5u16] == 65535)
    {
        gpu_capture_fail(GPU_CAPTURE_CAPACITY);
        return false;
    }
    GPU_CAPTURE_CONTROL[2u16] += 1;
    if GPU_CAPTURE_CONTROL[2u16] == 0 {
        GPU_CAPTURE_CONTROL[3u16] += 1;
    }
    gpu_zero(GPU_CAPTURE_HEADER.as_ptr(), GPU_CAPTURE_WORDS);
    GPU_CAPTURE_CONTROL[0u16] = 1;
    GPU_CAPTURE_CONTROL[6u16] = 0;
    GPU_CAPTURE_CONTROL[10u16] = cb.as_ptr().addr();
    GPU_CAPTURE_CONTROL[11u16] = 0;
    GPU_CAPTURE_CONTROL[12u16] = slot;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_VERSION] = GPU_CAPTURE_VERSION;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_WORDS] = GPU_CAPTURE_WORDS;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_FLAGS] = GPU_CAPTURE_PUBLISHED;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_BINDINGS] = GPU_CAPTURE_BINDINGS_SELF_CONTAINED;
    gpu_capture_pair(
        GPU_CAPTURE_H_TOKEN,
        GPU_CAPTURE_CONTROL[2u16],
        GPU_CAPTURE_CONTROL[3u16],
    );
    let sequence = GPU_CAPTURE_CONTROL[4u16] + 1;
    let mut sequence_high = GPU_CAPTURE_CONTROL[5u16];
    if sequence == 0 {
        sequence_high += 1;
    }
    gpu_capture_pair(GPU_CAPTURE_H_SUBMISSION, sequence, sequence_high);
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_FRAME] = frame;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_SLOT] = slot - 1;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_COUNT] = GPU_DRIVER_SUBMISSION.count_minus_one;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_PUBLICATION_LOW] = GPU_CAPTURE_CONTROL[2u16];
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_PUBLICATION_HIGH] = GPU_CAPTURE_CONTROL[3u16];
    gpu_capture_base(GPU_CAPTURE_H_COMMAND_BASE, cb[0u16].commands.sector);
    gpu_capture_pair(
        GPU_CAPTURE_H_COMMAND_BYTES,
        cb[0u16].command_words << 1,
        cb[0u16].command_words >> 15,
    );
    let padded = (cb[0u16].command_words + 63) & 65472;
    gpu_capture_pair(
        GPU_CAPTURE_H_COMMAND_PADDED_BYTES,
        padded << 1,
        padded >> 15,
    );
    gpu_capture_base(GPU_CAPTURE_H_OBJECT_BASE, cb[0u16].objects.sector);
    gpu_capture_pair(
        GPU_CAPTURE_H_OBJECT_BYTES,
        cb[0u16].object_words << 1,
        cb[0u16].object_words >> 15,
    );
    gpu_capture_base(GPU_CAPTURE_H_COLOR_BASE, cb[0u16].color);
    gpu_capture_base(GPU_CAPTURE_H_DEPTH_BASE, cb[0u16].depth);
    gpu_capture_base(
        GPU_CAPTURE_H_FRONT_BASE,
        gpu_display_sector(GPU_DISPLAY[1u16]),
    );
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_FRONT_FORMAT] = GPU_DISPLAY[6u16];
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_TARGET_FORMAT] = GPU_DISPLAY[7u16];
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_RANGE_WORDS] = GPU_CAPTURE_RANGE_WORDS;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_RANGE_VERSION] = GPU_CAPTURE_RANGE_VERSION;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_LAYOUT_ABI] = GPU_CAPTURE_CONFIG_LAYOUT_ABI;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_COMMAND_ABI] = GPU_CAPTURE_NATIVE_COMMAND_ABI;
    gpu_capture_pair(GPU_CAPTURE_H_PLANE_BYTES, GPU_FRAMEBUFFER_PHYSICAL_SECTORS << 7, GPU_FRAMEBUFFER_PHYSICAL_SECTORS >> 9);
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_MANIFEST_WORDS] = GPU_CAPTURE_MANIFEST_WORDS;
    let mut i: u16 = 0;
    while i < GPU_CAPTURE_MANIFEST_WORDS {
        GPU_CAPTURE_HEADER[GPU_CAPTURE_H_MANIFEST + i] = gpu_capture_manifest_word(i);
        i += 1;
    }
    if !gpu_capture_range_add(
        GPU_CAPTURE_TAG_COMMANDS,
        GPU_CAPTURE_READ,
        cb[0u16].commands.sector,
        padded << 1,
        padded >> 15,
    ) || !gpu_capture_range_add(
        GPU_CAPTURE_TAG_OBJECTS,
        GPU_CAPTURE_READ,
        cb[0u16].objects.sector,
        cb[0u16].object_words << 1,
        cb[0u16].object_words >> 15,
    ) {
        return false;
    }
    let reference = gpu_persistent_ui_reference(slot);
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_UI_REFERENCE] = reference;
    if reference != 0 {
        if reference != 2 {
            gpu_capture_fail(GPU_CAPTURE_GPU_ERROR);
            return false;
        }
        let ui = gpu_persistent_ui_page(slot);
        gpu_capture_base(GPU_CAPTURE_H_UI_BASE, ui);
        gpu_capture_pair(GPU_CAPTURE_H_UI_BYTES, 1024, 0);
        if !gpu_capture_range_add(GPU_CAPTURE_TAG_UI, GPU_CAPTURE_READ, ui, 1024, 0) {
            return false;
        }
    }
    if !gpu_capture_range_add(
        GPU_CAPTURE_TAG_MESH,
        GPU_CAPTURE_READ | GPU_CAPTURE_IMMUTABLE,
        GPU_RENDERER_MESH_FIRST,
        GPU_RENDERER_MESH_SECTORS << 7,
        GPU_RENDERER_MESH_SECTORS >> 9,
    ) || !gpu_capture_range_add(
        GPU_CAPTURE_TAG_TEXTURE,
        GPU_CAPTURE_READ | GPU_CAPTURE_IMMUTABLE,
        GPU_RENDERER_TEXTURE,
        GPU_RENDERER_TEXTURE_SECTORS << 7,
        GPU_RENDERER_TEXTURE_SECTORS >> 9,
    ) || !gpu_capture_range_add(
        GPU_CAPTURE_TAG_ATLAS,
        GPU_CAPTURE_READ | GPU_CAPTURE_IMMUTABLE,
        GPU_PUI_CANVAS[2u16],
        GPU_PUI_CANVAS[3u16] << 7,
        GPU_PUI_CANVAS[3u16] >> 9,
    ) || !gpu_capture_range_add(
        GPU_CAPTURE_TAG_COLOR,
        GPU_CAPTURE_WRITE | GPU_CAPTURE_PREIMAGE | GPU_CAPTURE_FULL_CLEAR,
        cb[0u16].color,
        GPU_FRAMEBUFFER_PHYSICAL_SECTORS << 7,
        GPU_FRAMEBUFFER_PHYSICAL_SECTORS >> 9,
    ) || !gpu_capture_range_add(
        GPU_CAPTURE_TAG_DEPTH,
        GPU_CAPTURE_WRITE | GPU_CAPTURE_PREIMAGE | GPU_CAPTURE_FULL_CLEAR,
        cb[0u16].depth,
        GPU_FRAMEBUFFER_PHYSICAL_SECTORS << 7,
        GPU_FRAMEBUFFER_PHYSICAL_SECTORS >> 9,
    ) || !gpu_capture_range_add(
        GPU_CAPTURE_TAG_FRONT,
        GPU_CAPTURE_READ | GPU_CAPTURE_PREIMAGE,
        gpu_display_sector(GPU_DISPLAY[1u16]),
        GPU_FRAMEBUFFER_PHYSICAL_SECTORS << 7,
        GPU_FRAMEBUFFER_PHYSICAL_SECTORS >> 9,
    ) {
        return false;
    }
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_RANGE_COUNT] = GPU_CAPTURE_CONTROL[11u16];
    return gpu_capture_health() == GPU_CAPTURE_OK;
}
fn gpu_capture_range_value(range: u16, field: u16) -> u16 {
    if gpu_capture_health() != GPU_CAPTURE_OK {
        return 0;
    }
    if range >= GPU_CAPTURE_CONTROL[11u16] || field >= GPU_CAPTURE_RANGE_WORDS {
        gpu_capture_fail(GPU_CAPTURE_BOUNDS);
        return 0;
    }
    GPU_CAPTURE_RANGES[range * GPU_CAPTURE_RANGE_WORDS + field]
}
fn gpu_capture_read_value(range: u16, offset_low: u16, offset_high: u16) -> u16 {
    if gpu_capture_health() != GPU_CAPTURE_OK {
        return 0;
    }
    if range >= GPU_CAPTURE_CONTROL[11u16] {
        gpu_capture_fail(GPU_CAPTURE_BOUNDS);
        return 0;
    }
    let at = range * GPU_CAPTURE_RANGE_WORDS;
    let bytes_low = GPU_CAPTURE_RANGES[at + GPU_CAPTURE_R_BYTES];
    let bytes_high = GPU_CAPTURE_RANGES[at + GPU_CAPTURE_R_BYTES + 1];
    let words_low = (bytes_low >> 1) | (bytes_high << 15);
    let words_high = bytes_high >> 1;
    if offset_high > words_high || (offset_high == words_high && offset_low >= words_low) {
        gpu_capture_fail(GPU_CAPTURE_BOUNDS);
        return 0;
    }
    let chunk = offset_low & 65472;
    if GPU_CAPTURE_CONTROL[6u16] != range + 1
        || GPU_CAPTURE_CONTROL[7u16] != chunk
        || GPU_CAPTURE_CONTROL[8u16] != offset_high
    {
        let base_low = GPU_CAPTURE_RANGES[at + GPU_CAPTURE_R_BASE];
        let base_high = GPU_CAPTURE_RANGES[at + GPU_CAPTURE_R_BASE + 1];
        let base_word = (base_low >> 1) | (base_high << 15);
        let physical_word = base_word + chunk;
        let mut physical_high = (base_high >> 1) + offset_high;
        if physical_word < base_word {
            physical_high += 1;
        }
        let remaining = words_low - chunk;
        let mut remaining_high = words_high - offset_high;
        if words_low < chunk {
            remaining_high -= 1;
        }
        let mut count: u16 = 64;
        if remaining_high == 0 && remaining < count {
            count = remaining;
        }
        let mut parts: GpuWindowParts = GpuWindowParts {
            first: GPU_CAPTURE_CACHE.as_ptr(),
            count: count,
            tail: GPU_CAPTURE_CACHE.as_ptr(),
            tail_count: 0,
        };
        // The lease freezes every source, including Display-readable Front.
        // Cache tags are physical; no source is written, and resume republishes.
        if !gpu_window_parts_mode(
            view_of(&mut parts),
            (physical_high << 10) | (physical_word >> 6),
            physical_word & 63,
            true,
            false,
        ) {
            gpu_capture_fail(GPU_CAPTURE_MEMORY);
            return 0;
        }
        GPU_CAPTURE_CONTROL[6u16] = range + 1;
        GPU_CAPTURE_CONTROL[7u16] = chunk;
        GPU_CAPTURE_CONTROL[8u16] = offset_high;
        GPU_CAPTURE_CONTROL[9u16] = count;
    }
    GPU_CAPTURE_CACHE[offset_low & 63]
}
fn gpu_capture_submitted() {
    GPU_CAPTURE_CONTROL[4u16] = GPU_CAPTURE_HEADER[GPU_CAPTURE_H_SUBMISSION];
    GPU_CAPTURE_CONTROL[5u16] = GPU_CAPTURE_HEADER[GPU_CAPTURE_H_SUBMISSION + 1];
    GPU_CAPTURE_CONTROL[0u16] = 0;
    GPU_CAPTURE_CONTROL[6u16] = 0;
}
