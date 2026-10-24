//! One CB, complete 3D -> character layer -> GPU drain -> actual A/B present.
use crate::dsl_rt::*;
mod gpu;
use gpu::*;
mod dualshock2;
use dualshock2::*;
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

static INPUT: Buf<u16, 10> = Buf::new([0; 10]);
static PACKET: Buf<u16, 9> = Buf::new([0; 9]);
static CAMERA: Buf<u16, 12> = Buf::new([0; 12]);
static BUTTONS: Buf<u16, 4> = Buf::new([0; 4]);
static MATRIX: Buf<u16, 45> = Buf::new([0; 45]);
static STATS: Buf<u16, 4> = Buf::new([0; 4]);

// One previous completed report, streamed only while GPU/present is pending.
// A slow UART never stalls input collection or command construction.
static REPORT: Buf<u16, 12> = Buf::new([0; 12]);
fn demo_report_queue(status: u16, frame: u16, us: u16) {
    REPORT[0u16] = 68;
    REPORT[1u16] = 68;
    REPORT[2u16] = 71;
    REPORT[3u16] = 68;
    REPORT[4u16] = 3;
    REPORT[5u16] = status;
    REPORT[6u16] = frame & 255;
    REPORT[7u16] = frame >> 8;
    REPORT[8u16] = us & 255;
    REPORT[9u16] = us >> 8;
    let mut i: u16 = 4;
    let mut sum: u16 = 0;
    while i < 10 {
        sum ^= REPORT[i];
        i += 1;
    }
    REPORT[10u16] = sum;
    REPORT[11u16] = 0;
}
fn demo_report_step() {
    let i = REPORT[11u16];
    if i < 11 && dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 == 0 {
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, REPORT[i]);
        REPORT[11u16] = i + 1;
    }
}
fn demo_report_drain() {
    while REPORT[11u16] < 11 {
        demo_report_step();
    }
}
fn demo_fail(code: u16) {
    frame_stats_reset(STATS.as_array());
    dev_send(7, 7, GPU_CB.state);
    demo_report_drain();
    demo_report_queue(128 | code, 0, 0);
    demo_report_drain();
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
        if status == GPU_PENDING {
            demo_report_step();
        }
        if status == GPU_PENDING && time_elapsed_ms(begin, time_milliseconds()) >= budget {
            return GPU_TIMEOUT;
        }
    }
    status
}
fn demo_run(limit: u16, budget: u16) {
    REPORT[11u16] = 11;
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
    let mut rendered_us: u16 = 65535;
    let mut completed = false;
    demo_buttons_init(
        BUTTONS.as_array(),
        dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_BUTTONS),
        time_milliseconds(),
    );
    ds2_init(INPUT.as_array());
    demo_require(
        demo_camera_init(MATRIX.as_array(), CAMERA.as_array(), time_milliseconds()),
        14,
    );
    while limit == 0 || frame < limit {
        time_clear_microseconds();
        let now = time_milliseconds();
        ds2_service(INPUT.as_array(), PACKET.as_array());
        // Input has priority over even the short report-buffer preparation.
        // Preserve an unusually slow in-flight UART packet; success telemetry
        // is best effort and its frame count exposes any omitted reports.
        if completed && REPORT[11u16] == 11 {
            demo_report_queue(1, frame, rendered_us);
        }
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, frame & 31);
        dev_send(7, 5, now);
        dev_send(7, 1, 1);
        let show_numbers = demo_buttons_update(
            BUTTONS.as_array(),
            dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_BUTTONS),
            now,
        );
        demo_ui_frame_begin(STATS.as_array(), rendered_us, completed);
        completed = false;
        dev_send(7, 1, 2);
        demo_require(
            demo_camera_update(MATRIX.as_array(), CAMERA.as_array(), INPUT.as_array(), now),
            14,
        );
        global.light_x = MATRIX[42u16] as i16;
        global.light_y = MATRIX[43u16] as i16;
        global.light_z = MATRIX[44u16] as i16;
        dev_send(7, 6, frame);
        dev_send(7, 1, 3);
        let cb = gpu_cb_begin();
        demo_require(cb == GPU_CB_HANDLE, 2);
        let pass = gpu_pass_begin(cb, 0, 65535);
        demo_require(pass == GPU_CB_HANDLE, 3);
        dev_send(7, 1, 4);
        demo_require(gpu_pass_set_global(pass, view_of(&mut global)), 4);
        dev_send(7, 1, 5);
        demo_require(
            gpu_pass_set_render(pass, view_of(&mut texture), view_of(&mut render)),
            5,
        );
        dev_send(7, 1, 6);
        demo_require(
            gpu_pass_draw_mesh(pass, view_of(&mut mesh), MATRIX.as_array(), 65535, 65535),
            6,
        );
        dev_send(7, 1, 7);
        mesh.directory_word = DEMO_RGB_WORD;
        mesh.count = 1;
        demo_require(
            gpu_pass_draw_mesh(pass, view_of(&mut mesh), MATRIX.as_array(), 65535, 65535),
            6,
        );
        dev_send(7, 1, 8);
        render.unlit = 0;
        demo_require(
            gpu_pass_set_render(pass, view_of(&mut texture), view_of(&mut render)),
            5,
        );
        dev_send(7, 1, 9);
        mesh.directory_word = DEMO_BUNNY_WORD;
        mesh.count = DEMO_BUNNY_COUNT;
        demo_require(
            gpu_pass_draw_mesh(pass, view_of(&mut mesh), MATRIX.as_array(), 65535, 65535),
            6,
        );
        mesh.directory_word = 0;
        mesh.count = DEMO_ROOM_COUNT;
        render.unlit = 1;
        dev_send(7, 1, 10);
        if show_numbers {
            demo_require(
                demo_ui_record(pass, view_of(&mut atlas), STATS.as_array()),
                7,
            );
        }
        dev_send(7, 1, 11);
        demo_require(gpu_pass_end(pass) && gpu_cb_finish(cb), 8);
        dev_send(7, 1, 12);
        demo_require(gpu_submit_idle(cb) == GPU_PENDING, 9);
        dev_send(7, 1, 13);
        demo_require(demo_wait(cb, false, budget) == GPU_COMPLETED, 10);
        rendered_us = time_microseconds();
        dev_send(7, 4, rendered_us);
        dev_send(7, 0, frame + 1);
        dev_send(7, 1, 14);
        demo_require(gpu_present(cb) == GPU_PENDING, 11);
        demo_require(demo_wait(cb, true, budget) == GPU_COMPLETED, 12);
        dev_send(7, 2, frame + 1);
        dev_send(7, 1, 15);
        completed = true;
        frame += 1;
    }
    demo_report_drain();
    if completed {
        demo_report_queue(1, frame, rendered_us);
        demo_report_drain();
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
