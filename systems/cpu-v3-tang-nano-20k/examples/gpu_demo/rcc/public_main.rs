//! Complete classic scene through the public API and continuous gamepad input.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
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

static BUTTONS: Buf<u16, 4> = Buf::new([0; 4]);
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
    demo_report_drain();
    demo_report_queue(128 | code, 0, 0);
    demo_report_drain();
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 32 | code);
    halt(0xd000 | code);
}
fn require(ok: bool, code: u16) {
    if !ok {
        demo_fail(code);
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
        require(time_elapsed_ms(start, time_milliseconds()) < budget, 10);
    }
}
fn wait_display(display: &mut Display, budget: u16) {
    let start = time_milliseconds();
    while !display.try_wait_swap() {
        demo_report_step();
        require(time_elapsed_ms(start, time_milliseconds()) < budget, 12);
    }
}
fn prepare_ui(grid: &mut UiGrid) -> u16 {
    if STATS[3u16] == 0 {
        return 0;
    }
    let digits: Buf<u16, 4> = Buf::new([0; 4]);
    frame_stats_tenths_digits(STATS.as_array(), digits.as_array());
    let mut first: u16 = 0;
    if digits[0u16] == 48 {
        first = 1;
    }
    let mut i = first;
    while i < 4 {
        let x = 2 + i - first;
        if digits[i] == 46 {
            grid.write(
                CellRect {
                    x: x,
                    y: 1,
                    width: 1,
                    height: 1,
                },
                b".",
            );
        } else {
            demo_glyph(grid, x, 1, digits[i] - 48);
        }
        i += 1;
    }
    4 - first
}
fn demo_run(limit: u16, budget: u16) {
    REPORT[11u16] = 11;
    frame_stats_reset(STATS.as_array());
    let mut gpu = Gpu::init(demo_config());
    let mut buffer = gpu.command_buffer();
    let color_a = demo_color_a();
    let color_b = demo_color_b();
    let depth = demo_depth();
    let mut display = Display::open_with_format(DisplayFormat::LinearRgb565);
    let mut camera = demo_camera();
    let mut motion = demo_motion_init(time_milliseconds());
    camera.set_pose(motion.eye, demo_motion_angles(motion));
    let mut state = demo_state();
    state.set_depth(DepthFunc::Less, true);
    state.set_filter(TextureFilter::Trilinear);
    state.set_lighting(demo_lighting(demo_point(
        fix32::from_words(36045, 65535),
        fix32::from_words(52429, 0),
        fix32::from_words(6554, 1),
    )));
    let room = demo_mesh_part(0, 6);
    let rgb = demo_mesh_part(6, 1);
    let rabbit = demo_mesh_part(7, 17);
    let model = Model::new(demo_pose(demo_zero(), demo_zero(), demo_scale()));
    let mut unlit = demo_material(demo_white(), true);
    // Preserve the native global/render codes even while lighting is bypassed.
    unlit.set_response(fix32::from_words(16384, 0), fix32::from_words(49152, 0));
    unlit.set_specular(
        Rgb8 {
            r: 160,
            g: 160,
            b: 160,
        },
        Shininess::S32,
    );
    let mut lit = demo_material(demo_white(), false);
    lit.set_response(fix32::from_words(16384, 0), fix32::from_words(49152, 0));
    lit.set_specular(
        Rgb8 {
            r: 160,
            g: 160,
            b: 160,
        },
        Shininess::S32,
    );
    let texture = demo_texture();
    let view = texture.view();
    let mut grid = demo_grid(false);
    gamepad_init();
    demo_buttons_init(
        BUTTONS.as_array(),
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
        if completed && REPORT[11u16] == 11 {
            demo_report_queue(1, frame, observed);
        }
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, frame & 31);
        dev_send(7, 1, 1);
        let visible = demo_buttons_update(
            BUTTONS.as_array(),
            dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_BUTTONS),
            now,
        );
        frame_stats_elapsed_begin(STATS.as_array(), observed, completed);
        completed = false;
        motion = demo_motion_update(motion, input, now);
        if motion.changed {
            camera.set_pose(motion.eye, demo_motion_angles(motion));
        }
        let mut glyphs: u16 = 0;
        if visible {
            glyphs = prepare_ui(&mut grid);
        }
        buffer.reset();
        if frame == 0 {
            buffer.invalidate_textures();
        }
        if frame & 1 == 0 {
            let mut pass = buffer.pass(
                &color_b,
                &depth,
                ColorLoad::clear(Rgb565::from_raw(0)),
                DepthLoad::clear(65535),
            );
            pass.set_camera(&camera);
            pass.set_state(&state);
            pass.draw(&room, &model, &unlit, &view);
            pass.draw_colored(&rgb, &model, &unlit);
            pass.draw(&rabbit, &model, &lit, &view);
            if glyphs != 0 {
                pass.draw_ui(
                    &grid,
                    CellRect {
                        x: 2,
                        y: 1,
                        width: glyphs,
                        height: 1,
                    },
                );
            }
        } else {
            let mut pass = buffer.pass(
                &color_a,
                &depth,
                ColorLoad::clear(Rgb565::from_raw(0)),
                DepthLoad::clear(65535),
            );
            pass.set_camera(&camera);
            pass.set_state(&state);
            pass.draw(&room, &model, &unlit, &view);
            pass.draw_colored(&rgb, &model, &unlit);
            pass.draw(&rabbit, &model, &lit, &view);
            if glyphs != 0 {
                pass.draw_ui(
                    &grid,
                    CellRect {
                        x: 2,
                        y: 1,
                        width: glyphs,
                        height: 1,
                    },
                );
            }
        }
        dev_send(7, 1, 12);
        gpu.submit(buffer);
        dev_send(7, 1, 13);
        buffer = wait_gpu(&mut gpu, budget);
        observed = time_microseconds();
        dev_send(7, 4, observed);
        dev_send(7, 1, 14);
        if frame & 1 == 0 {
            display.set_next_swap(&color_b);
        } else {
            display.set_next_swap(&color_a);
        }
        wait_display(&mut display, budget);
        dev_send(7, 2, frame + 1);
        dev_send(7, 1, 15);
        completed = true;
        frame += 1;
    }
    demo_report_drain();
    if completed {
        demo_report_queue(1, frame, observed);
        demo_report_drain();
    }
    halt(0x600d);
}
fn main() {
    demo_run(0, 2000);
}
pub fn demo_public_three_frames() {
    demo_run(3, 2000);
}
