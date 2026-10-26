#![rcc_resource_library(
    gpu_device,
    gpu_command_buffer,
    gpu_builder,
    gpu_display,
    gpu_encode,
    gpu_memory,
    gpu_layout,
    gpu_assets,
    gpu_window,
    gpu_ui,
    gpu_ui_persistent,
    gpu_scene
)]
//! Resource facade over the single native two-input scheduler.
//! One recorder, one sealed successor, one GPU execution; no execution queue.
use crate::dsl_rt::*;
mod boot_resources;
use boot_resources::*;
mod gpu_device;
use gpu_device::*;
mod gpu_command_buffer;
use gpu_command_buffer::*;
mod gpu_builder;
use gpu_builder::*;
mod gpu_display;
use gpu_display::*;
// Explicit flat helper domain; only this root exposes the resource facade.
mod gpu_encode;
use gpu_encode::*;
mod gpu_memory;
use gpu_memory::*;
mod gpu_layout;
use gpu_layout::*;
mod gpu_assets;
use gpu_assets::*;
mod gpu_window;
use gpu_window::*;
mod gpu_ui;
use gpu_ui::*;
mod gpu_ui_persistent;
use gpu_ui_persistent::*;
mod gpu_scene;
use gpu_scene::*;
mod gamepad;
use gamepad::*;
mod frame_stats;
use frame_stats::*;

