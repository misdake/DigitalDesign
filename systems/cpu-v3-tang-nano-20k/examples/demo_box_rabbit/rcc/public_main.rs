//! Complete classic scene through the public API and continuous gamepad input.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_board;
mod gpu_demo_api;
use gpu_board::*;
use gpu_demo_api::*;
mod gamepad;
use gamepad::*;
mod motion;
use motion::*;
mod controls;
use controls::*;
mod frame_stats;
use frame_stats::*;
mod time;
use time::*;
mod device_abi;
use device_abi::*;
mod boot_resources;
use boot_resources::*;

#[static_buffer]
const STATIC_SCENE: PreparedMemory = PreparedMemory::allocate(4096, 4096);

// One previous completed report, streamed only while GPU/present is pending.
// A slow UART never stalls input collection or command construction.
#[repr(C)]
struct DemoReport {
    bytes: Buf<u16, 11>,
    cursor: u16,
}
static REPORT: DemoReport = DemoReport {
    bytes: Buf::new([0; 11]),
    cursor: 0,
};
fn demo_report_queue(status: u16, frame: u16, us: u16) {
    REPORT.bytes[0u16] = 68;
    REPORT.bytes[1u16] = 68;
    REPORT.bytes[2u16] = 71;
    REPORT.bytes[3u16] = 68;
    REPORT.bytes[4u16] = 3;
    REPORT.bytes[5u16] = status;
    REPORT.bytes[6u16] = frame & 255;
    REPORT.bytes[7u16] = frame >> 8;
    REPORT.bytes[8u16] = us & 255;
    REPORT.bytes[9u16] = us >> 8;
    let mut i: u16 = 4;
    let mut sum: u16 = 0;
    while i < 10 {
        sum ^= REPORT.bytes[i];
        i += 1;
    }
    REPORT.bytes[10u16] = sum;
    REPORT.cursor = 0;
}
fn demo_report_step() {
    let i = REPORT.cursor;
    if i < 11 && dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 == 0 {
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, REPORT.bytes[i]);
        REPORT.cursor = i + 1;
    }
}
fn demo_report_drain() {
    while REPORT.cursor < 11 {
        demo_report_step();
    }
}

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum DemoPhase {
    FrameStart = 1,
    InvalidateTextures = 9,
    RecordStatic = 10,
    StaticSubmitted = 11,
    DynamicRecorded = 12,
    DynamicSubmitted = 13,
    DynamicRetired = 14,
    Presented = 15,
    WaitStatic = 16,
}

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum DemoTimeout {
    Gpu = 10,
    Display = 12,
}

fn demo_fail(reason: DemoTimeout) {
    let code = reason as u16;
    demo_report_drain();
    demo_report_queue(128 | code, 0, 0);
    demo_report_drain();
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 32 | code);
    halt(0xd000 | code);
}
fn require(ok: bool, reason: DemoTimeout) {
    if !ok {
        demo_fail(reason);
    }
}
fn wait_gpu(gpu: &mut Gpu, budget: u16) -> CommandBuffer {
    let start = time_milliseconds();
    loop {
        match gpu.try_wait() {
            Some(buffer) => {
                return buffer;
            }
            None => {}
        }
        demo_report_step();
        require(
            time_elapsed_ms(start, time_milliseconds()) < budget,
            DemoTimeout::Gpu,
        );
    }
}
fn wait_display(display: &mut Display, budget: u16) {
    let start = time_milliseconds();
    while !display.try_wait_swap() {
        demo_report_step();
        require(
            time_elapsed_ms(start, time_milliseconds()) < budget,
            DemoTimeout::Display,
        );
    }
}
fn prepare_ui(grid: &mut UiGrid, stats: Array<u16>) -> u16 {
    if stats[3u16] == 0 {
        return 0;
    }
    let digits: Buf<u16, 4> = Buf::new([0; 4]);
    frame_stats_tenths_digits(stats, digits.as_array());
    let mut first: u16 = 0;
    if digits[0u16] == 48 {
        first = 1;
    }
    let mut i = first;
    while i < 4 {
        let x = 2 + i - first;
        grid.write_glyph(x, 1, digits[i] as u8);
        i += 1;
    }
    4 - first
}
const ROOM_FIRST: u16 = 0;
const ROOM_COUNT: u16 = 6;
const RGB_FIRST: u16 = 6;
const RABBIT_FIRST: u16 = 7;
const RABBIT_COUNT: u16 = 17;

