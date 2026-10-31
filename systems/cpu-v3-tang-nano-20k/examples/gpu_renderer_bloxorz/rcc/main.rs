//! Fixed-column RTT transitions over the two existing Bloxorz rule fixtures.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
use gpu_demo_api::*;
mod profiler_clock;
use profiler_clock::*;
mod gamepad;
use gamepad::*;
mod boot_resources;
use boot_resources::*;
mod session;
use session::*;

fn plane_floor() -> Mesh {
    let first = if MOTION[6u16] == 0 {
        FLOOR0_FIRST
    } else {
        FLOOR1_FIRST
    };
    let count = if MOTION[6u16] == 0 {
        FLOOR0_COUNT
    } else {
        FLOOR1_COUNT
    };
    return demo_mesh_part(first, count);
}
fn pad_box(
    pass: &mut Pass,
    mesh: &Mesh,
    model: &mut Model,
    material: &mut Material,
    center: Vec3,
    size: Vec3,
) {
    model.set_pose(demo_pose(center, demo_zero(), size));
    let texture = demo_texture();
    let view = texture.view();
    pass.draw(mesh, model, material, &view);
}
fn draw_pad(pass: &mut Pass, mesh: &Mesh, model: &mut Model, material: &mut Material, start: bool) {
    let x = if start { LEVEL[2u16] } else { SESSION[6u16] };
    let z = if start { LEVEL[3u16] } else { SESSION[7u16] };
    if x == 65535 {
        return;
    }
    let mode = visual_mode();
    let t = visual_time();
    let mut opening = fix32::zero();
    if (start && mode == ENTRY_OPEN) || (!start && mode == GOAL_OPEN) {
        let f = fraction(t, 200);
        opening = f * f * (fix32::from_int(3) - fix32::from_int(2) * f);
    }
    if (start && mode == ENTRY_RISE) || (!start && (mode == GOAL_DROP || mode == GOAL_LAND)) {
        opening = fix32::from_int(1);
    }
    if !start && mode == GOAL_CLOSE {
        let f = fraction(t, 200);
        opening = fix32::from_int(1) - f * f * (fix32::from_int(3) - fix32::from_int(2) * f);
    }
    let one = fix32::from_int(1);
    let half = fix32::from_words(32768, 0);
    let cx = fix32::from_int(x as i16) - fix32::from_words(32768, 3);
    let cz = fix32::from_int(z as i16) - one;
    // Four opaque well walls, sharing one ordinary box and one Model slot.
    material.set_uniform_color(Rgba8 {
        r: 48,
        g: 64,
        b: 72,
        a: 255,
    });
    let mut side: u16 = 0;
    while side < 4 {
        let along_x = side < 2;
        let sign = if side & 1 == 0 { -half } else { half };
        pad_box(
            pass,
            mesh,
            model,
            material,
            demo_point(
                cx + if along_x { sign } else { fix32::zero() },
                -one,
                cz + if along_x { fix32::zero() } else { sign },
            ),
            demo_point(
                if along_x {
                    fix32::from_words(3932, 0)
                } else {
                    one
                },
                fix32::from_int(2),
                if along_x {
                    one
                } else {
                    fix32::from_words(3932, 0)
                },
            ),
        );
        side += 1;
    }
    if start {
        material.set_uniform_color(Rgba8 {
            r: 80,
            g: 255,
            b: 128,
            a: 255,
        });
    } else {
        material.set_uniform_color(Rgba8 {
            r: 255,
            g: 64,
            b: 64,
            a: 255,
        });
    }
    let width = (one - opening) * half;
    if width < fix32::from_words(256, 0) {
        return;
    }
    let mut panel: u16 = 0;
    while panel < 4 {
        let along_x = panel < 2;
        let sign = if panel & 1 == 0 { -one } else { one };
        let offset = sign * (half - width * half);
        // Side panels span the whole tile; the other pair fills only the center.
        if along_x || opening >= fix32::from_words(256, 0) {
            pad_box(
                pass,
                mesh,
                model,
                material,
                demo_point(
                    cx + if along_x { offset } else { fix32::zero() },
                    fix32::from_words(1638, 0),
                    cz + if along_x { fix32::zero() } else { offset },
                ),
                demo_point(
                    if along_x { width } else { opening },
                    fix32::from_words(3277, 0),
                    if along_x { one } else { width },
                ),
            );
        }
        panel += 1;
    }
}
fn draw_floor(pass: &mut Pass, floor: &Mesh, bridge: &Mesh, model: &Model, material: &Material) {
    let texture = demo_texture();
    let view = texture.view();
    pass.draw(floor, model, material, &view);
    if MOTION[6u16] == 0 && GAME[3u16] & 1 != 0 {
        pass.draw(bridge, model, material, &view);
    }
}
fn draw_pads(pass: &mut Pass, block: &Mesh, model: &mut Model, material: &mut Material) {
    draw_pad(pass, block, model, material, true);
    draw_pad(pass, block, model, material, false);
}
fn draw_block(pass: &mut Pass, block: &Mesh, model: &mut Model, material: &Material) {
    block_pose(model);
    if block_visible() {
        let texture = demo_texture();
        let view = texture.view();
        pass.draw(block, model, material, &view);
    }
}
fn game_camera() -> Camera {
    Camera::new(CameraDesc {
        eye: demo_point(fix32::zero(), fix32::from_int(5), fix32::from_int(12)),
        angles: demo_point(
            fix32::from_words(32768, 65535),
            fix32::zero(),
            fix32::zero(),
        ),
        projection: Projection::from_focal_pixels(fix32::from_int(200), fix32::from_int(100)),
    })
}
// Capture-only progress: ASCII stage, four hexadecimal digits and LF.
// A=loop, B=source begin, C=source retired, D=consumer submit,
// E=consumer wait, F=presented, H=selected-brightness halt.
fn capture_report(stage: u16, value: u16) {
    let mut i: u16 = 0;
    while i < 6 {
        let mut byte = stage;
        if i == 5 {
            byte = 10;
        }
        if i > 0 && i < 5 {
            let nibble = (value >> ((4 - i) * 4)) & 15;
            byte = if nibble < 10 {
                nibble + 48
            } else {
                nibble + 55
            };
        }
        while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 {}
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, byte);
        i += 1;
    }
}
// Explicit capture-card diagnostic only: hold the initial scene, make one real
// rule move, hold that result, then stop after an opaque selected-brightness RTT frame.
static CAPTURE_STIMULUS: Buf<u16, 2> = Buf::new([0; 2]);
fn capture_buttons(elapsed: u16) -> u16 {
    if SESSION[0u16] != PLAY {
        return 0;
    }
    let phase = CAPTURE_STIMULUS[0u16];
    let dt = if elapsed > 50 { 50u16 } else { elapsed };
    if phase == 1 && GAME[5u16] == 1 {
        CAPTURE_STIMULUS[0u16] = 2;
        CAPTURE_STIMULUS[1u16] = 0;
        return 0;
    }
    if phase == 0 || phase == 2 {
        CAPTURE_STIMULUS[1u16] += dt;
        if CAPTURE_STIMULUS[1u16] >= 2000 {
            CAPTURE_STIMULUS[1u16] = 0;
            CAPTURE_STIMULUS[0u16] = phase + 1;
            return if phase == 0 { 32u16 } else { 8u16 };
        }
    }
    0
}
fn game_floor_model() -> Model {
    let half = fix32::from_words(32768, 0);
    Model::new(demo_pose(
        demo_zero(),
        demo_zero(),
        demo_point(half, half, half),
    ))
}
fn game_plain_model() -> Model {
    Model::new(demo_pose(demo_zero(), demo_zero(), demo_scale()))
}
fn game_screen_model() -> Model {
    let one = fix32::from_int(1);
    Model::new(demo_pose(
        demo_point(fix32::zero(), fix32::zero(), -one),
        demo_zero(),
        demo_point(one, fix32::from_words(39322, 0), one),
    ))
}