static GPU_PIPE_A: GpuBuffer = GpuBuffer {
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
static GPU_PIPE_B: GpuBuffer = GpuBuffer {
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
// phase(0=new,1=active,3=frozen), recorder, recorded, GPU, next slot, sequence.
// Stored slot identities are index+1; zero is none. They never identify colors.
static GPU_PIPE: Buf<u16, 6> = Buf::new([0; 6]);
// First failure: code, sequence, slot, valid command words, sampled GPU status.
static GPU_PIPE_FAULT: Buf<u16, 5> = Buf::new([0; 5]);

fn gpu_pipeline_buffer(slot: u16) -> Array<GpuBuffer> {
    if slot == 1 {
        return view_of(&mut GPU_PIPE_A);
    }
    view_of(&mut GPU_PIPE_B)
}
fn gpu_pipeline_fail(code: u16) -> u16 {
    let mut p = GPU_PIPE.as_array();
    if p[0u16] != 3 {
        let mut f = GPU_PIPE_FAULT.as_array();
        f[0u16] = code;
        f[1u16] = p[5u16];
        let mut slot = p[3u16];
        if slot == 0 {
            slot = p[2u16];
        }
        if slot == 0 {
            slot = p[1u16];
        }
        f[2u16] = slot;
        if slot != 0 {
            f[3u16] = gpu_pipeline_buffer(slot)[0u16].command_words;
        }
        f[4u16] = dev_recv(GPU_DEVICE, GPU_STATUS);
        p[0u16] = 3;
        GPU_DRIVER.as_array()[0u16] = 3;
        gpu_display_fault();
    }
    GPU_FAULT
}
fn gpu_pipeline_init(format: u16) -> u16 {
    let mut p = GPU_PIPE.as_array();
    if p[0u16] != 0 || GPU_DRIVER.as_array()[0u16] != 0 || format > DISPLAY_LINEAR_RGB565 {
        return GPU_REJECTED;
    }
    let display = gpu_display_init_format(format);
    if display != GPU_IDLE && display != GPU_PENDING {
        return gpu_pipeline_fail(1);
    }
    let driver = gpu_driver_init();
    if driver != GPU_IDLE && driver != GPU_PENDING {
        return gpu_pipeline_fail(2);
    }
    if !gpu_high_slot_init(view_of(&mut GPU_PIPE_A), 0)
        || !gpu_high_slot_init(view_of(&mut GPU_PIPE_B), 1)
    {
        return gpu_pipeline_fail(3);
    }
    p[4u16] = 1;
    p[0u16] = 1;
    if display == GPU_PENDING || driver == GPU_PENDING {
        return GPU_PENDING;
    }
    GPU_IDLE
}
// Polling never submits a queued successor. Successful GPU terminal releases
// its input slot; present is automatic, with independent Display retirement.
fn gpu_pipeline_poll() -> u16 {
    let mut p = GPU_PIPE.as_array();
    if p[0u16] == 0 {
        return GPU_NOT_INITIALIZED;
    }
    if p[0u16] == 3 {
        return GPU_FAULT;
    }
    if gpu_memory_faulted() {
        return gpu_pipeline_fail(4);
    }
    let slot = p[3u16];
    let mut owned = gpu_pipeline_buffer(1);
    if slot != 0 {
        owned = gpu_pipeline_buffer(slot);
    }
    let result = gpu_driver_poll(owned);
    if result == GPU_FAULT || result == GPU_REJECTED {
        return gpu_pipeline_fail(5);
    }
    if slot != 0 && result == GPU_COMPLETED {
        let mut observed_us: u16 = 0;
        let mut observed_ms: u16 = 0;
        if GPU_RENDERER[0u16] == 1 {
            observed_us = dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MICROSECONDS);
            observed_ms = dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MILLISECONDS);
        }
        if !gpu_display_render_done(owned[0u16].color)
            || gpu_display_present(owned[0u16].color) != GPU_PENDING
        {
            return gpu_pipeline_fail(6);
        }
        if !gpu_persistent_ui_retire(slot) {
            return gpu_pipeline_fail(15);
        }
        renderer_observed(slot, observed_us, observed_ms);
        p[3u16] = 0;
    }
    let display = gpu_display_refresh();
    if display == GPU_FAULT {
        return gpu_pipeline_fail(7);
    }
    if p[3u16] != 0 || GPU_DRIVER.as_array()[0u16] == 4 || display == GPU_PENDING {
        return GPU_PENDING;
    }
    display
}
// Recording does not reserve a plane. The placeholder Target is private input;
// its builder-generated relocation is replaced before final cache publication.
fn gpu_pipeline_begin(color_value: u16, depth_value: u16) -> u16 {
    let observed = gpu_pipeline_poll();
    let mut p = GPU_PIPE.as_array();
    if observed == GPU_FAULT
        || observed == GPU_NOT_INITIALIZED
        || p[1u16] != 0
        || p[2u16] != 0
        || GPU_DRIVER.as_array()[0u16] == 4
    {
        return 0;
    }
    let slot = p[4u16];
    let mut cb = gpu_pipeline_buffer(slot);
    if cb[0u16].state == GPU_CB_COMPLETED && !gpu_buffer_reset(cb) {
        return 0;
    }
    if cb[0u16].state != GPU_CB_BUILDING || cb[0u16].command_words != 0 {
        return 0;
    }
    if gpu_persistent_ui_reference(slot) != 0 {
        gpu_pipeline_fail(14);
        return 0;
    }
    p[1u16] = slot;
    if !gpu_builder_begin(
        cb,
        GPU_COLOR_A_SECTOR,
        GPU_DEPTH_SECTOR,
        color_value,
        depth_value,
    ) {
        if gpu_memory_faulted() {
            gpu_pipeline_fail(8);
        } else {
            p[1u16] = 0;
        }
        return 0;
    }
    slot
}
fn gpu_pipeline_seal() -> bool {
    let mut p = GPU_PIPE.as_array();
    if p[0u16] != 1 || p[1u16] == 0 || p[2u16] != 0 {
        return false;
    }
    let cb = gpu_pipeline_buffer(p[1u16]);
    if !gpu_persistent_ui_prepare(p[1u16]) {
        gpu_pipeline_fail(13);
        return false;
    }
    if !gpu_builder_end(cb) || !gpu_builder_finish(cb) {
        if gpu_memory_faulted() {
            gpu_pipeline_fail(9);
        }
        return false;
    }
    gpu_persistent_ui_sealed(p[1u16]);
    p[2u16] = p[1u16];
    p[1u16] = 0;
    true
}
fn gpu_pipeline_cancel() -> bool {
    let mut p = GPU_PIPE.as_array();
    if p[0u16] != 1 || p[1u16] == 0 || gpu_memory_faulted() {
        return false;
    }
    let mut cb = gpu_pipeline_buffer(p[1u16]);
    if cb[0u16].state != GPU_CB_BUILDING {
        return false;
    }
    if !gpu_persistent_ui_cancel(p[1u16]) {
        gpu_pipeline_fail(14);
        return false;
    }
    cb[0u16].command_words = 0;
    cb[0u16].object_words = 0;
    GPU_SLOT_TARGETS.as_array()[p[1u16] - 1] = 0;
    GPU_BUILD.as_array()[0u16] = 0;
    GPU_BUILD.as_array()[1u16] = 0;
    GPU_BUILD.as_array()[2u16] = 0;
    p[1u16] = 0;
    true
}
fn gpu_pipeline_advance() -> u16 {
    let result = gpu_pipeline_poll();
    let mut p = GPU_PIPE.as_array();
    if result == GPU_FAULT || result == GPU_NOT_INITIALIZED {
        return result;
    }
    if p[3u16] != 0 {
        return GPU_PENDING;
    }
    if p[2u16] == 0 {
        return result;
    }
    if !gpu_persistent_ui_ready(p[2u16]) {
        return gpu_pipeline_fail(14);
    }
    let target = gpu_display_reserve();
    if target == 0 {
        if GPU_DISPLAY.as_array()[0u16] == 3 {
            return gpu_pipeline_fail(10);
        }
        return GPU_PENDING;
    }
    let slot = p[2u16];
    let cb = gpu_pipeline_buffer(slot);
    if !gpu_slot_bind_target(cb, target) || !gpu_display_render_start(target) {
        return gpu_pipeline_fail(11);
    }
    // This final driver fence drains old scalar/FST stores and every relocation.
    // Neither SEND occurs on a reported maintenance failure.
    renderer_submitting(slot);
    if gpu_driver_submit_idle(cb) != GPU_PENDING {
        return gpu_pipeline_fail(12);
    }
    gpu_persistent_ui_submitted(slot);
    p[2u16] = 0;
    p[3u16] = slot;
    p[4u16] = 3 - slot;
    p[5u16] += 1;
    GPU_PENDING
}
// All persistent ranges in this frame use the same page. Its content is chosen
// only when sealing; later desired edits belong to a future frame.
fn gpu_pipeline_ui_range(first: u16, end: u16) -> bool {
    if !gpu_persistent_ui_live() || GPU_PIPE[1u16] == 0 || first > end || end > 750 {
        return false;
    }
    if first == end {
        return true;
    }
    if GPU_PUI_CANVAS[0u16] == 0 {
        return false;
    }
    let slot = GPU_PIPE[1u16];
    let reference = gpu_persistent_ui_reference(slot);
    if reference > 1 {
        gpu_pipeline_fail(14);
        return false;
    }
    let descriptor = gpu_persistent_ui_page(slot) + (GPU_UI_DESCRIPTOR_WORD >> 6);
    let cb = gpu_pipeline_buffer(slot);
    let result = gpu_builder_ui_page(cb, descriptor, first, end, reference == 0);
    if result || gpu_memory_faulted() {
        GPU_PUI_PAGES[(slot - 1) * 4] = 1;
    }
    if gpu_memory_faulted() {
        gpu_pipeline_fail(13);
    }
    result
}