// Record World/material once; each frame supplies its own Pass View and target.
fn demo_record_static(
    mesh: &Mesh,
    draws: &mut PreparedDraws,
    model: &Model,
    material: &Material,
    view: &TextureView,
    state: &RenderState,
) {
    let parts = mesh.ptr();
    let room = parts.slice(ROOM_FIRST, ROOM_COUNT);
    let rgb = parts.slice(RGB_FIRST, 1);
    let mut part = draws.record();
    part.set_state(state.ptr());
    part.draw(room, model.ptr(), material.ptr(), view.ptr());
    part.draw_colored(rgb, model.ptr(), material.ptr());
    part.finish();
}
fn scene_material(unlit: bool) -> Material {
    // Retain S32/specular codes even when the room bypasses lighting.
    Material::new(
        MaterialDesc::lit(Rgba8::white(), Rgb8::new(160, 160, 160), Shininess::S32)
            .with_response(fix!(0.25), fix!(0.75))
            .with_unlit(unlit),
    )
}
fn demo_run(limit: u16, budget: u16, orbit: bool) {
    // These four-word buffers are the controls/frame_stats module interfaces.
    let buttons: Buf<u16, 4> = Buf::new([0; 4]);
    let stats: Buf<u16, 4> = Buf::new([0; 4]);
    frame_stats_reset(stats.as_array());
    REPORT.cursor = 11;
    let mut gpu = Gpu::init(board_gpu_config());
    let mut static_buffer = gpu.command_buffer();
    let mut dynamic_buffer = gpu.command_buffer();
    let color_a = board_color_a();
    let color_b = board_color_b();
    let depth = board_depth();
    let mut display = Display::open_with_format(DisplayFormat::LinearRgb565);
    let mut camera = Camera::new(CameraDesc::new(
        Vec3::zero(),
        Vec3::zero(),
        Projection::from_focal_pixels(fix!(200), fix!(199)),
    ));
    let mut motion = DemoMotion::new(time_milliseconds());
    camera.set_pose(motion.eye, motion.angles());
    let light = Lighting::directional(
        Vec3::new(fix!(-0.45), fix!(0.8), fix!(1.1)),
        fix!(1),
        fix!(1),
    );
    let state = RenderState::new(RenderStateDesc::opaque(light));
    let mesh = demo_mesh();
    let parts = mesh.ptr();
    let rabbit = parts.slice(RABBIT_FIRST, RABBIT_COUNT);
    let box_model = Model::new(Transform::identity());
    let rabbit_model = Model::new(Transform::identity());
    let unlit = scene_material(true);
    let lit = scene_material(false);
    let texture = demo_texture();
    let view = texture.view();
    let mut grid = demo_grid(false);
    // Cache invalidation is startup maintenance, never part of the reusable stream.
    dynamic_buffer.invalidate_textures();
    dev_send(7, 1, DemoPhase::InvalidateTextures as u16);
    gpu.submit(dynamic_buffer);
    dynamic_buffer = wait_gpu(&mut gpu, budget);
    let mut room_draws = PreparedDraws::new(STATIC_SCENE);
    demo_record_static(&mesh, &mut room_draws, &box_model, &unlit, &view, &state);
    gamepad_init();
    demo_buttons_init(
        buttons.as_array(),
        dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_BUTTONS),
        time_milliseconds(),
    );
    // Initial resource construction is outside the first motion interval.
    motion.previous_ms = time_milliseconds();
    let mut frame: u16 = 0;
    let mut observed: u16 = 65535;
    let mut completed = false;
    while limit == 0 || frame < limit {
        time_clear_microseconds();
        let now = time_milliseconds();
        gamepad_capture();
        let input = gamepad_take_frame();
        if completed && REPORT.cursor == 11 {
            demo_report_queue(1, frame, observed);
        }
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, frame & 31);
        dev_send(7, 1, DemoPhase::FrameStart as u16);
        let visible = demo_buttons_update(
            buttons.as_array(),
            dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_BUTTONS),
            now,
        );
        frame_stats_elapsed_begin(stats.as_array(), observed, completed);
        completed = false;
        if orbit {
            // A complete, repeatable orbit every 512 successful frames.
            let phase = fix32::from_words(frame << 7, 0);
            let direction = fsincos(phase * fix32::from_words(18559, 6));
            let radius = fix!(2.8);
            camera.set_look_at_y_up(
                Vec3 {
                    x: direction.x() * radius,
                    y: fix!(0.22),
                    z: direction.y() * radius,
                },
                Vec3::zero(),
            );
        } else {
            if motion.update(&input, now) {
                camera.set_pose(motion.eye, motion.angles());
            }
        }
        // The immutable part supplies World/material, the new Pass supplies View.
        dev_send(7, 1, DemoPhase::RecordStatic as u16);
        let back: ColorBufferPtr = if frame & 1 != 0 {
            color_a.ptr()
        } else {
            color_b.ptr()
        };
        static_buffer.reset();
        {
            let mut pass = static_buffer.pass(
                back,
                depth.ptr(),
                ColorLoad::clear(Rgb565::from_raw(0)),
                DepthLoad::clear_far(),
            );
            pass.set_camera(camera.ptr());
            pass.set_state(state.ptr());
            pass.draw_prepared(room_draws.ptr());
        }
        gpu.submit(static_buffer);
        dev_send(7, 1, DemoPhase::StaticSubmitted as u16);
        // Only dynamic CPU storage is touched while the static GPU work runs.
        dynamic_buffer.reset();
        let mut glyphs: u16 = 0;
        if visible {
            glyphs = prepare_ui(&mut grid, stats.as_array());
        }
        {
            let mut pass =
                dynamic_buffer.pass(back, depth.ptr(), ColorLoad::load(), DepthLoad::load());
            pass.set_camera(camera.ptr());
            pass.set_state(state.ptr());
            pass.draw(rabbit, rabbit_model.ptr(), lit.ptr(), view.ptr());
            if glyphs != 0 {
                pass.draw_ui(grid.ptr(), CellRect::new(2, 1, glyphs, 1));
            }
        }
        dev_send(7, 1, DemoPhase::DynamicRecorded as u16);
        dev_send(7, 1, DemoPhase::WaitStatic as u16);
        static_buffer = wait_gpu(&mut gpu, budget);
        gpu.submit(dynamic_buffer);
        dev_send(7, 1, DemoPhase::DynamicSubmitted as u16);
        dynamic_buffer = wait_gpu(&mut gpu, budget);
        observed = time_microseconds();
        dev_send(7, 4, observed);
        dev_send(7, 1, DemoPhase::DynamicRetired as u16);
        display.set_next_swap(back);
        wait_display(&mut display, budget);
        dev_send(7, 2, frame + 1);
        dev_send(7, 1, DemoPhase::Presented as u16);
        completed = true;
        frame += 1;
    }
    static_buffer.reset();
    demo_report_drain();
    if completed {
        demo_report_queue(1, frame, observed);
        demo_report_drain();
    }
    halt(0x600d);
}
fn main() {
    demo_run(0, 2000, GPU_DEMO_AUTO_ORBIT != 0);
}
pub fn demo_public_three_frames() {
    demo_run(3, 2000, false);
}