fn run(limit: u16, automatic_capture: bool, capture_brightness: u16) {
    let mut gpu = Gpu::init(demo_config());
    let mut commands = gpu.command_buffer();
    let offscreen = ColorBuffer::from_memory(unsafe { GpuMemory::from_sectors(0xc000, 2048) });
    let offscreen_depth =
        DepthBuffer::from_memory(unsafe { GpuMemory::from_sectors(0xc800, 2048) });
    let color_a = demo_color_a();
    let color_b = demo_color_b();
    let depth = demo_depth();
    let mut display = Display::open();
    let camera = game_camera();
    let screen_camera = demo_camera();
    let state = demo_state();
    let bridge = demo_mesh_part(BRIDGE_FIRST, BRIDGE_COUNT);
    let block = demo_mesh_part(BLOCK_FIRST, BLOCK_COUNT);
    let screen = demo_mesh_part(SCREEN_FIRST, SCREEN_COUNT);
    let floor_model = game_floor_model();
    let mut block_model = game_plain_model();
    let mut pad_model = game_plain_model();
    let screen_model = game_screen_model();
    let material = demo_material(demo_white(), false);
    let mut pad_material = demo_material(demo_white(), true);
    let mut tint_material = demo_material(demo_white(), true);
    let mut grid = demo_grid(false);
    session_init();
    gamepad_init();
    let mut previous = profiler_active_milliseconds();
    let mut connected: u16 = 0;
    let mut number: u16 = 0;
    let mut back_b = true;
    loop {
        signal(4, number);
        if automatic_capture {
            capture_report(65, number);
        }
        if SESSION[3u16] != 0 {
            if automatic_capture {
                capture_report(66, number);
            }
            let floor = plane_floor();
            demo_number(&mut grid, 0, 0, 4, GAME[5u16]);
            demo_number(&mut grid, 6, 0, 1, connected);
            commands.reset();
            commands.invalidate_textures();
            {
                let mut pass = commands.pass(
                    &offscreen,
                    &offscreen_depth,
                    ColorLoad::clear(Rgb565::from_raw(0x2222)),
                    DepthLoad::clear(65535),
                );
                pass.set_camera(&camera);
                pass.set_state(&state);
                draw_floor(&mut pass, &floor, &bridge, &floor_model, &material);
                draw_pads(&mut pass, &block, &mut pad_model, &mut pad_material);
                draw_block(&mut pass, &block, &mut block_model, &material);
                pass.draw_ui(
                    &grid,
                    CellRect {
                        x: 0,
                        y: 0,
                        width: 7,
                        height: 1,
                    },
                );
            }
            let resume_before = profiler_resume_epoch();
            gpu.submit(commands);
            commands = gpu.wait();
            if profiler_resume_epoch() != resume_before {
                gamepad_capture();
                gamepad_discard_edges();
            }
            if automatic_capture {
                capture_report(67, number);
            }
            session_captured();
            signal(11, SESSION[4u16]);
        }
        gamepad_capture();
        let input = gamepad_take_frame();
        connected = input.connected;
        let now = profiler_active_milliseconds();
        let elapsed = now - previous;
        previous = now;
        let buttons = if automatic_capture {
            capture_buttons(elapsed)
        } else {
            input.pressed
        };
        session_tick(buttons, elapsed);
        if automatic_capture && CAPTURE_STIMULUS[0u16] == 3 && SESSION[0u16] == FADE_OUT {
            let stop_time = if capture_brightness == 255 {
                0u16
            } else {
                160u16
            };
            if SESSION[1u16] > stop_time {
                SESSION[1u16] = stop_time;
            }
        }
        if SESSION[3u16] == 0 {
            let floor = plane_floor();
            let fade = fading();
            let color = tint();
            tint_material.set_uniform_color(Rgba8 {
                r: color as u8,
                g: color as u8,
                b: color as u8,
                a: 255,
            });
            demo_number(&mut grid, 0, 0, 4, GAME[5u16]);
            demo_number(&mut grid, 6, 0, 1, connected);
            if fade && automatic_capture {
                demo_number(&mut grid, 0, 0, 4, 1000 + color);
            }
            commands.reset();
            commands.invalidate_textures();
            if back_b {
                {
                    let mut pass = commands.pass(
                        &color_b,
                        &depth,
                        ColorLoad::clear(Rgb565::from_raw(0x2222)),
                        DepthLoad::clear(65535),
                    );
                    pass.set_state(&state);
                    if fade {
                        let view = offscreen.texture_view();
                        pass.set_camera(&screen_camera);
                        pass.draw(&screen, &screen_model, &tint_material, &view);
                        if automatic_capture {
                            pass.draw_ui(
                                &grid,
                                CellRect {
                                    x: 0,
                                    y: 0,
                                    width: 4,
                                    height: 1,
                                },
                            );
                        }
                    } else {
                        pass.set_camera(&camera);
                        draw_floor(&mut pass, &floor, &bridge, &floor_model, &material);
                        draw_pads(&mut pass, &block, &mut pad_model, &mut pad_material);
                        draw_block(&mut pass, &block, &mut block_model, &material);
                        pass.draw_ui(
                            &grid,
                            CellRect {
                                x: 0,
                                y: 0,
                                width: 7,
                                height: 1,
                            },
                        );
                    }
                }
            } else {
                {
                    let mut pass = commands.pass(
                        &color_a,
                        &depth,
                        ColorLoad::clear(Rgb565::from_raw(0x2222)),
                        DepthLoad::clear(65535),
                    );
                    pass.set_state(&state);
                    if fade {
                        let view = offscreen.texture_view();
                        pass.set_camera(&screen_camera);
                        pass.draw(&screen, &screen_model, &tint_material, &view);
                        if automatic_capture {
                            pass.draw_ui(
                                &grid,
                                CellRect {
                                    x: 0,
                                    y: 0,
                                    width: 4,
                                    height: 1,
                                },
                            );
                        }
                    } else {
                        pass.set_camera(&camera);
                        draw_floor(&mut pass, &floor, &bridge, &floor_model, &material);
                        draw_pads(&mut pass, &block, &mut pad_model, &mut pad_material);
                        draw_block(&mut pass, &block, &mut block_model, &material);
                        pass.draw_ui(
                            &grid,
                            CellRect {
                                x: 0,
                                y: 0,
                                width: 7,
                                height: 1,
                            },
                        );
                    }
                }
            }
            if automatic_capture {
                capture_report(68, tint());
            }
            let resume_before = profiler_resume_epoch();
            gpu.submit(commands);
            if automatic_capture {
                capture_report(69, tint());
            }
            commands = gpu.wait();
            if profiler_resume_epoch() != resume_before {
                gamepad_capture();
                gamepad_discard_edges();
            }
            if back_b {
                display.set_next_swap(&color_b);
            } else {
                display.set_next_swap(&color_a);
            }
            display.wait_vblank();
            // Capture reports received during the locked presentation interval.
            // session_presented discards their edges before entry becomes active.
            gamepad_capture();
            signal(12, SESSION[0u16]);
            if automatic_capture {
                capture_report(70, SESSION[0u16]);
            }
            if automatic_capture
                && CAPTURE_STIMULUS[0u16] == 3
                && SESSION[0u16] == FADE_OUT
                && tint() == capture_brightness
            {
                capture_report(72, tint());
                // Read-only final diagnostic, after both public completion boundaries.
                capture_report(76, dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW));
                capture_report(85, dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH));
                capture_report(77, dev_recv(DISPLAY_DEVICE, DISPLAY_COLOR_FORMAT));
                capture_report(80, dev_recv(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND));
                capture_report(71, dev_recv(GPU_DEVICE, GPU_STATUS));
                halt(0x600d);
            }
            session_presented();
            back_b = !back_b;
        }
        number += 1;
        if limit != 0 && number == limit {
            break;
        }
    }
}
pub fn renderer_game_frames() {
    run(120, false, 128);
    halt(0x600d);
}
fn main() {
    run(0, false, 128);
}
pub fn renderer_game_capture() {
    run(0, true, 128);
    halt(0x600d);
}
pub fn renderer_game_capture_full() {
    run(0, true, 255);
    halt(0x600d);
}

pub fn renderer_game_three_frames() {
    run(3, false, 128);
    halt(0x600d);
}