#[rcc_resource(cleanup = renderer_cleanup)]
pub struct Renderer {
    word: u16,
}
#[rcc_resource(cleanup = frame_cleanup, parent = Renderer)]
pub struct Frame {
    word: u16,
}
#[rcc_resource(cleanup = pass_cleanup, parent = Frame)]
pub struct Pass {
    word: u16,
}
// 0 new,1 Renderer,2 Frame,3 Pass,4 closed,5 frozen; pass 0 new/2 ended/3 cancelled.
static GPU_RENDERER: Buf<u16, 3> = Buf::new([0; 3]);
// Two fixed timestamp records: sequence, record us/epoch/ms, sealed ms,
// prepared us/valid, submit us/epoch/ms, submitted, reserved.
static GPU_RENDERER_TIMES: Buf<u16, 24> = Buf::new([0; 24]);
// Own clear epoch, logical frame sequence, previous tick ms, started.
static GPU_RENDERER_CLOCK: Buf<u16, 4> = Buf::new([0; 4]);
// Available, sequence, prep valid/us, observed valid/us, queue ms, span ms.
static GPU_RENDERER_LAST: Buf<u16, 8> = Buf::new([0; 8]);
static GPU_RENDERER_TICK: Buf<u16, 2> = Buf::new([0; 2]);
static GPU_RENDERER_INPUT: GamepadFrame = GamepadFrame {
    connected: 0,
    id: 0,
    buttons: 0,
    pressed: 0,
    released: 0,
    left_x: fix32::zero(),
    left_y: fix32::zero(),
    right_x: fix32::zero(),
    right_y: fix32::zero(),
};
pub struct FrameStats {
    pub ready: bool,
    pub sequence: u16,
    pub prepare_valid: bool,
    pub prepare_us: u16,
    pub observed_valid: bool,
    pub submit_to_observed_done_us: u16,
    pub queue_ms: u16,
    pub span_ms: u16,
}
// TextField owns CPU metadata only. GPU page references belong to the scheduler.
#[rcc_resource(cleanup=text_cleanup)]
pub struct TextField {
    word: u16,
}
// used, first cell, width, visible, logical glyph count, desired is current.
static GPU_RENDERER_FIELDS: Buf<u16, 24> = Buf::new([0; 24]);
static GPU_RENDERER_GLYPHS: Buf<u16, 200> = Buf::new([0; 200]);
fn renderer_field(slot: u16) -> Array<u16> {
    unsafe {
        GPU_RENDERER_FIELDS
            .as_ptr()
            .add(((slot - 1) * 6) as i16)
            .as_u16_array()
    }
}
fn renderer_glyphs(slot: u16) -> Array<u16> {
    unsafe {
        GPU_RENDERER_GLYPHS
            .as_ptr()
            .add(((slot - 1) * 50) as i16)
            .as_u16_array()
    }
}
fn renderer_field_apply(slot: u16) -> bool {
    let mut f = renderer_field(slot);
    if !gpu_persistent_ui_field(
        f[1u16],
        f[2u16],
        renderer_glyphs(slot),
        f[4u16],
        f[3u16] != 0,
    ) {
        return false;
    }
    f[5u16] = 1;
    true
}
fn renderer_field_write(slot: u16, ids: Array<u16>, count: u16) -> bool {
    let mut f = renderer_field(slot);
    if count > f[2u16] {
        return false;
    }
    let mut glyphs = renderer_glyphs(slot);
    let mut changed = f[4u16] != count;
    let mut i: u16 = 0;
    while i < count {
        if glyphs[i] != ids[i] {
            changed = true;
        }
        i += 1;
    }
    if changed {
        i = 0;
        while i < count {
            glyphs[i] = ids[i];
            i += 1;
        }
        f[4u16] = count;
        if f[3u16] != 0 {
            f[5u16] = 0;
        }
    }
    if f[5u16] == 0 {
        return renderer_field_apply(slot);
    }
    true
}
pub fn renderer_text(r: &mut Renderer, column: u16, row: u16, width: u16) -> TextField {
    if column >= 50 || row >= 15 || width == 0 || width > 50 - column {
        renderer_fault(51);
    }
    let first = row * 50 + column;
    let end = first + width;
    let mut free: u16 = 0;
    let mut slot: u16 = 1;
    while slot <= 4 {
        let f = renderer_field(slot);
        if f[0u16] == 0 {
            if free == 0 {
                free = slot;
            }
        } else {
            if first < f[1u16] + f[2u16] && f[1u16] < end {
                renderer_fault(52);
            }
        }
        slot += 1;
    }
    if free == 0 {
        renderer_fault(53);
    }
    let mut f = renderer_field(free);
    f[0u16] = 1;
    f[1u16] = first;
    f[2u16] = width;
    f[3u16] = 1;
    f[4u16] = 0;
    f[5u16] = 0;
    if !renderer_field_apply(free) {
        renderer_fault(54);
    }
    TextField { word: free }
}
fn text_cleanup(field: TextField) {
    renderer_field(field.word)[0u16] = 0;
}
pub fn text_visible(field: &mut TextField, visible: bool) {
    let mut f = renderer_field(field.word);
    let mut v: u16 = 0;
    if visible {
        v = 1;
    }
    if f[3u16] != v {
        f[3u16] = v;
        f[5u16] = 0;
        if !renderer_field_apply(field.word) {
            renderer_fault(55);
        }
    }
}
fn renderer_write_number(slot: u16, number: u16) -> bool {
    let mut digits: Buf<u16, 5> = Buf::new([0; 5]);
    let mut count: u16 = 1;
    let mut n = number;
    while n >= 10 {
        count += 1;
        n = n / 10;
    }
    let mut i = count;
    n = number;
    while i != 0 {
        i -= 1;
        digits[i] = 48 + n % 10;
        n = n / 10;
    }
    renderer_field_write(slot, digits.as_array(), count)
}
pub fn pass_observed_time(mut p: &mut Pass, mut field: &mut TextField) {
    if GPU_RENDERER[0u16] != 3 || renderer_field(field.word)[2u16] < 8 {
        renderer_fault(49);
    }
    let last = GPU_RENDERER_LAST.as_array();
    let mut state: Buf<u16, 4> = Buf::new([0; 4]);
    state[1u16] = last[5u16];
    if last[0u16] != 0 && last[4u16] != 0 {
        state[3u16] = 1;
    }
    let mut digits: Buf<u16, 4> = Buf::new([0; 4]);
    frame_stats_tenths_digits(state.as_array(), digits.as_array());
    let mut text: Buf<u16, 8> = Buf::new([79, 66, 83, 32, 255, 255, 255, 255]);
    let mut first: u16 = 0;
    if state[3u16] != 0 && digits[0u16] == 48 {
        first = 1;
    }
    let mut i = first;
    while i < 4 {
        text[4 + i - first] = digits[i];
        i += 1;
    }
    if !renderer_field_write(field.word, text.as_array(), 8 - first) {
        renderer_fault(50);
    }
    pass_text(&mut p, &mut field);
}
fn renderer_timer_clear() {
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_MICROSECONDS, 1);
    GPU_RENDERER_CLOCK[0u16] += 1;
}
fn renderer_time(slot: u16) -> Array<u16> {
    unsafe {
        GPU_RENDERER_TIMES
            .as_ptr()
            .add(((slot - 1) * 12) as i16)
            .as_u16_array()
    }
}
fn renderer_recording(slot: u16) {
    let mut t = renderer_time(slot);
    let now = dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MILLISECONDS);
    GPU_RENDERER_CLOCK[1u16] += 1;
    t[0u16] = GPU_RENDERER_CLOCK[1u16];
    t[1u16] = dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MICROSECONDS);
    t[2u16] = GPU_RENDERER_CLOCK[0u16];
    t[3u16] = now;
    t[10u16] = 0;
    GPU_RENDERER_TICK[0u16] = now;
    GPU_RENDERER_TICK[1u16] = 0;
    if GPU_RENDERER_CLOCK[3u16] != 0 {
        GPU_RENDERER_TICK[1u16] = now - GPU_RENDERER_CLOCK[2u16];
    }
    GPU_RENDERER_CLOCK[2u16] = now;
    GPU_RENDERER_CLOCK[3u16] = 1;
    gamepad_capture();
    GPU_RENDERER_INPUT = gamepad_take_frame();
}
fn renderer_sealed(slot: u16) {
    let mut t = renderer_time(slot);
    let end = dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MICROSECONDS);
    t[4u16] = dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MILLISECONDS);
    t[5u16] = end - t[1u16];
    t[6u16] = 0;
    if t[1u16] != 65535 && end != 65535 && end >= t[1u16] && t[2u16] == GPU_RENDERER_CLOCK[0u16] {
        t[6u16] = 1;
    }
}
fn renderer_submitting(slot: u16) {
    if GPU_RENDERER[0u16] != 1 {
        return;
    }
    let mut t = renderer_time(slot);
    t[7u16] = dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MICROSECONDS);
    t[8u16] = GPU_RENDERER_CLOCK[0u16];
    t[9u16] = dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MILLISECONDS);
    t[10u16] = 1;
}
fn renderer_observed(slot: u16, observed_us: u16, observed_ms: u16) {
    if GPU_RENDERER[0u16] != 1 {
        return;
    }
    let mut t = renderer_time(slot);
    let mut last = GPU_RENDERER_LAST.as_array();
    last[0u16] = 1;
    last[1u16] = t[0u16];
    last[2u16] = t[6u16];
    last[3u16] = t[5u16];
    last[4u16] = 0;
    last[5u16] = observed_us - t[7u16];
    if t[10u16] != 0
        && t[7u16] != 65535
        && observed_us != 65535
        && observed_us >= t[7u16]
        && t[8u16] == GPU_RENDERER_CLOCK[0u16]
    {
        last[4u16] = 1;
    }
    last[6u16] = t[9u16] - t[4u16];
    last[7u16] = observed_ms - t[3u16];
    t[10u16] = 0;
    // Observation happens only after a recording has sealed/cancelled. Thus
    // an already finalized successor's prep sample survives this new epoch.
    renderer_timer_clear();
}
pub fn frame_previous(f: &mut Frame) -> FrameStats {
    let s = GPU_RENDERER_LAST.as_array();
    FrameStats {
        ready: s[0u16] != 0,
        sequence: s[1u16],
        prepare_valid: s[2u16] != 0,
        prepare_us: s[3u16],
        observed_valid: s[4u16] != 0,
        submit_to_observed_done_us: s[5u16],
        queue_ms: s[6u16],
        span_ms: s[7u16],
    }
}
pub fn frame_input(f: &mut Frame) -> GamepadFrame {
    GPU_RENDERER_INPUT
}
pub fn frame_tick_ms(f: &mut Frame) -> u16 {
    GPU_RENDERER_TICK[0u16]
}
pub fn frame_elapsed_ms(f: &mut Frame) -> u16 {
    GPU_RENDERER_TICK[1u16]
}
pub fn frame_elapsed_seconds(f: &mut Frame) -> fix32 {
    // Scale the fractional raw value rather than narrowing 1/1000 to Q16.
    fix32::from_words(GPU_RENDERER_TICK[1u16], 0) * fix32::from_words(35127, 65)
}

