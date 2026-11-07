//! All-stage rules, input and independent animation/veil clocks.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
use gpu_demo_api::*;
mod boot_resources;
use boot_resources::*;
mod bloxorz_core;
use bloxorz_core::*;
mod gamepad;
use gamepad::*;
mod orientation;
use orientation::*;
pub static LEVEL_GENERATION: Buf<u16, 1> = Buf::new([0]);
pub static LEVEL: Buf<u16, 308> = Buf::new([0; 308]);
pub static GAME: Buf<u16, 10> = Buf::new([0; 10]);
// Shared rule engine scratch; the app packs every valid pre-action PLAY state
// into32 bits (whole/split, two coordinates, active cube, nine bridge channels).
static HISTORY: Buf<u16, 6> = Buf::new([0; 6]);
static HISTORY_STATE: Buf<u16, 2> = Buf::new([0; 2]);
static UNDO: Buf<u16, 512> = Buf::new([0; 512]);
static UNDO_STATE: Buf<u16, 2> = Buf::new([0; 2]);
// from x/z/orient, direction, roll/mask elapsed, moving, level,
// old mask, fall kind(0 release/1 tip), release threshold, first support.
//11..16 retain split/active, second cube and the pre-teleport roll landing.
pub static MOTION: Buf<u16, 17> = Buf::new([0; 17]);
pub static FRACTURE: Buf<u16, 1> = Buf::new([65535]);
// animation/time, destination, held lock, source-render count, fall notified,
// goal x/z, most-recent-direction stack. Rendering completion owns resets.
pub static SESSION: Buf<u16, 9> = Buf::new([0; 9]);
// Previous visual mask and the nine channel press ages.
static SWITCH_FX: Buf<u16, 10> = Buf::new([0, 260, 260, 260, 260, 260, 260, 260, 260, 260]);
pub static TRANSITION: Buf<u16, 2> = Buf::new([0; 2]);
pub const PLAY: u16 = 0;
pub const ROLL: u16 = 1;
pub const ENTRY_OPEN: u16 = 2;
pub const ENTRY_RISE: u16 = 3;
pub const GOAL_OPEN: u16 = 4;
pub const GOAL_DROP: u16 = 5;
pub const GOAL_LAND: u16 = 6;
pub const GOAL_CLOSE: u16 = 7;
pub const FALL: u16 = 8;
pub const FADE_OUT: u16 = 9;
pub const HOLD: u16 = 10;
pub const FADE_IN: u16 = 11;
pub const TIP: u16 = 12;
pub const SPLIT_POP: u16 = 13;
pub const CRACK: u16 = 14;
#[derive(Copy, Clone)]
struct FallState {
    position: Vec3,
    velocity: Vec3,
    scale: Vec3,
    angle: fix32,
    spin: fix32,
    axis: u16,
}
static FALL_STATE: Buf<FallState, 1> = Buf::new([FallState {
    position: Vec3 {
        x: fix32::zero(),
        y: fix32::zero(),
        z: fix32::zero(),
    },
    velocity: Vec3 {
        x: fix32::zero(),
        y: fix32::zero(),
        z: fix32::zero(),
    },
    scale: Vec3 {
        x: fix32::zero(),
        y: fix32::zero(),
        z: fix32::zero(),
    },
    angle: fix32::zero(),
    spin: fix32::zero(),
    axis: 0,
}]);
pub fn level_field(index: u16, field: u16) -> u16 {
    blox_field(LEVEL.as_array(), index + 1, field)
}
pub fn level_reset(which: u16) {
    rotation_reset();
    LEVEL_GENERATION[0u16] += 1;
    FRACTURE[0u16] = 65535;
    MOTION[11u16] = 0;
    blox_fixture_level(which, LEVEL.as_array());
    blox_reset(LEVEL.as_array(), GAME.as_array(), HISTORY_STATE.as_array());
    UNDO_STATE[0u16] = 0;
    UNDO_STATE[1u16] = 0;
    SWITCH_FX[0u16] = GAME[3u16];
    SWITCH_FX[1u16] = 260;
    let mut channel: u16 = 1;
    while channel <= 9 {
        SWITCH_FX[channel] = 260;
        channel += 1;
    }
    MOTION[4u16] = 220;
    MOTION[5u16] = 0;
    MOTION[6u16] = which;
    SESSION[5u16] = 0;
    SESSION[6u16] = 65535;
    SESSION[7u16] = 65535;
    let mut i: u16 = 0;
    while i < LEVEL[6u16] {
        if level_field(i, 0) == 6 {
            SESSION[6u16] = i % LEVEL[0u16];
            SESSION[7u16] = div_u16(i, LEVEL[0u16]);
        }
        i += 1;
    }
}
pub fn session_init() {
    rotation_begin();
    level_reset(BLOXORZ_INITIAL_LEVEL);
    SESSION[0u16] = ENTRY_OPEN;
    if BLOXORZ_INITIAL_PLAY != 0 {
        SESSION[0u16] = PLAY;
    }
    SESSION[1u16] = 0;
    SESSION[3u16] = 0;
    SESSION[4u16] = 0;
    SESSION[8u16] = 0;
    TRANSITION[0u16] = 0;
    TRANSITION[1u16] = 0;
}
pub fn session_end() {
    rotation_end();
}
pub fn fading() -> bool {
    TRANSITION[0u16] != 0
}
pub fn visual_mode() -> u16 {
    SESSION[0u16]
}
pub fn visual_time() -> u16 {
    SESSION[1u16]
}
pub fn visual_mask() -> u16 {
    if MOTION[4u16] < 154 {
        MOTION[7u16]
    } else {
        GAME[3u16]
    }
}
fn ratio(t: u16, duration: u16) -> fix32 {
    // Exact power-of-two normalization keeps the reciprocal in a useful range.
    fix32::from_words(t << 8, t >> 8) * frcp(fix32::from_words(duration << 8, duration >> 8))
}
pub fn fraction(t: u16, duration: u16) -> fix32 {
    if t >= duration {
        fix32::from_int(1)
    } else {
        ratio(t, duration)
    }
}
pub fn begin_fade(destination: u16) {
    if fading() {
        return;
    }
    SESSION[2u16] = destination;
    SESSION[3u16] = 1;
    SESSION[8u16] = 0;
    TRANSITION[0u16] = FADE_OUT;
    TRANSITION[1u16] = 0;
}
pub fn tint() -> u16 {
    if TRANSITION[0u16] == FADE_OUT {
        return 255 - div_u16(TRANSITION[1u16] * 51, 64);
    }
    if TRANSITION[0u16] == HOLD {
        return 0;
    }
    if TRANSITION[0u16] == FADE_IN {
        return div_u16(TRANSITION[1u16] * 51, 76);
    }
    255
}
pub fn session_captured() {
    SESSION[4u16] += 1;
}
// Invoked after consumer WAIT_ALL and actual Display retirement.
pub fn session_presented() {
    if TRANSITION[0u16] == FADE_OUT && TRANSITION[1u16] == 320 {
        level_reset(SESSION[2u16]);
        SESSION[0u16] = ENTRY_OPEN;
        SESSION[1u16] = 0;
        TRANSITION[0u16] = HOLD;
        TRANSITION[1u16] = 0;
    } else if TRANSITION[0u16] == FADE_IN && TRANSITION[1u16] == 380 {
        TRANSITION[0u16] = 0;
        TRANSITION[1u16] = 0;
        SESSION[0u16] = ENTRY_OPEN;
        SESSION[1u16] = 0;
        SESSION[8u16] = 0;
        gamepad_discard_edges();
        let input = gamepad_take_frame();
        SESSION[3u16] = if input.buttons & 240 != 0 { 1u16 } else { 0u16 };
    }
}
fn presentation_support(x: i16, z: i16, mask: u16) -> bool {
    if x < 0 || z < 0 || x as u16 >= LEVEL[0u16] || z as u16 >= LEVEL[1u16] {
        return false;
    }
    let at = (z as u16) * LEVEL[0u16] + x as u16;
    let kind = level_field(at, 0);
    kind != 0 && !(kind == 5 && mask & level_field(at, 1) == 0)
}
pub fn session_move(direction: u16) -> bool {
    if fading() || SESSION[0u16] != PLAY || (direction >= 4 && direction != 6) {
        return false;
    }
    MOTION[11u16] = GAME[9u16];
    MOTION[12u16] = GAME[8u16];
    MOTION[13u16] = GAME[6u16];
    MOTION[14u16] = GAME[7u16];
    MOTION[0u16] = GAME[0u16];
    MOTION[1u16] = GAME[1u16];
    MOTION[2u16] = GAME[2u16];
    MOTION[3u16] = direction;
    MOTION[7u16] = GAME[3u16];
    HISTORY_STATE[0u16] = 0;
    HISTORY_STATE[1u16] = 0;
    if !blox_dispatch(
        LEVEL.as_array(),
        GAME.as_array(),
        HISTORY.as_array(),
        HISTORY_STATE.as_array(),
        direction,
    ) {
        return false;
    }
    let mode = if HISTORY[5u16] & 32768 != 0 {
        3 + ((HISTORY[5u16] >> 14) & 1)
    } else {
        HISTORY[2u16] & 3
    };
    let mask = (HISTORY[2u16] >> 2) & 511;
    rotation_save(UNDO_STATE[0u16]);
    let at = UNDO_STATE[0u16] * 2;
    UNDO[at] =
        (HISTORY[0u16] & 31) | ((HISTORY[1u16] & 31) << 5) | (mode << 10) | ((mask & 7) << 13);
    UNDO[at + 1] = (mask >> 3)
        | ((HISTORY[4u16] & 31) << 6)
        | ((((HISTORY[4u16] >> 15) | (HISTORY[5u16] << 1)) & 31) << 11);
    UNDO_STATE[0u16] = (UNDO_STATE[0u16] + 1) & 255;
    if UNDO_STATE[1u16] < 256 {
        UNDO_STATE[1u16] += 1;
    }
    if direction == 6 {
        MOTION[4u16] = 220;
        MOTION[5u16] = 0;
        if GAME[4u16] == 2 {
            // A soft-switch swap can remove the other cube's bridge. Preserve
            // the supported cube and drop the actual unsupported one at rest.
            let first = presentation_support(GAME[0u16] as i16, GAME[1u16] as i16, GAME[3u16]);
            let falling = if first { 1u16 } else { 0u16 };
            let mut pose = block_transform();
            if falling != GAME[8u16] {
                pose = idle_cube_transform();
            }
            MOTION[12u16] = falling;
            rotation_start(falling);
            FALL_STATE[0u16] = FallState {
                position: pose.position,
                velocity: demo_zero(),
                scale: pose.scale,
                angle: fix32::zero(),
                spin: fix32::zero(),
                axis: 0,
            };
            SESSION[0u16] = FALL;
            SESSION[1u16] = 0;
            SESSION[5u16] = 0;
            SESSION[3u16] = 1;
            SESSION[8u16] = 0;
        }
        return true;
    }
    rotation_start(if MOTION[11u16] != 0 {
        MOTION[12u16]
    } else {
        0u16
    });
    SESSION[0u16] = ROLL;
    SESSION[1u16] = 0;
    SESSION[5u16] = 0;
    MOTION[4u16] = 0;
    MOTION[5u16] = 1;
    MOTION[8u16] = 0;
    MOTION[9u16] = 79;
    if MOTION[2u16] != 0 {
        MOTION[9u16] = 124;
        if (MOTION[2u16] == 1 && direction < 2) || (MOTION[2u16] == 2 && direction >= 2) {
            MOTION[9u16] = 169;
        }
    }
    if MOTION[11u16] != 0 {
        MOTION[9u16] = 124;
    }
    if GAME[4u16] == 2 && GAME[9u16] == 0 && GAME[2u16] == 0 {
        let x = GAME[0u16];
        let z = GAME[1u16];
        if x < LEVEL[0u16] && z < LEVEL[1u16] && level_field(z * LEVEL[0u16] + x, 0) == 2 {
            MOTION[8u16] = 2;
        }
    }
    if MOTION[11u16] == 0 && GAME[9u16] != 0 {
        MOTION[8u16] = 3;
    }
    if GAME[4u16] == 2 && GAME[2u16] != 0 && MOTION[11u16] == 0 {
        let x = GAME[0u16] as i16;
        let z = GAME[1u16] as i16;
        let first = presentation_support(x, z, GAME[3u16]);
        let second = presentation_support(
            x + if GAME[2u16] == 1 { 1i16 } else { 0i16 },
            z + if GAME[2u16] == 2 { 1i16 } else { 0i16 },
            GAME[3u16],
        );
        if (first && !second) || (!first && second) {
            MOTION[8u16] = 1;
            MOTION[10u16] = if first { 1u16 } else { 0u16 };
        }
    }
    if GAME[4u16] != 0 {
        SESSION[3u16] = 1;
        SESSION[8u16] = 0;
    }
    true
}
fn session_undo() {
    SESSION[3u16] = 1;
    SESSION[8u16] = 0;
    if UNDO_STATE[1u16] == 0 {
        return;
    }
    UNDO_STATE[0u16] = (UNDO_STATE[0u16] + 255) & 255;
    UNDO_STATE[1u16] -= 1;
    rotation_undo(UNDO_STATE[0u16]);
    let at = UNDO_STATE[0u16] * 2;
    let first = UNDO[at];
    let second = UNDO[at + 1];
    let mode = (first >> 10) & 7;
    GAME[0u16] = first & 31;
    GAME[1u16] = (first >> 5) & 31;
    GAME[2u16] = if mode < 3 { mode } else { 0u16 };
    GAME[3u16] = (first >> 13) | ((second & 63) << 3);
    GAME[4u16] = 0;
    GAME[5u16] -= 1;
    GAME[6u16] = (second >> 6) & 31;
    GAME[7u16] = (second >> 11) & 31;
    GAME[8u16] = if mode >= 3 { mode - 3 } else { 0u16 };
    GAME[9u16] = if mode >= 3 { 1u16 } else { 0u16 };
    FRACTURE[0u16] = 65535;
    MOTION[11u16] = 0;
    SESSION[0u16] = PLAY;
    SESSION[1u16] = 0;
    SESSION[5u16] = 0;
    MOTION[4u16] = 220;
    MOTION[5u16] = 0;
}
fn direction_button(direction: u16) -> u16 {
    if direction == 0 {
        32
    } else {
        if direction == 1 {
            128
        } else {
            if direction == 2 {
                64
            } else {
                16
            }
        }
    }
}
fn held_update(pressed: u16, held: u16) {
    if fading() {
        SESSION[3u16] = 1;
        SESSION[8u16] = 0;
        return;
    }
    if SESSION[3u16] != 0 {
        if held & 240 == 0 {
            SESSION[3u16] = 0;
        } else {
            return;
        }
    }
    // Remove released keys and old copies of any re-pressed key before pushing.
    // A release/re-press captured within one frame must not evict another held key.
    let mut stack = SESSION[8u16];
    let mut compact: u16 = 0;
    let mut shift: u16 = 0;
    let mut seen: u16 = 0;
    while stack != 0 {
        let code = stack & 15;
        let bit = direction_button(code - 1);
        if held & bit != 0 && pressed & bit == 0 && seen & bit == 0 {
            compact |= code << shift;
            shift += 4;
            seen |= bit;
        }
        stack >>= 4;
    }
    SESSION[8u16] = compact;
    if pressed & 240 != 0 {
        let mut i: u16 = 0;
        while i < 4 {
            let bit = direction_button(i);
            if pressed & held & bit != 0 {
                SESSION[8u16] = (SESSION[8u16] << 4) | (i + 1);
            }
            i += 1;
        }
    }
}

