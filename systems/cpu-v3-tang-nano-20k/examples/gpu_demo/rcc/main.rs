//! One CB, complete 3D -> character layer -> GPU drain -> actual A/B present.
use crate::dsl_rt::*;
mod gpu;
use gpu::*;
mod controls;
use controls::*;
mod camera;
use camera::*;
mod overlay_gpu;
use overlay_gpu::*;
mod time;
use time::*;
mod boot_resources;
use boot_resources::*;

static BUTTONS: Buf<u16, 4> = Buf::new([0; 4]);
static MATRIX: Buf<u16, 45> = Buf::new([0; 45]);
static STATS: Buf<u16, 4> = Buf::new([0; 4]);

// DDGD, version, status, completed count, previous full-loop ms, XOR checksum.
fn demo_report(status: u16, frame: u16, ms: u16) {
    let mut bytes: Buf<u16, 11> = Buf::new([68, 68, 71, 68, 1, 0, 0, 0, 0, 0, 0]);
    bytes[5u16] = status;
    bytes[6u16] = frame & 255;
    bytes[7u16] = frame >> 8;
    bytes[8u16] = ms & 255;
    bytes[9u16] = ms >> 8;
    let mut i: u16 = 4;
    let mut sum: u16 = 0;
    while i < 10 {
        sum ^= bytes[i];
        i += 1;
    }
    bytes[10u16] = sum;
    i = 0;
    while i < 11 {
        while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 {}
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, bytes[i]);
        i += 1;
    }
}
fn demo_fail(code: u16) {
    frame_stats_reset(STATS.as_array());
    dev_send(7, 7, GPU_CB.state);
    demo_report(128 | code, 0, 0);
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 32 | code);
    halt(0xd000 | code);
}
fn demo_require(ok: bool, code: u16) {
    if !ok {
        demo_fail(code);
    }
}
fn demo_wait(cb: u16, present: bool, budget: u16) -> u16 {
    let begin = time_milliseconds();
    let mut status = GPU_PENDING;
    while status == GPU_PENDING {
        status = if present {
            gpu_present_poll()
        } else {
            gpu_poll(cb)
        };
        if status == GPU_PENDING && time_elapsed_ms(begin, time_milliseconds()) >= budget {
            return GPU_TIMEOUT;
        }
    }
    status
}
fn demo_run(limit: u16, budget: u16) {
    demo_ui_reset(STATS.as_array());
    demo_require(gpu_init_with_format(DISPLAY_LINEAR_RGB565) == GPU_IDLE, 1);
    let mut mesh: GpuMesh = GpuMesh {
        directory_sector: DEMO_MESH_DIRECTORY,
        directory_word: 0,
        count: DEMO_ROOM_COUNT,
        first_sector: DEMO_MESH_DIRECTORY,
        sectors: DEMO_MESH_SECTORS,
    };
    let mut texture: GpuTexture = GpuTexture {
        sector: DEMO_TEXTURE,
        sectors: DEMO_TEXTURE_SECTORS,
        size_log2: 6,
        full_mip: 1,
    };
    let mut atlas: GpuUiAtlas = GpuUiAtlas {
        sector: DEMO_ATLAS,
        sectors: 512,
    };
    let mut global: GpuGlobal = GpuGlobal {
        far_low: 0,
        far_high: 199,
        light_x: 0,
        light_y: 0,
        light_z: 16384,
        ray_x: -8192,
        ray_y: -4915,
        ray_k: 8192,
        ambient: 64,
        directional: 192,
        mode: 3,
        shininess: 12,
        epoch: 0,
    };
    let mut render: GpuRender = GpuRender {
        texture_sector: 0,
        texture_sectors: 0,
        size_log2: 0,
        full_mip: 0,
        filter: 2,
        specular_low: 41120,
        specular_high: 160,
        depth_func: 1,
        depth_write: 1,
        blend: 0,
        unlit: 1,
        lod_bias: 0,
    };
    let mut frame: u16 = 0;
    let mut phase: u16 = 0;
    let mut completed = false;
    demo_buttons_init(
        BUTTONS.as_array(),
        dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_BUTTONS),
        time_milliseconds(),
    );
    while limit == 0 || frame < limit {
        let now = time_milliseconds();
        dev_send(7, 5, now);
        let show_numbers = demo_buttons_update(
            BUTTONS.as_array(),
            dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_BUTTONS),
            now,
        );
        demo_ui_frame_begin(STATS.as_array(), now, completed);
        demo_report(if completed { 1 } else { 0 }, frame, STATS.as_array()[1u16]);
        completed = false;
        demo_require(demo_camera(MATRIX.as_array(), phase), 14);
        global.light_x = MATRIX[42u16] as i16;
        global.light_y = MATRIX[43u16] as i16;
        global.light_z = MATRIX[44u16] as i16;
        dev_send(7, 6, phase);
        let cb = gpu_cb_begin();
        demo_require(cb == GPU_CB_HANDLE, 2);
        let pass = gpu_pass_begin(cb, 0, 65535);
        demo_require(pass == GPU_CB_HANDLE, 3);
        demo_require(gpu_pass_set_global(pass, view_of(&mut global)), 4);
        demo_require(
            gpu_pass_set_render(pass, view_of(&mut texture), view_of(&mut render)),
            5,
        );
        demo_require(
            gpu_pass_draw_mesh(pass, view_of(&mut mesh), MATRIX.as_array(), 65535, 65535),
            6,
        );
        mesh.directory_word = DEMO_RGB_WORD;
        mesh.count = 1;
        demo_require(
            gpu_pass_draw_mesh(pass, view_of(&mut mesh), MATRIX.as_array(), 65535, 65535),
            6,
        );
        render.unlit = 0;
        demo_require(
            gpu_pass_set_render(pass, view_of(&mut texture), view_of(&mut render)),
            5,
        );
        mesh.directory_word = DEMO_BUNNY_WORD;
        mesh.count = DEMO_BUNNY_COUNT;
        demo_require(
            gpu_pass_draw_mesh(pass, view_of(&mut mesh), MATRIX.as_array(), 65535, 65535),
            6,
        );
        mesh.directory_word = 0;
        mesh.count = DEMO_ROOM_COUNT;
        render.unlit = 1;
        if show_numbers {
            demo_require(
                demo_ui_record(pass, view_of(&mut atlas), STATS.as_array()),
                7,
            );
        }
        demo_require(gpu_pass_end(pass) && gpu_cb_finish(cb), 8);
        demo_require(gpu_submit_idle(cb) == GPU_PENDING, 9);
        demo_require(demo_wait(cb, false, budget) == GPU_COMPLETED, 10);
        dev_send(7, 0, frame + 1);
        demo_require(gpu_present(cb) == GPU_PENDING, 11);
        demo_require(demo_wait(cb, true, budget) == GPU_COMPLETED, 12);
        dev_send(7, 2, frame + 1);
        completed = true;
        frame += 1;
        phase += 256;
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, frame & 31);
    }
    // Bounded verification waits for the last complete visible scan as well.
    let scan = dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX);
    let start = time_milliseconds();
    while dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX) == scan {
        demo_require(time_elapsed_ms(start, time_milliseconds()) < 1000, 13);
    }
    halt(0x600d);
}
fn main() {
    demo_run(0, 2000);
}
// Same frame body; only stopping/budget policy differs in bounded verification.
pub fn demo_native_main() {
    demo_run(2, 2000);
}
pub fn demo_one_frame_main() {
    demo_run(1, 2000);
}