#[rcc_resource(cleanup=camera_cleanup)]
pub struct Camera {
    word: u16,
}
#[rcc_resource(cleanup=model_cleanup)]
pub struct Model {
    word: u16,
}
#[rcc_resource(cleanup=mesh_cleanup)]
pub struct Mesh {
    word: u16,
}
#[rcc_resource(cleanup=material_cleanup)]
pub struct Material {
    word: u16,
}
fn mesh_cleanup(m: Mesh) {
    scene_mesh_release(m.word);
}
fn material_cleanup(m: Material) {
    scene_material_release(m.word);
}
pub fn renderer_mesh(r: &mut Renderer) -> Mesh {
    let mut root: GpuMesh = GpuMesh {
        directory_sector: GPU_RENDERER_MESH_DIRECTORY,
        directory_word: GPU_RENDERER_MESH_DIRECTORY_WORD,
        count: GPU_RENDERER_MESH_COUNT,
        first_sector: GPU_RENDERER_MESH_FIRST,
        sectors: GPU_RENDERER_MESH_SECTORS,
    };
    let minimum = vec3::new(
        fix32::from_words(GPU_RENDERER_MIN_X_LOW, GPU_RENDERER_MIN_X_HIGH),
        fix32::from_words(GPU_RENDERER_MIN_Y_LOW, GPU_RENDERER_MIN_Y_HIGH),
        fix32::from_words(GPU_RENDERER_MIN_Z_LOW, GPU_RENDERER_MIN_Z_HIGH),
    );
    let maximum = vec3::new(
        fix32::from_words(GPU_RENDERER_MAX_X_LOW, GPU_RENDERER_MAX_X_HIGH),
        fix32::from_words(GPU_RENDERER_MAX_Y_LOW, GPU_RENDERER_MAX_Y_HIGH),
        fix32::from_words(GPU_RENDERER_MAX_Z_LOW, GPU_RENDERER_MAX_Z_HIGH),
    );
    let slot = scene_mesh_new(view_of(&mut root), minimum, maximum);
    if slot == 0 {
        renderer_fault(42);
    }
    Mesh { word: slot }
}
pub fn renderer_material(r: &mut Renderer, red: u16, green: u16, blue: u16, lit: bool) -> Material {
    let slot = scene_material_new(red, green, blue, lit);
    if slot == 0 {
        renderer_fault(43);
    }
    Material { word: slot }
}
pub fn material_color(m: &mut Material, red: u16, green: u16, blue: u16, alpha: u16) {
    if !scene_material_color(m.word, red, green, blue, alpha) {
        renderer_fault(44);
    }
}
pub fn material_lighting(m: &mut Material, ambient: u16, directional: u16, shininess: u16) {
    if !scene_material_lighting(m.word, ambient, directional, shininess) {
        renderer_fault(45);
    }
}
pub fn material_specular(m: &mut Material, red: u16, green: u16, blue: u16) {
    if !scene_material_specular(m.word, red, green, blue) {
        renderer_fault(46);
    }
}
pub fn pass_draw(p: &mut Pass, mesh: &mut Mesh, model: &mut Model, material: &mut Material) {
    if GPU_RENDERER[0u16] != 3
        || GPU_RENDERER[2u16] != 1
        || !scene_model_prepare(model.word, scene_mesh_bounds(mesh.word))
    {
        renderer_fault(47);
    }
    let cb = gpu_pipeline_buffer(p.word);
    let mut global = scene_global(material.word);
    let mut render = scene_render(material.word);
    let mut texture = scene_texture();
    let mut root = scene_mesh_value(mesh.word);
    let color = scene_material(material.word);
    if !gpu_builder_global(cb, view_of(&mut global))
        || !gpu_builder_render(cb, view_of(&mut texture), view_of(&mut render))
        || !gpu_builder_draw_known(
            cb,
            view_of(&mut root),
            scene_matrix(model.word),
            color[13u16],
            color[14u16],
        )
    {
        renderer_fault(48);
    }
}
fn camera_cleanup(c: Camera) {
    scene_camera_release(c.word);
}
fn model_cleanup(m: Model) {
    scene_model_release(m.word);
}
pub fn renderer_camera(
    r: &mut Renderer,
    eye: vec3,
    angles: vec3,
    focal: fix32,
    far: fix32,
) -> Camera {
    let slot = scene_camera_new(eye, angles, focal, far);
    if slot == 0 {
        renderer_fault(35);
    }
    Camera { word: slot }
}
pub fn camera_pose(c: &mut Camera, eye: vec3, angles: vec3) {
    if !scene_camera_pose(c.word, eye, angles) {
        renderer_fault(36);
    }
}
pub fn camera_projection(c: &mut Camera, focal: fix32, far: fix32) {
    if !scene_camera_projection(c.word, focal, far) {
        renderer_fault(37);
    }
}
pub fn camera_light(c: &mut Camera, direction: vec3) {
    if !scene_camera_light(c.word, direction) {
        renderer_fault(38);
    }
}
pub fn renderer_model(r: &mut Renderer, position: vec3, angles: vec3, scale: vec3) -> Model {
    let slot = scene_model_new(position, angles, scale);
    if slot == 0 {
        renderer_fault(39);
    }
    Model { word: slot }
}
pub fn model_pose(m: &mut Model, position: vec3, angles: vec3, scale: vec3) {
    if !scene_model_pose(m.word, position, angles, scale) {
        renderer_fault(40);
    }
}
fn renderer_pass_ready() -> bool {
    GPU_RENDERER[0u16] == 2 && (GPU_RENDERER[1u16] == 0 || GPU_RENDERER[1u16] == 2)
}
pub fn frame_pass(f: &mut Frame, c: &mut Camera) -> Pass {
    if !renderer_pass_ready() || !scene_view_begin(c.word) {
        renderer_fault(41);
    }
    GPU_RENDERER[0u16] = 3;
    GPU_RENDERER[2u16] = 1;
    Pass { word: f.word }
}

