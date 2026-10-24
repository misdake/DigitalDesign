//! Diagnostic retained-scene experiment, not the production application API.
//! A trusted classic-scene builder creates the template once. Only prevalidated
//! pose/light words and three packed numeric-cell words change subsequently.
use crate::dsl_rt::*;
mod gpu;
use gpu::*;
mod boot_resources;
use boot_resources::*;

static EXP_COMMAND: Buf<u16, 256> = Buf::new([0; 256]);
static EXP_OBJECT: Buf<u16, 1024> = Buf::new([0; 1024]);
// Global, room Instance, RGB Instance, rabbit Instance, UI grid.
static EXP_OFFSET: Buf<u16, 5> = Buf::new([0; 5]);
static EXP_CB0: GpuBuffer = GpuBuffer {
    high_slot: 0,
    commands: GpuStorage {
        cpu: 0 as Ptr,
        sector: 0,
        sectors: 0,
    },
    objects: GpuStorage {
        cpu: 0 as Ptr,
        sector: 0,
        sectors: 0,
    },
    command_words: 0,
    object_words: 0,
    state: 0,
    color: 0,
    depth: 0,
};
static EXP_CB1: GpuBuffer = GpuBuffer {
    high_slot: 0,
    commands: GpuStorage {
        cpu: 0 as Ptr,
        sector: 0,
        sectors: 0,
    },
    objects: GpuStorage {
        cpu: 0 as Ptr,
        sector: 0,
        sectors: 0,
    },
    command_words: 0,
    object_words: 0,
    state: 0,
    color: 0,
    depth: 0,
};
fn exp_check(ok: bool) {
    if !ok {
        halt(0xe001);
    }
}
fn exp_buffer(slot: u16) -> Array<GpuBuffer> {
    if slot == 0 {
        view_of(&mut EXP_CB0)
    } else {
        view_of(&mut EXP_CB1)
    }
}
fn exp_note(index: u16, offset: u16) {
    EXP_OFFSET[index] = offset;
}

