//! Fixed-column RTT transitions over the two existing Bloxorz rule fixtures.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_pipeline;
use gpu_pipeline::*;
mod boot_resources;
use boot_resources::*;
mod session;
use session::*;

fn plane_floor(renderer: &mut Renderer) -> Mesh {
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
    return renderer_mesh_batch(renderer, first, count);
}
fn pad_box(
    pass: &mut Pass,
    mesh: &mut Mesh,
    model: &mut Model,
    material: &mut Material,
    center: vec3,
    size: vec3,
) {
    model_pose(model, center, vec3::zero(), size);
    pass.draw(mesh, model, material);
}
fn draw_pad(
    pass: &mut Pass,
    mesh: &mut Mesh,
    model: &mut Model,
    material: &mut Material,
    start: bool,
) {
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
    material_color(material, 48, 64, 72, 255);
    let mut side: u16 = 0;
    while side < 4 {
        let along_x = side < 2;
        let sign = if side & 1 == 0 { -half } else { half };
        pad_box(
            pass,
            mesh,
            model,
            material,
            vec3::new(
                cx + if along_x { sign } else { fix32::zero() },
                -one,
                cz + if along_x { fix32::zero() } else { sign },
            ),
            vec3::new(
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
        material_color(material, 80, 255, 128, 255);
    } else {
        material_color(material, 255, 64, 64, 255);
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
                vec3::new(
                    cx + if along_x { offset } else { fix32::zero() },
                    fix32::from_words(1638, 0),
                    cz + if along_x { fix32::zero() } else { offset },
                ),
                vec3::new(
                    if along_x { width } else { opening },
                    fix32::from_words(3277, 0),
                    if along_x { one } else { width },
                ),
            );
        }
        panel += 1;
    }
}
fn draw_floor(
    frame: &mut Frame,
    camera: &mut Camera,
    floor: &mut Mesh,
    bridge: &mut Mesh,
    model: &mut Model,
    material: &mut Material,
) {
    let mut pass = frame.pass(camera);
    pass.draw(floor, model, material);
    if MOTION[6u16] == 0 && GAME[3u16] & 1 != 0 {
        pass.draw(bridge, model, material);
    }
    pass.finish();
}
fn draw_pads(
    frame: &mut Frame,
    camera: &mut Camera,
    block: &mut Mesh,
    model: &mut Model,
    material: &mut Material,
) {
    let mut pass = frame.pass(camera);
    draw_pad(&mut pass, block, model, material, true);
    draw_pad(&mut pass, block, model, material, false);
    pass.finish();
}
fn draw_block(
    frame: &mut Frame,
    camera: &mut Camera,
    block: &mut Mesh,
    model: &mut Model,
    material: &mut Material,
) {
    block_pose(model);
    let mut pass = frame.pass(camera);
    if block_visible() {
        pass.draw(block, model, material);
    }
    pass.finish();
}
fn draw_hud(frame: &mut Frame, moves: &mut TextField, connection: &mut TextField) {
    let input = frame_input(frame);
    frame_number(frame, moves, GAME[5u16]);
    frame_number(frame, connection, input.connected);
    let mut pass = frame_ui_pass(frame);
    pass_text(&mut pass, moves);
    pass_text(&mut pass, connection);
    pass.finish();
}
fn submit(renderer: &mut Renderer) {
    if renderer.submit() != RENDERER_PENDING || renderer.wait(WAIT_ALL) != RENDERER_COMPLETED {
        halt(0xe080);
    }
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
fn run(limit: u16, automatic_capture: bool, capture_brightness: u16) {
    let mut renderer = renderer_open();
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    let mut storage: RenderBufferMemory = RenderBufferMemory {
        color_base_low: 0,
        color_base_high: 0x60,
        color_capacity_low: 0,
        color_capacity_high: 4,
        depth_base_low: 0,
        depth_base_high: 0x64,
        depth_capacity_low: 0,
        depth_capacity_high: 4,
    };
    let mut buffer = renderer.render_buffer(view_of(&mut storage));
    let mut camera = renderer_camera(
        &mut renderer,
        vec3::new(zero, fix32::from_int(5), fix32::from_int(12)),
        vec3::new(fix32::from_words(32768, 65535), zero, zero),
        fix32::from_int(200),
        fix32::from_int(100),
    );
    let mut screen_camera = renderer_camera(
        &mut renderer,
        vec3::zero(),
        vec3::zero(),
        fix32::from_int(200),
        fix32::from_int(199),
    );
    let mut bridge = renderer_mesh_batch(&mut renderer, BRIDGE_FIRST, BRIDGE_COUNT);
    let mut block = renderer_mesh_batch(&mut renderer, BLOCK_FIRST, BLOCK_COUNT);
    let mut screen = renderer_mesh_batch(&mut renderer, SCREEN_FIRST, SCREEN_COUNT);
    let half = fix32::from_words(32768, 0);
    let mut floor_model = renderer_model(
        &mut renderer,
        vec3::zero(),
        vec3::zero(),
        vec3::new(half, half, half),
    );
    let mut block_model = renderer_model(
        &mut renderer,
        vec3::zero(),
        vec3::zero(),
        vec3::new(one, one, one),
    );
    let mut pad_model = renderer_model(
        &mut renderer,
        vec3::zero(),
        vec3::zero(),
        vec3::new(one, one, one),
    );
    let mut screen_model = renderer_model(
        &mut renderer,
        vec3::new(zero, zero, -one),
        vec3::zero(),
        vec3::new(one, fix32::from_words(39322, 0), one),
    );
    let mut material = renderer_material(&mut renderer, 255, 255, 255, true);
    let mut pad_material = renderer_material(&mut renderer, 255, 255, 255, false);
    let mut tint_material = renderer_material(&mut renderer, 255, 255, 255, false);
    let mut moves = renderer_text(&mut renderer, 0, 0, 4);
    let mut connection = renderer_text(&mut renderer, 6, 0, 1);
    session_init();
    let mut number: u16 = 0;
    loop {
        signal(4, number);
        if automatic_capture {
            capture_report(65, number);
        }
        if SESSION[3u16] != 0 {
            if automatic_capture {
                capture_report(66, number);
            }
            let mut floor = plane_floor(&mut renderer);
            {
                let mut frame = renderer.frame_to(&mut buffer, 0x2222);
                draw_floor(
                    &mut frame,
                    &mut camera,
                    &mut floor,
                    &mut bridge,
                    &mut floor_model,
                    &mut material,
                );
                draw_pads(
                    &mut frame,
                    &mut camera,
                    &mut block,
                    &mut pad_model,
                    &mut pad_material,
                );
                draw_block(
                    &mut frame,
                    &mut camera,
                    &mut block,
                    &mut block_model,
                    &mut material,
                );
                draw_hud(&mut frame, &mut moves, &mut connection);
                frame.finish();
            }
            if renderer.submit() != RENDERER_PENDING
                || renderer.wait(WAIT_GPU) != RENDERER_COMPLETED
            {
                halt(0xe081);
            }
            if automatic_capture {
                capture_report(67, number);
            }
            session_captured();
            signal(11, SESSION[4u16]);
        }
        {
            let mut floor = plane_floor(&mut renderer);
            let mut frame = renderer.frame(0x2222);
            let input = frame_input(&mut frame);
            let elapsed = frame_elapsed_ms(&mut frame);
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
                if fading() {
                    let color = tint();
                    material_color(&mut tint_material, color, color, color, 255);
                    let mut view = buffer.texture(false);
                    let mut pass = frame.pass(&mut screen_camera);
                    pass.draw_texture(
                        &mut screen,
                        &mut screen_model,
                        &mut tint_material,
                        &mut view,
                    );
                    pass.finish();
                    if automatic_capture {
                        frame_number(&mut frame, &mut moves, 1000 + color);
                        let mut overlay = frame_ui_pass(&mut frame);
                        pass_text(&mut overlay, &mut moves);
                        overlay.finish();
                    }
                    frame.finish();
                    if automatic_capture {
                        capture_report(68, tint());
                    }
                    if renderer.submit() != RENDERER_PENDING {
                        halt(0xe082);
                    }
                    view.close();
                    if automatic_capture {
                        capture_report(69, tint());
                    }
                    if renderer.wait(WAIT_ALL) != RENDERER_COMPLETED {
                        halt(0xe083);
                    }
                } else {
                    draw_floor(
                        &mut frame,
                        &mut camera,
                        &mut floor,
                        &mut bridge,
                        &mut floor_model,
                        &mut material,
                    );
                    draw_pads(
                        &mut frame,
                        &mut camera,
                        &mut block,
                        &mut pad_model,
                        &mut pad_material,
                    );
                    draw_block(
                        &mut frame,
                        &mut camera,
                        &mut block,
                        &mut block_model,
                        &mut material,
                    );
                    draw_hud(&mut frame, &mut moves, &mut connection);
                    frame.finish();
                    submit(&mut renderer);
                }
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
                    capture_report(76, dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW));
                    capture_report(85, dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH));
                    capture_report(77, dev_recv(DISPLAY_DEVICE, DISPLAY_COLOR_FORMAT));
                    capture_report(80, dev_recv(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND));
                    capture_report(71, dev_recv(GPU_DEVICE, GPU_STATUS));
                    halt(0x600d);
                }
                session_presented();
            } else {
                drop(frame);
            }
        }
        number += 1;
        if limit != 0 && number == limit {
            break;
        }
    }
    if !buffer.close() {
        halt(0xe084);
    }
    renderer.close();
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
