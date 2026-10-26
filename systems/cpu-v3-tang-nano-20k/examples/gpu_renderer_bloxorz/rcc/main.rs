//! Bloxorz-shaped API fixture, not a full game or legacy renderer migration.
use crate::dsl_rt::*;
mod gpu_pipeline;
use gpu_pipeline::*;
mod boot_resources;
use boot_resources::*;
mod bloxorz_core;
use bloxorz_core::*;
static LEVEL: Buf<u16, 200> = Buf::new([0; 200]);
static GAME: Buf<u16, 10> = Buf::new([0; 10]);
static HISTORY: Buf<u16, 1536> = Buf::new([0; 1536]);
static HISTORY_STATE: Buf<u16, 2> = Buf::new([0; 2]);
// Semantic animation state: old x/z/orientation, direction, elapsed, moving, level.
static MOTION: Buf<u16, 7> = Buf::new([0; 7]);
fn level_reset(which: u16) {
    blox_fixture_level(which, LEVEL.as_array());
    blox_reset(LEVEL.as_array(), GAME.as_array(), HISTORY_STATE.as_array());
    MOTION[5u16] = 0;
    MOTION[6u16] = which;
}
fn game_update(buttons: u16, elapsed: u16) {
    if buttons & 4096 != 0 {
        level_reset(MOTION[6u16] ^ 1);
        return;
    }
    if buttons & 8 != 0 {
        level_reset(MOTION[6u16]);
        return;
    }
    if MOTION[5u16] != 0 {
        let dt = if elapsed > 100 { 100u16 } else { elapsed };
        MOTION[4u16] += dt;
        if MOTION[4u16] >= 100 {
            blox_dispatch(
                LEVEL.as_array(),
                GAME.as_array(),
                HISTORY.as_array(),
                HISTORY_STATE.as_array(),
                MOTION[3u16],
            );
            MOTION[5u16] = 0;
        }
    } else {
        let mut direction: u16 = 4;
        if buttons & 32 != 0 {
            direction = 0;
        }
        if buttons & 128 != 0 {
            direction = 1;
        }
        if buttons & 64 != 0 {
            direction = 2;
        }
        if buttons & 16 != 0 {
            direction = 3;
        }
        if direction < 4 && GAME[4u16] == 0 && GAME[9u16] == 0 {
            MOTION[0u16] = GAME[0u16];
            MOTION[1u16] = GAME[1u16];
            MOTION[2u16] = GAME[2u16];
            MOTION[3u16] = direction;
            MOTION[4u16] = 0;
            MOTION[5u16] = 1;
        }
    }
}
fn block_pose(mut model: &mut Model) {
    let moving = MOTION[5u16] != 0;
    let orient = if moving { MOTION[2u16] } else { GAME[2u16] };
    let x = if moving { MOTION[0u16] } else { GAME[0u16] };
    let z = if moving { MOTION[1u16] } else { GAME[1u16] };
    let one = fix32::from_int(1);
    let two = fix32::from_int(2);
    let half = fix32::from_words(32768, 0);
    let zero = fix32::zero();
    let sx = if orient == 1 { two } else { one };
    let sy = if orient == 0 { two } else { one };
    let sz = if orient == 2 { two } else { one };
    let mut px = zero;
    let mut pz = half;
    let mut ax = zero;
    let mut az = zero;
    let mut angle = zero;
    if moving {
        angle = fix32::from_words(MOTION[4u16] * 655, 0) * fix32::from_words(37408, 1);
        if MOTION[3u16] == 0 || MOTION[3u16] == 3 {
            angle = -angle;
        }
        if MOTION[3u16] < 2 {
            px = if MOTION[3u16] == 0 { sx } else { zero };
            az = angle;
        } else {
            px = half;
            pz = if MOTION[3u16] == 2 { sz } else { zero };
            ax = angle;
        }
    } else {
        px = half;
    }
    let mut cx = sx * half;
    let mut cy = sy * half;
    let mut cz = sz * half;
    if moving {
        let sc = fsincos(angle);
        let c = sc.y();
        let s = sc.x();
        if MOTION[3u16] < 2 {
            cx = px + c * (sx * half - px) - s * sy * half;
            cy = s * (sx * half - px) + c * sy * half;
        } else {
            cy = c * sy * half - s * (sz * half - pz);
            cz = pz + s * sy * half + c * (sz * half - pz);
        }
    }
    model_pose(
        &mut model,
        vec3::new(
            cx + fix32::from_int(x as i16) - fix32::from_int(4),
            cy,
            cz + fix32::from_int(z as i16) - fix32::from_words(32768, 1),
        ),
        vec3::new(ax, zero, az),
        vec3::new(sx, sy, sz),
    );
}
fn finish(
    frame: Frame,
    mut moves: &mut TextField,
    mut connection: &mut TextField,
    connected: u16,
    number: u16,
) {
    let mut active = frame;
    // Completed 3D Passes remain unpublished when the owner returns early.
    if connected == 0 {
        return;
    }
    {
        let mut ui = frame_ui_pass(&mut active);
        pass_text(&mut ui, &mut moves);
        pass_text(&mut ui, &mut connection);
        pass_complete(ui);
    }
    signal(2, number);
    frame_complete(active);
}
fn run(limit: u16) {
    let mut renderer = renderer_open();
    let z = fix32::zero();
    let one = fix32::from_int(1);
    let mut camera = renderer_camera(
        &mut renderer,
        vec3::new(z, fix32::from_int(5), fix32::from_int(12)),
        vec3::new(fix32::from_words(32768, 65535), z, z),
        fix32::from_int(200),
        fix32::from_int(100),
    );
    let mut floor0 = renderer_mesh_batch(&mut renderer, FLOOR0_FIRST, FLOOR0_COUNT);
    let mut floor1 = renderer_mesh_batch(&mut renderer, FLOOR1_FIRST, FLOOR1_COUNT);
    let mut bridge = renderer_mesh_batch(&mut renderer, BRIDGE_FIRST, BRIDGE_COUNT);
    let mut block = renderer_mesh_batch(&mut renderer, BLOCK_FIRST, BLOCK_COUNT);
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
    let mut material = renderer_material(&mut renderer, 255, 255, 255, true);
    let mut moves = renderer_text(&mut renderer, 0, 0, 4);
    let mut connection = renderer_text(&mut renderer, 6, 0, 1);
    level_reset(0);
    let mut number: u16 = 0;
    loop {
        signal(4, number);
        let mut frame = renderer_frame(&mut renderer, 0x2222 + number);
        signal(1, number);
        let input = frame_input(&mut frame);
        let dt = frame_elapsed_ms(&mut frame);
        game_update(input.pressed, dt);
        signal(5, number);
        block_pose(&mut block_model);
        signal(6, number);
        frame_number(&mut frame, &mut moves, GAME[5u16]);
        frame_number(&mut frame, &mut connection, input.connected);
        signal(7, number);
        {
            let mut pass = frame_pass(&mut frame, &mut camera);
            if MOTION[6u16] == 0 {
                pass_draw(&mut pass, &mut floor0, &mut floor_model, &mut material);
            } else {
                pass_draw(&mut pass, &mut floor1, &mut floor_model, &mut material);
            }
            pass_complete(pass);
        }
        {
            let mut pass = frame_pass(&mut frame, &mut camera);
            if MOTION[6u16] == 0 && GAME[3u16] & 1 != 0 {
                pass_draw(&mut pass, &mut bridge, &mut floor_model, &mut material);
            }
            pass_draw(&mut pass, &mut block, &mut block_model, &mut material);
            pass_complete(pass);
        }
        signal(9, number);
        finish(frame, &mut moves, &mut connection, input.connected, number);
        signal(3, number);
        number += 1;
        if limit != 0 && number == limit {
            break;
        }
    }
    renderer_close(renderer);
}
pub fn renderer_game_frames() {
    run(22);
    halt(0x600d);
}
fn main() {
    run(0);
}