pub fn session_tick(pressed: u16, held: u16, elapsed: u16) {
    let dt = if elapsed > 50 { 50u16 } else { elapsed };
    held_update(pressed, held);
    if !fading() {
        if pressed & 1024 != 0 {
            begin_fade((MOTION[6u16] + BLOX_LEVEL_COUNT - 1) % BLOX_LEVEL_COUNT);
        } else if pressed & (4096 | 2048) != 0 {
            begin_fade((MOTION[6u16] + 1) % BLOX_LEVEL_COUNT);
        } else if pressed & 8 != 0 {
            begin_fade(MOTION[6u16]);
        } else if pressed & 32768 != 0 {
            session_undo();
        } else if pressed & 16384 != 0 {
            session_move(6);
        }
    }
    let phase = TRANSITION[0u16];
    if phase == 0 || phase == FADE_OUT {
        animation_tick(dt);
    }
    if TRANSITION[0u16] == FADE_OUT {
        TRANSITION[1u16] += dt;
        if TRANSITION[1u16] > 320 {
            TRANSITION[1u16] = 320;
        }
    } else if TRANSITION[0u16] == HOLD {
        TRANSITION[1u16] += dt;
        if TRANSITION[1u16] >= 80 {
            TRANSITION[0u16] = FADE_IN;
            TRANSITION[1u16] = 0;
        }
    } else if TRANSITION[0u16] == FADE_IN {
        TRANSITION[1u16] += dt;
        if TRANSITION[1u16] > 380 {
            TRANSITION[1u16] = 380;
        }
    }
    switch_tick(dt);
    if !fading() && SESSION[0u16] == PLAY && SESSION[3u16] == 0 && SESSION[8u16] != 0 {
        session_move((SESSION[8u16] & 15) - 1);
    }
}
pub fn switch_age(channel: u16) -> u16 {
    SWITCH_FX[channel + 1]
}
pub fn switch_fraction(channel: u16) -> fix32 {
    fraction(SWITCH_FX[channel + 1], 260)
}
fn switch_tick(dt: u16) {
    let mask = visual_mask();
    let old = SWITCH_FX[0u16];
    let mut channel: u16 = 0;
    while channel < 9 {
        let bit = 1 << channel;
        if mask & bit != 0 {
            if old & bit == 0 {
                SWITCH_FX[channel + 1] = 0;
            }
            SWITCH_FX[channel + 1] += dt;
            if SWITCH_FX[channel + 1] > 260 {
                SWITCH_FX[channel + 1] = 260;
            }
        } else {
            SWITCH_FX[channel + 1] = 260;
        }
        channel += 1;
    }
    SWITCH_FX[0u16] = mask;
}
fn animation_tick(dt: u16) {
    let mode = SESSION[0u16];
    if mode == PLAY {
        return;
    }
    if mode != ROLL && MOTION[4u16] < 154 {
        MOTION[4u16] += dt;
        if MOTION[4u16] > 154 {
            MOTION[4u16] = 154;
        }
    }
    SESSION[1u16] += dt;
    if mode == ROLL {
        if SESSION[1u16] > 220 {
            SESSION[1u16] = 220;
        }
        MOTION[4u16] = SESSION[1u16];
        if GAME[4u16] == 2 && MOTION[8u16] == 0 && SESSION[1u16] >= MOTION[9u16] {
            release_pivot();
        } else if SESSION[1u16] == 220 {
            rotation_finish(
                MOTION[3u16],
                MOTION[11u16] != 0,
                GAME[9u16] != 0,
                MOTION[12u16],
            );
            if MOTION[11u16] == 0 && GAME[9u16] != 0 {
                let landing = block_transform();
                let half = fix32::from_words(32768, 0);
                MOTION[15u16] = (landing.position.x + fix32::from_int(LEVEL[0u16] as i16) * half
                    - fix32::from_words(16384, 0))
                .to_int() as u16;
                MOTION[16u16] = (landing.position.z + fix32::from_int(LEVEL[1u16] as i16) * half
                    - fix32::from_words(16384, 0))
                .to_int() as u16;
                SESSION[0u16] = SPLIT_POP;
            } else if GAME[4u16] == 1 {
                SESSION[0u16] = GOAL_OPEN;
            } else if GAME[4u16] == 2 {
                if MOTION[8u16] == 2 {
                    SESSION[0u16] = CRACK;
                    FRACTURE[0u16] = GAME[1u16] * LEVEL[0u16] + GAME[0u16];
                } else {
                    SESSION[0u16] = TIP;
                }
            } else {
                SESSION[0u16] = PLAY;
            }
            MOTION[5u16] = 0;
            SESSION[1u16] = 0;
        }
    } else if mode == SPLIT_POP && SESSION[1u16] >= 280 {
        SESSION[0u16] = PLAY;
        SESSION[1u16] = 0;
    } else if mode == CRACK && SESSION[1u16] >= 140 {
        let pose = block_transform();
        FALL_STATE[0u16] = FallState {
            position: pose.position,
            velocity: demo_zero(),
            scale: pose.scale,
            angle: fix32::zero(),
            spin: fix32::zero(),
            axis: 0,
        };
        SESSION[0u16] = FALL;
        SESSION[1u16] = 0;
    } else if mode == TIP && SESSION[1u16] >= 360 {
        SESSION[1u16] = 360;
        release_pivot();
    } else if mode == ENTRY_OPEN && SESSION[1u16] >= 200 {
        SESSION[0u16] = ENTRY_RISE;
        SESSION[1u16] = 0;
    } else if mode == ENTRY_RISE && SESSION[1u16] >= 380 {
        SESSION[0u16] = PLAY;
        SESSION[1u16] = 0;
    } else if mode == GOAL_OPEN && SESSION[1u16] >= 200 {
        SESSION[0u16] = GOAL_DROP;
        SESSION[1u16] = 0;
    } else if mode == GOAL_DROP && SESSION[1u16] >= 420 {
        SESSION[0u16] = GOAL_LAND;
        SESSION[1u16] = 0;
    } else if mode == GOAL_LAND && SESSION[1u16] >= 80 {
        SESSION[0u16] = GOAL_CLOSE;
        SESSION[1u16] = 0;
    } else if mode == GOAL_CLOSE && SESSION[1u16] >= 200 {
        SESSION[1u16] = 200;
        if !fading() {
            begin_fade((MOTION[6u16] + 1) % BLOX_LEVEL_COUNT);
        }
    } else if mode == FALL && SESSION[1u16] >= 600 && SESSION[5u16] == 0 {
        SESSION[5u16] = 1;
        begin_fade(MOTION[6u16]);
    }
}
pub fn block_visible() -> bool {
    SESSION[0u16] != ENTRY_OPEN && !(SESSION[0u16] == GOAL_CLOSE && SESSION[1u16] >= 200)
}
fn release_pivot() {
    let pose = block_transform();
    let half = fix32::from_words(32768, 0);
    let tipping = SESSION[0u16] == TIP;
    let axis = if tipping {
        if GAME[2u16] == 1 {
            1u16
        } else {
            0u16
        }
    } else {
        if MOTION[3u16] < 2 {
            1u16
        } else {
            0u16
        }
    };
    let mut spin = fix32::from_words(9174, 7);
    if tipping {
        spin = fix32::from_words(47621, 8);
        if (axis == 1 && MOTION[10u16] != 0) || (axis == 0 && MOTION[10u16] == 0) {
            spin = -spin;
        }
    } else if MOTION[3u16] == 0 || MOTION[3u16] == 3 {
        spin = -spin;
    }
    let mut pivot = fix32::zero();
    if axis == 1 {
        if tipping {
            pivot = fix32::from_int(GAME[0u16] as i16) + fix32::from_int(1);
        } else {
            pivot = fix32::from_int(motion_x() as i16);
            if MOTION[3u16] == 0 {
                pivot = pivot
                    + if MOTION[2u16] == 1 {
                        fix32::from_int(2)
                    } else {
                        fix32::from_int(1)
                    };
            }
        }
        pivot = pivot - fix32::from_int(LEVEL[0u16] as i16) * half;
    } else {
        if tipping {
            pivot = fix32::from_int(GAME[1u16] as i16) + fix32::from_int(1);
        } else {
            pivot = fix32::from_int(motion_z() as i16);
            if MOTION[3u16] == 2 {
                pivot = pivot
                    + if MOTION[2u16] == 2 {
                        fix32::from_int(2)
                    } else {
                        fix32::from_int(1)
                    };
            }
        }
        pivot = pivot - fix32::from_int(LEVEL[1u16] as i16) * half;
    }
    let mut velocity = demo_zero();
    if axis == 1 {
        velocity = demo_point(
            -spin * pose.position.y,
            spin * (pose.position.x - pivot),
            fix32::zero(),
        );
    } else {
        velocity = demo_point(
            fix32::zero(),
            -spin * (pose.position.z - pivot),
            spin * pose.position.y,
        );
    }
    FALL_STATE[0u16] = FallState {
        position: pose.position,
        velocity: velocity,
        scale: pose.scale,
        angle: if axis == 1 {
            pose.angles.z
        } else {
            pose.angles.x
        },
        spin: spin,
        axis: axis,
    };
    MOTION[5u16] = 0;
    SESSION[0u16] = FALL;
    SESSION[1u16] = 0;
}
fn motion_x() -> u16 {
    if MOTION[11u16] != 0 && MOTION[12u16] != 0 {
        MOTION[13u16]
    } else {
        MOTION[0u16]
    }
}
fn motion_z() -> u16 {
    if MOTION[11u16] != 0 && MOTION[12u16] != 0 {
        MOTION[14u16]
    } else {
        MOTION[1u16]
    }
}
pub fn split_visual() -> bool {
    if SESSION[0u16] == SPLIT_POP {
        return SESSION[1u16] >= 70;
    }
    if SESSION[0u16] == ROLL {
        return MOTION[11u16] != 0;
    }
    if SESSION[0u16] == FALL && MOTION[11u16] != 0 {
        return true;
    }
    GAME[9u16] != 0
}
fn split_pop_scale() -> fix32 {
    if SESSION[0u16] != SPLIT_POP {
        return fix32::from_int(1);
    }
    let pop =
        (fraction(SESSION[1u16], 280) - fix32::from_words(13107, 0)) * fix32::from_words(16384, 1);
    let remain = pop - fix32::from_int(1);
    let back = fix32::from_int(1)
        + fix32::from_words(45875, 2) * remain * remain * remain
        + fix32::from_words(45875, 1) * remain * remain;
    if back < fix32::from_words(655, 0) {
        fix32::from_words(655, 0)
    } else {
        back
    }
}
pub fn idle_cube_transform() -> Transform {
    let old = (SESSION[0u16] == ROLL || SESSION[0u16] == FALL) && MOTION[11u16] != 0;
    let active = if old { MOTION[12u16] } else { GAME[8u16] };
    let mut x = if active == 0 { GAME[6u16] } else { GAME[0u16] };
    let mut z = if active == 0 { GAME[7u16] } else { GAME[1u16] };
    if old {
        x = if active == 0 {
            MOTION[13u16]
        } else {
            MOTION[0u16]
        };
        z = if active == 0 {
            MOTION[14u16]
        } else {
            MOTION[1u16]
        };
    }
    let half = fix32::from_words(32768, 0);
    let scale = split_pop_scale();
    demo_pose(
        demo_point(
            fix32::from_int(x as i16) - fix32::from_int(LEVEL[0u16] as i16) * half + half,
            half,
            fix32::from_int(z as i16) - fix32::from_int(LEVEL[1u16] as i16) * half + half,
        ),
        demo_zero(),
        demo_point(scale, scale, scale),
    )
}
pub fn idle_cube_pose(model: &mut Model) {
    let old = (SESSION[0u16] == ROLL || SESSION[0u16] == FALL) && MOTION[11u16] != 0;
    let active = if old { MOTION[12u16] } else { GAME[8u16] };
    rotation_pose(model, idle_cube_transform(), rotation_current(1 - active));
}
pub fn fragile_dropped(index: u16) -> bool {
    FRACTURE[0u16] == index && (SESSION[0u16] == CRACK || SESSION[0u16] == FALL)
}
pub fn fragile_height(index: u16) -> fix32 {
    if FRACTURE[0u16] != index {
        return fix32::zero();
    }
    if SESSION[0u16] == CRACK {
        let f = fraction(SESSION[1u16], 140);
        let sc = fsincos(f * fix32::from_int(60));
        return sc.x() * fix32::from_words(786, 0) * (fix32::from_int(1) - f);
    }
    if SESSION[0u16] == FALL {
        let f = ratio(SESSION[1u16], 600);
        return -fix32::from_int(16) * f * f;
    }
    fix32::zero()
}
pub fn block_transform() -> Transform {
    let mode = SESSION[0u16];
    let t = SESSION[1u16];
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    let two = fix32::from_int(2);
    let half = fix32::from_words(32768, 0);
    if mode == FALL {
        let state = FALL_STATE[0u16];
        let seconds = ratio(t, 1000);
        let f = ratio(t, 600);
        let angle = state.angle + state.spin * seconds;
        return demo_pose(
            demo_point(
                state.position.x + state.velocity.x * seconds,
                state.position.y + state.velocity.y * seconds - fix32::from_int(16) * f * f,
                state.position.z + state.velocity.z * seconds,
            ),
            demo_point(
                if state.axis == 0 { angle } else { zero },
                zero,
                if state.axis == 1 { angle } else { zero },
            ),
            state.scale,
        );
    }
    if mode == SPLIT_POP && t < 70 {
        let size = one - fraction(t, 70);
        return demo_pose(
            demo_point(
                fix32::from_int(MOTION[15u16] as i16) - fix32::from_int(LEVEL[0u16] as i16) * half
                    + half,
                size,
                fix32::from_int(MOTION[16u16] as i16) - fix32::from_int(LEVEL[1u16] as i16) * half
                    + half,
            ),
            demo_zero(),
            demo_point(one, two * size, one),
        );
    }
    let moving = MOTION[5u16] != 0;
    let split = split_visual();
    let orient = if moving { MOTION[2u16] } else { GAME[2u16] };
    let x = if moving {
        motion_x()
    } else {
        if split && GAME[8u16] != 0 {
            GAME[6u16]
        } else {
            GAME[0u16]
        }
    };
    let z = if moving {
        motion_z()
    } else {
        if split && GAME[8u16] != 0 {
            GAME[7u16]
        } else {
            GAME[1u16]
        }
    };
    let sx = if !split && orient == 1 { two } else { one };
    let sy = if !split && orient == 0 { two } else { one };
    let sz = if !split && orient == 2 { two } else { one };
    let mut cx = sx * half;
    let mut cy = sy * half;
    let mut cz = sz * half;
    let mut ax = zero;
    let mut az = zero;
    let mut angle = zero;
    let mut axis: u16 = 0;
    let mut pivot = zero;
    if moving {
        angle = fraction(MOTION[4u16], 220) * fix32::from_words(37408, 1);
        if MOTION[3u16] == 0 || MOTION[3u16] == 3 {
            angle = -angle;
        }
        if MOTION[3u16] < 2 {
            axis = 1;
            pivot = if MOTION[3u16] == 0 { sx } else { zero };
        } else {
            pivot = if MOTION[3u16] == 2 { sz } else { zero };
        }
    } else if mode == TIP {
        let f = fraction(t, 360);
        angle = fix32::from_words(37408, 1) * f * f;
        if orient == 1 {
            axis = 1;
            pivot = one;
            if MOTION[10u16] != 0 {
                angle = -angle;
            }
        } else {
            pivot = one;
            if MOTION[10u16] == 0 {
                angle = -angle;
            }
        }
    }
    if moving || mode == TIP {
        let sc = fsincos(angle);
        let c = sc.y();
        let s = sc.x();
        if axis == 1 {
            az = angle;
            cx = pivot + c * (sx * half - pivot) - s * sy * half;
            cy = s * (sx * half - pivot) + c * sy * half;
        } else {
            ax = angle;
            cy = c * sy * half - s * (sz * half - pivot);
            cz = pivot + s * sy * half + c * (sz * half - pivot);
        }
    }
    let mut vertical = zero;
    let mut squeeze = one;
    if mode == ENTRY_RISE {
        let remain = one - fraction(t, 380);
        let e =
            remain * fix32::from_words(26214, 0) + remain * remain * fix32::from_words(39322, 0);
        vertical = -fix32::from_words(5243, 2) * e;
        squeeze = one - fix32::from_words(6554, 0) * e;
    }
    if mode == GOAL_DROP || mode == GOAL_LAND || mode == GOAL_CLOSE {
        let f = if mode == GOAL_DROP {
            fraction(t, 420)
        } else {
            one
        };
        vertical = -fix32::from_words(5243, 2)
            * (f * fix32::from_words(26214, 0) + f * f * fix32::from_words(39322, 0));
        squeeze = one - fix32::from_words(6554, 0) * f;
    }
    demo_pose(
        demo_point(
            cx + fix32::from_int(x as i16) - fix32::from_int(LEVEL[0u16] as i16) * half,
            cy + vertical,
            cz + fix32::from_int(z as i16) - fix32::from_int(LEVEL[1u16] as i16) * half,
        ),
        demo_point(ax, zero, az),
        demo_point(
            sx * squeeze * split_pop_scale(),
            sy * split_pop_scale(),
            sz * squeeze * split_pop_scale(),
        ),
    )
}
pub fn block_pose(model: &mut Model) {
    rotation_pose(model, block_transform(), block_orientation());
}
pub fn block_orientation() -> u16 {
    if MOTION[5u16] != 0 || SESSION[0u16] == FALL {
        rotation_animation()
    } else {
        rotation_current(if split_visual() { GAME[8u16] } else { 0u16 })
    }
}