// Only CPU-owned CB/object inputs and immutable assets are admitted here.
// Physical cache tags and captured physical pending-store addresses permit
// remapping without per-copy flushes. Submission is still a blocking barrier.
fn exp_copy(low: Array<u16>, sector: u16, word: u16, count: u16, read: bool) {
    let offset = ((sector & 255) << 6) + word;
    exp_check(
        sector >= GPU_COMMAND_FIRST_SECTOR
            && word < 16384
            && offset < 16384
            && count <= 16384 - offset
            && gpu_window_low_valid(low, count),
    );
    let saved = read_dseg_page(3);
    let page = sector >> 8;
    unsafe {
        write_dseg_page(3, page);
    }
    exp_check(read_dseg_page(3) == page);
    let p = Ptr::from_addr(0xc000 | offset);
    if read {
        gpu_window_words(low.as_ptr(), p, count);
    } else {
        gpu_window_words(p, low.as_ptr(), count);
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    exp_check(read_dseg_page(3) == saved);
}

fn exp_capture() {
    exp_check(
        GPU_CB.state == GPU_CB_READY
            && GPU_CB.command_words <= 256
            && GPU_CB.object_words <= 1024
            && GPU_CB.object_words == EXP_OFFSET[4u16] + 400,
    );
    let padded = (GPU_CB.command_words + 63) & 65472;
    exp_check(gpu_window_copy(
        EXP_COMMAND.as_array(),
        GPU_CB.commands.sector,
        0,
        padded,
        true,
    ));
    exp_check(gpu_window_copy(
        EXP_OBJECT.as_array(),
        GPU_CB.objects.sector,
        0,
        GPU_CB.object_words,
        true,
    ));
    let mut slot: u16 = 0;
    while slot < 2 {
        let mut cb = exp_buffer(slot);
        cb[0u16] = GPU_CB;
        cb[0u16].high_slot = 0;
        cb[0u16].commands.sector += slot << 8;
        cb[0u16].objects.sector += slot << 8;
        cb[0u16].commands.sectors = 256;
        cb[0u16].objects.sectors = 256;
        cb[0u16].state = GPU_CB_BUILDING;
        slot += 1;
    }
    // All DMA_LOCAL sources in this qualified scene are in the first 32 KiB
    // of its object arena. Moving them +0x8000 sets source bit15 (code bit20).
    let mut i: u16 = 0;
    while i < GPU_CB.command_words {
        let h = EXP_COMMAND[i + 1];
        if h >> 12 == 1 {
            exp_check((h & 16) == 0 && ((h >> 5) & 127) == 74);
            EXP_COMMAND[i + 1] = h + 16;
        }
        i += 2;
    }
    let ui = EXP_OFFSET[4u16] + 384;
    EXP_OBJECT[ui] += 256;
    exp_copy(
        EXP_COMMAND.as_array(),
        GPU_COMMAND_FIRST_SECTOR + 256,
        0,
        padded,
        false,
    );
    exp_copy(
        EXP_OBJECT.as_array(),
        GPU_OBJECT_FIRST_SECTOR + 256,
        0,
        GPU_CB.object_words,
        false,
    );
    exp_check(gpu_discard_ready(GPU_CB_HANDLE));
}

fn exp_prepare(slot: u16, phase: u16) {
    exp_check(slot < 2 && phase & 255 == 0);
    let mut cb = exp_buffer(slot);
    exp_check(cb[0u16].state == GPU_CB_BUILDING || cb[0u16].state == GPU_CB_COMPLETED);
    dev_send(7, 8, slot);
    exp_copy(
        MATRIX.as_array(),
        DEMO_CAMERA_FIRST + (phase >> 8),
        0,
        45,
        true,
    );
    let saved = read_dseg_page(3);
    let page = cb[0u16].objects.sector >> 8;
    unsafe {
        write_dseg_page(3, page);
    }
    exp_check(read_dseg_page(3) == page);
    let p = Ptr::from_addr(0xc000);
    let light = EXP_OFFSET[0u16] + 2;
    unsafe {
        p.write(light as i16, MATRIX[42u16]);
        p.write((light + 1) as i16, MATRIX[43u16]);
        p.write((light + 2) as i16, MATRIX[44u16]);
    }
    gpu_window_words(p.add(EXP_OFFSET[1u16] as i16), MATRIX.as_ptr(), 42);
    gpu_window_words(p.add(EXP_OFFSET[2u16] as i16), MATRIX.as_ptr(), 42);
    gpu_window_words(p.add(EXP_OFFSET[3u16] as i16), MATRIX.as_ptr(), 42);
    gpu_window_words(
        p.add((EXP_OFFSET[4u16] + 26) as i16),
        CELLS.as_ptr().add(26),
        3,
    );
    unsafe {
        write_dseg_page(3, saved);
    }
    exp_check(read_dseg_page(3) == saved);
    cb[0u16].state = GPU_CB_READY;
    dev_send(7, 9, slot);
}

fn exp_submit(slot: u16) {
    let mut cb = exp_buffer(slot);
    exp_check(
        cb[0u16].state == GPU_CB_READY
            && GPU_DRIVER.as_array()[0u16] == 1
            && dev_recv(GPU_DEVICE, GPU_STATUS) == 0,
    );
    exp_check(gpu_display_reserve() == cb[0u16].color);
    exp_check(gpu_display_render_start(cb[0u16].color));
    exp_check(dcache_invalidate_all() == 0);
    cb[0u16].state = GPU_CB_INFLIGHT;
    GPU_DRIVER.as_array()[0u16] = 2;
    GPU_DRIVER.as_array()[1u16] = cb.as_ptr().addr();
    dev_send(7, 10, slot);
    dev_send(
        GPU_DEVICE,
        GPU_COMMAND_COUNT,
        (cb[0u16].command_words >> 1) - 1,
    );
    dev_send(GPU_DEVICE, GPU_COMMAND_START, cb[0u16].commands.sector);
}
fn exp_wait_discard(slot: u16) {
    let cb = exp_buffer(slot);
    let mut left: u16 = 60000;
    let mut result = GPU_PENDING;
    while result == GPU_PENDING && left != 0 {
        result = gpu_driver_poll(cb);
        left -= 1;
    }
    exp_check(result == GPU_COMPLETED);
    exp_check(gpu_display_render_done(cb[0u16].color));
    dev_send(7, 11, slot);
    // Offscreen throughput experiment: keep Front A, reuse only completed B.
    // Production presentation must additionally await actual Pending/Front swap.
    exp_check(gpu_display_discard(cb[0u16].color));
}
fn exp_retained_prefix() {
    exp_capture();
    // Finish one-time cloning before measuring the next frame's preparation.
    exp_check(dcache_invalidate_all() == 0);
    let now = time_milliseconds();
    // Keep the same previous-time fixture as the full-recording comparison.
    STATS.as_array()[0u16] = now - 17;
    STATS.as_array()[2u16] = 1;
    dev_send(7, 1, 1);
    exp_check(demo_buttons_update(
        BUTTONS.as_array(),
        dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_BUTTONS),
        now,
    ));
    demo_overlay_begin(CELLS.as_array(), STATS.as_array(), now, true);
    demo_report(1, 1, STATS.as_array()[1u16]);
    exp_prepare(0, 256);
    dev_send(7, 1, 12);
    exp_submit(0);
    dev_send(7, 1, 13);
    halt(0x600d);
}
fn exp_pingpong() {
    exp_capture();
    exp_prepare(0, 0);
    exp_submit(0);
    exp_prepare(1, 256);
    exp_wait_discard(0);
    exp_submit(1);
    exp_prepare(0, 512);
    exp_wait_discard(1);
    exp_check(dcache_invalidate_all() == 0);
    halt(0x600d);
}

fn exp_wait_present(slot: u16) {
    let cb = exp_buffer(slot);
    let mut left: u16 = 60000;
    let mut result = GPU_PENDING;
    while result == GPU_PENDING && left != 0 {
        result = gpu_driver_poll(cb);
        left -= 1;
    }
    exp_check(result == GPU_COMPLETED);
    exp_check(gpu_display_render_done(cb[0u16].color));
    dev_send(7, 11, slot);
    exp_check(gpu_display_present(cb[0u16].color) == GPU_PENDING);
    left = 60000;
    result = GPU_PENDING;
    while result == GPU_PENDING && left != 0 {
        result = gpu_display_refresh();
        left -= 1;
    }
    exp_check(result == GPU_COMPLETED);
    dev_send(7, 12, slot);
}
fn exp_pingpong_present() {
    exp_capture();
    let mut second = exp_buffer(1);
    second[0u16].color = GPU_COLOR_A_SECTOR;
    EXP_OBJECT[0u16] = (GPU_COLOR_A_SECTOR >> 2) | (GPU_DEPTH_SECTOR << 12);
    exp_copy(
        EXP_OBJECT.as_array(),
        second[0u16].objects.sector,
        0,
        1,
        false,
    );
    exp_prepare(0, 0);
    exp_submit(0);
    exp_prepare(1, 256);
    exp_wait_present(0);
    exp_submit(1);
    exp_prepare(0, 512);
    // Two real renders and one actual swap suffice; omit a second scan wait.
    exp_wait_discard(1);
    exp_check(dcache_invalidate_all() == 0);
    halt(0x600d);
}