fn renderer_fault(code: u16) -> ! {
    GPU_RENDERER[0u16] = 5;
    gpu_pipeline_fail(code);
    halt(0xe000 + code);
}
pub fn renderer_open() -> Renderer {
    if GPU_RENDERER[0u16] != 0 || GPU_PIPE[0u16] != 0 {
        renderer_fault(34);
    }
    let status = gpu_pipeline_init(DISPLAY_LINEAR_RGB565);
    if status != GPU_IDLE && status != GPU_PENDING {
        renderer_fault(20);
    }
    let mut atlas: GpuUiAtlas = GpuUiAtlas {
        sector: (GPU_RENDERER_ATLAS_BYTE_ADDRESS_HIGH << 9)
            | (GPU_RENDERER_ATLAS_BYTE_ADDRESS_LOW >> 7),
        sectors: 512,
    };
    if !gpu_persistent_ui_register(view_of(&mut atlas)) {
        renderer_fault(21);
    }
    GPU_RENDERER[0u16] = 1;
    gamepad_init();
    renderer_timer_clear();
    Renderer { word: 1 }
}
pub fn renderer_frame(r: &mut Renderer, color: u16) -> Frame {
    if GPU_RENDERER[0u16] != 1 {
        renderer_fault(22);
    }
    let mut left: u16 = 60000;
    while GPU_PIPE[2u16] != 0 || GPU_DRIVER[0u16] == 4 {
        gamepad_capture();
        if left == 0 || gpu_pipeline_advance() == GPU_FAULT {
            renderer_fault(23);
        }
        left -= 1;
    }
    let slot = gpu_pipeline_begin(color, 65535);
    if slot == 0 {
        renderer_fault(24);
    }
    GPU_RENDERER[0u16] = 2;
    GPU_RENDERER[1u16] = 0;
    renderer_recording(slot);
    Frame { word: slot }
}
// One short field; the game supplies neither packed words nor transparency.
pub fn frame_number(f: &mut Frame, field: &mut TextField, number: u16) {
    if GPU_RENDERER[0u16] != 2 {
        renderer_fault(25);
    }
    if !renderer_write_number(field.word, number) {
        renderer_fault(26);
    }
}
pub fn frame_ui_pass(f: &mut Frame) -> Pass {
    if !renderer_pass_ready() {
        renderer_fault(27);
    }
    GPU_RENDERER[0u16] = 3;
    GPU_RENDERER[2u16] = 0;
    Pass { word: f.word }
}
pub fn pass_text(p: &mut Pass, field: &mut TextField) {
    if GPU_RENDERER[0u16] != 3 {
        renderer_fault(28);
    }
    let f = renderer_field(field.word);
    if f[3u16] == 0 {
        return;
    }
    if !gpu_pipeline_ui_range(f[1u16], f[1u16] + f[2u16]) {
        renderer_fault(28);
    }
}
pub fn pass_complete(p: Pass) {
    GPU_RENDERER[0u16] = 2;
    GPU_RENDERER[1u16] = 2;
    resource_disarm(p);
}
fn pass_cleanup(p: Pass) {
    GPU_RENDERER[0u16] = 2;
    GPU_RENDERER[1u16] = 3;
}
fn frame_cleanup(f: Frame) {
    if GPU_RENDERER[0u16] == 2 {
        if !gpu_pipeline_cancel() {
            renderer_fault(29);
        }
        GPU_RENDERER[0u16] = 1;
        renderer_time(f.word)[10u16] = 0;
    }
}
pub fn frame_complete(f: Frame) {
    if GPU_RENDERER[1u16] != 2 {
        frame_cleanup(f);
        return;
    }
    if !gpu_pipeline_seal() {
        gpu_pipeline_fail(30);
        GPU_RENDERER[0u16] = 5;
        resource_disarm(f);
        halt(0xe01e);
    }
    renderer_sealed(f.word);
    // Scheduler owns the sealed page now, even if it must wait for Display.
    GPU_RENDERER[0u16] = 1;
    resource_disarm(f);
    if gpu_pipeline_advance() == GPU_FAULT {
        renderer_fault(31);
    }
}
fn renderer_cleanup(r: Renderer) {
    // No poll/advance or wait: asynchronous work remains frozen in place.
    if GPU_PIPE[2u16] != 0 || GPU_PIPE[3u16] != 0 || GPU_DISPLAY[2u16] != 0 {
        GPU_RENDERER[0u16] = 5;
        gpu_pipeline_fail(32);
    } else {
        GPU_RENDERER[0u16] = 4;
    }
}
pub fn renderer_close(r: Renderer) {
    let mut left: u16 = 60000;
    loop {
        gamepad_capture();
        let status = gpu_pipeline_advance();
        if status == GPU_FAULT || left == 0 {
            GPU_RENDERER[0u16] = 5;
            gpu_pipeline_fail(33);
            resource_disarm(r);
            halt(0xe021);
        }
        if GPU_PIPE[2u16] == 0 && GPU_PIPE[3u16] == 0 && GPU_DISPLAY[2u16] == 0 {
            break;
        }
        left -= 1;
    }
    GPU_RENDERER[0u16] = 4;
    resource_disarm(r);
}
