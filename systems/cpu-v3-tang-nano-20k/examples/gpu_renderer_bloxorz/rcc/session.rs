//! Serial presentation phases over the unchanged deterministic integer rules.
use crate::dsl_rt::*;
mod gpu_pipeline;
use gpu_pipeline::*;
mod boot_resources;
use boot_resources::*;
mod bloxorz_core;
use bloxorz_core::*;
mod gamepad;
use gamepad::*;
pub static LEVEL: Buf<u16, 200> = Buf::new([0; 200]);
pub static GAME: Buf<u16, 10> = Buf::new([0; 10]);
static HISTORY: Buf<u16, 1536> = Buf::new([0; 1536]);
static HISTORY_STATE: Buf<u16, 2> = Buf::new([0; 2]);
pub static MOTION: Buf<u16, 7> = Buf::new([0; 7]);
// mode, elapsed, destination, capture-needed, captures, frozen-mode, goal x/z,
// frozen-time, first-consumer flag. Capture/read ownership remains in RenderBuffer, not these words.
pub static SESSION: Buf<u16, 10> = Buf::new([0; 10]);
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
pub fn level_reset(which: u16) {
    blox_fixture_level(which, LEVEL.as_array());
    blox_reset(LEVEL.as_array(), GAME.as_array(), HISTORY_STATE.as_array());
    MOTION[5u16] = 0;
    MOTION[6u16] = which;
    SESSION[6u16] = 65535;
    SESSION[7u16] = 65535;
    let mut i: u16 = 0;
    while i < LEVEL[6u16] {
        if LEVEL[8 + i * 8] == 6 {
            SESSION[6u16] = rem_u16(i, LEVEL[0u16]);
            SESSION[7u16] = div_u16(i, LEVEL[0u16]);
        }
        i += 1;
    }
}
pub fn session_init() {
    level_reset(0);
    SESSION[0u16] = ENTRY_OPEN;
    SESSION[1u16] = 0;
    SESSION[3u16] = 0;
    SESSION[4u16] = 0;
}
pub fn fading() -> bool {
    SESSION[0u16] >= FADE_OUT
}
pub fn visual_mode() -> u16 {
    if fading() {
        SESSION[5u16]
    } else {
        SESSION[0u16]
    }
}
pub fn visual_time() -> u16 {
    if fading() {
        SESSION[8u16]
    } else {
        SESSION[1u16]
    }
}
pub fn fraction(t: u16, duration: u16) -> fix32 {
    if t >= duration {
        return fix32::from_int(1);
    }
    fix32::from_int(t as i16) * frcp(fix32::from_int(duration as i16))
}
pub fn begin_fade(destination: u16) {
    SESSION[5u16] = SESSION[0u16];
    SESSION[8u16] = SESSION[1u16];
    SESSION[0u16] = FADE_OUT;
    SESSION[1u16] = 0;
    SESSION[2u16] = destination;
    SESSION[3u16] = 1;
}
pub fn tint() -> u16 {
    if SESSION[0u16] == FADE_OUT {
        return 255 - div_u16(SESSION[1u16] * 51, 64);
    }
    if SESSION[0u16] == HOLD {
        return 0;
    }
    if SESSION[0u16] == FADE_IN {
        return div_u16(SESSION[1u16] * 51, 76);
    }
    255
}
pub fn session_captured() {
    SESSION[3u16] = 0;
    SESSION[4u16] += 1;
    SESSION[9u16] = 1;
}
// Called only after successful consumer WAIT_ALL: the fully black frame is Front.
pub fn session_presented() {
    if SESSION[0u16] == FADE_OUT && SESSION[1u16] == 320 {
        level_reset(SESSION[2u16]);
        SESSION[0u16] = HOLD;
        SESSION[1u16] = 0;
        SESSION[3u16] = 1;
        SESSION[5u16] = ENTRY_OPEN;
        SESSION[8u16] = 0;
    } else if SESSION[0u16] == FADE_IN && SESSION[1u16] == 380 {
        // WAIT_ALL also captures input. Discard its locked-period edges before
        // entry can interpret START/TRIANGLE again; keep held-button history.
        gamepad_discard_edges();
        SESSION[0u16] = ENTRY_OPEN;
        SESSION[1u16] = 0;
    }
}
pub fn session_tick(buttons: u16, elapsed: u16) {
    let mode = SESSION[0u16];
    let dt = if elapsed > 50 { 50u16 } else { elapsed };
    if mode < FADE_OUT {
        if buttons & 4096 != 0 {
            begin_fade(MOTION[6u16] ^ 1);
            return;
        }
        if buttons & 8 != 0 {
            begin_fade(MOTION[6u16]);
            return;
        }
    }
    if SESSION[9u16] != 0 {
        SESSION[9u16] = 0;
        return;
    }
    if mode == PLAY {
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
            SESSION[0u16] = ROLL;
            SESSION[1u16] = 0;
        }
        return;
    }
    if SESSION[3u16] != 0 {
        return;
    }
    SESSION[1u16] += dt;
    if mode == ROLL {
        MOTION[4u16] = SESSION[1u16];
        if SESSION[1u16] >= 220 {
            blox_dispatch(
                LEVEL.as_array(),
                GAME.as_array(),
                HISTORY.as_array(),
                HISTORY_STATE.as_array(),
                MOTION[3u16],
            );
            MOTION[5u16] = 0;
            SESSION[1u16] = 0;
            SESSION[0u16] = if GAME[4u16] == 1 {
                GOAL_OPEN
            } else {
                if GAME[4u16] == 2 {
                    FALL
                } else {
                    PLAY
                }
            };
        }
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
        begin_fade(MOTION[6u16] ^ 1);
    } else if mode == FALL && SESSION[1u16] >= 600 {
        SESSION[1u16] = 600;
        begin_fade(MOTION[6u16]);
    } else if mode == FADE_OUT && SESSION[1u16] > 320 {
        SESSION[1u16] = 320;
    } else if mode == HOLD && SESSION[1u16] >= 80 {
        SESSION[0u16] = FADE_IN;
        SESSION[1u16] = 0;
    } else if mode == FADE_IN && SESSION[1u16] > 380 {
        SESSION[1u16] = 380;
    }
}
pub fn block_visible() -> bool {
    let mode = visual_mode();
    mode != ENTRY_OPEN && !(mode == GOAL_CLOSE && visual_time() >= 200)
}
pub fn block_pose(model: &mut Model) {
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
        angle = fraction(MOTION[4u16], 220) * fix32::from_words(37408, 1);
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
    let mode = visual_mode();
    let t = visual_time();
    let mut vertical = zero;
    let mut squeeze = one;
    if mode == ENTRY_RISE {
        let f = fraction(t, 380);
        let remaining = one - f;
        vertical = -fix32::from_words(5243, 2)
            * (remaining * fix32::from_words(26214, 0)
                + remaining * remaining * fix32::from_words(39322, 0));
        squeeze = one - fix32::from_words(5243, 0) * remaining;
    }
    if mode == GOAL_DROP || mode == GOAL_LAND || mode == GOAL_CLOSE {
        let f = if mode == GOAL_DROP {
            fraction(t, 420)
        } else {
            one
        };
        vertical = -fix32::from_words(5243, 2)
            * (f * fix32::from_words(26214, 0) + f * f * fix32::from_words(39322, 0));
        squeeze = one - fix32::from_words(5243, 0) * f;
    }
    if mode == FALL {
        let f = fraction(t, 600);
        vertical = -fix32::from_int(4) * f * f;
    }
    model_pose(
        model,
        vec3::new(
            cx + fix32::from_int(x as i16) - fix32::from_int(4),
            cy + vertical,
            cz + fix32::from_int(z as i16) - fix32::from_words(32768, 1),
        ),
        vec3::new(ax, zero, az),
        vec3::new(sx * squeeze, sy, sz * squeeze),
    );
}
