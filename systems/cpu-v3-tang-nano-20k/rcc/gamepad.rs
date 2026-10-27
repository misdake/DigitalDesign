//! Single-consumer game input: bounded capture, cached sticks, explicit edges.
use crate::dsl_rt::*;
mod dualshock2;
use dualshock2::*;
mod stick;
use stick::*;

pub struct GamepadFrame {
    pub connected: u16,
    pub id: u16,
    pub buttons: u16,
    pub pressed: u16,
    pub released: u16,
    pub left_x: fix32,
    pub left_y: fix32,
    pub right_x: fix32,
    pub right_y: fix32,
}
struct GamepadStick {
    prepared: StickPrepared,
    x: fix32,
    y: fix32,
    dir_x: fix32,
    dir_y: fix32,
    strength: fix32,
    valid: u16,
    raw_pair: u16,
    key_state: u16,
}
static GAMEPAD_STATE: Buf<u16, 10> = Buf::new([0; 10]);
static GAMEPAD_RX: Buf<u16, 9> = Buf::new([0; 9]);
// Bit0 initialized; bits1/2 mean left/right needs resolution before consumption.
static GAMEPAD_READY: u16 = 0;
static GAMEPAD_STICKS: Buf<GamepadStick, 2> = Buf::new(
    [GamepadStick {
        prepared: StickPrepared {
            min_x: 0,
            center_x: 0,
            max_x: 0,
            min_y: 0,
            center_y: 0,
            max_y: 0,
            negative_x: fix32::zero(),
            positive_x: fix32::zero(),
            negative_y: fix32::zero(),
            positive_y: fix32::zero(),
            inner: fix32::zero(),
            inverse_range: fix32::zero(),
            valid: 0,
        },
        x: fix32::zero(),
        y: fix32::zero(),
        dir_x: fix32::zero(),
        dir_y: fix32::zero(),
        strength: fix32::zero(),
        valid: 0,
        raw_pair: 0,
        key_state: 0,
    }; 2],
);

// Only a pending consumer enters this numerical/install path. Captures can
// replace the desired raw pair repeatedly before it is used.
fn gamepad_resolve(which: u16) {
    let s = GAMEPAD_STATE.as_array();
    let offset: u16 = if which == 0 { 5 } else { 3 };
    let x = s[offset];
    let y = s[offset + 1];
    let enabled = s[0u16] != 0 && s[1u16] != 65;
    let mut all = GAMEPAD_STICKS.as_array();
    let mut c = view_of(&mut all[which]);
    // Device5 axes are zero-extended bytes. State 0 is uninitialized, 1 is
    // disabled, 2 is analog. The complete cache occupies exactly 32 words.
    c[0u16].raw_pair = x | (y << 8);
    c[0u16].key_state = if enabled { 2 } else { 1 };
    let sample = stick_read(view_of(&mut c[0u16].prepared), x, y, enabled);
    // Five Q16.16 values plus valid are adjacent in both layouts. Move their
    // eleven native words directly, without a second FLD/FST round trip.
    let out = addr_of(&c[0u16].x);
    let input = addr_of(&sample.x);
    unsafe {
        out.write(0, input.read(0));
        out.write(1, input.read(1));
        out.write(2, input.read(2));
        out.write(3, input.read(3));
        out.write(4, input.read(4));
        out.write(5, input.read(5));
        out.write(6, input.read(6));
        out.write(7, input.read(7));
        out.write(8, input.read(8));
        out.write(9, input.read(9));
        out.write(10, input.read(10));
        addr_of(&GAMEPAD_READY).write(0, GAMEPAD_READY & ((2 << which) ^ 65535));
    }
}

// Initialize software state and default calibration. No serial transaction/wait.
pub fn gamepad_init() {
    ds2_init(GAMEPAD_STATE.as_array());
    let mut config = stick_default();
    let prepared = stick_prepare(view_of(&mut config));
    let mut cache = GAMEPAD_STICKS.as_array();
    let mut i: u16 = 0;
    while i < 2 {
        cache[i].prepared = prepared;
        gamepad_resolve(i);
        i += 1;
    }
    unsafe {
        addr_of(&GAMEPAD_READY).write(0, 1);
    }
}

// Stick 0 is left; 1 is right. Rejection preserves the old configuration/result.
// A successful change invalidates results, including without a new wire sample.
pub fn gamepad_configure(which: u16, config: Array<StickConfig>) -> bool {
    if (GAMEPAD_READY & 1) == 0 || which >= 2 || !stick_config_valid(config) {
        return false;
    }
    let mut cache = GAMEPAD_STICKS.as_array();
    cache[which].prepared = stick_prepare(config);
    cache[which].key_state = 0;
    unsafe {
        addr_of(&GAMEPAD_READY).write(0, GAMEPAD_READY | (2 << which));
    }
    true
}

// Exactly one coherent-read attempt, with no numerical conversion. Extra calls
// cannot consume game edges or calculate intermediate, unconsumed positions.
pub fn gamepad_capture() -> bool {
    if (GAMEPAD_READY & 1) == 0 {
        return false;
    }
    let s = GAMEPAD_STATE.as_array();
    let updated = ds2_capture(s, GAMEPAD_RX.as_array());
    if !updated && s[0u16] != 0 {
        return false;
    }
    let enabled = s[0u16] != 0 && s[1u16] != 65;
    let cache = GAMEPAD_STICKS.as_array();
    let key: u16 = if enabled { 2 } else { 1 };
    let mut pending: u16 = 1;
    if cache[0u16].key_state != key || cache[0u16].raw_pair != (s[5u16] | (s[6u16] << 8)) {
        pending |= 2;
    }
    if cache[1u16].key_state != key || cache[1u16].raw_pair != (s[3u16] | (s[4u16] << 8)) {
        pending |= 4;
    }
    unsafe {
        addr_of(&GAMEPAD_READY).write(0, pending);
    }
    updated
}

// Consume once per game tick. Held buttons/motion survive no-new-sample ticks;
// the game still integrates motion using its elapsed time on every tick.
pub fn gamepad_take_frame() -> GamepadFrame {
    if (GAMEPAD_READY & 2) != 0 {
        gamepad_resolve(0);
    }
    if (GAMEPAD_READY & 4) != 0 {
        gamepad_resolve(1);
    }
    let mut s = GAMEPAD_STATE.as_array();
    let cache = GAMEPAD_STICKS.as_array();
    let pressed = s[8u16];
    let released = s[9u16];
    // Clear the same two consumed masks directly after resolving motion, so
    // frame values do not spill around a call to a two-store wrapper.
    s[8u16] = 0;
    s[9u16] = 0;
    return GamepadFrame {
        connected: s[0u16],
        id: s[1u16],
        buttons: s[2u16],
        pressed: pressed,
        released: released,
        left_x: cache[0u16].x,
        left_y: cache[0u16].y,
        right_x: cache[1u16].x,
        right_y: cache[1u16].y,
    };
}

// Optional consumption resolves only this stick, once per changed raw/config.
pub fn gamepad_direction(which: u16) -> vec2 {
    if which >= 2 {
        return vec2::new(fix32::zero(), fix32::zero());
    }
    if (GAMEPAD_READY & (2 << which)) != 0 {
        gamepad_resolve(which);
    }
    let c = GAMEPAD_STICKS.as_array();
    vec2::new(c[which].dir_x, c[which].dir_y)
}
pub fn gamepad_strength(which: u16) -> fix32 {
    if which >= 2 {
        return fix32::zero();
    }
    if (GAMEPAD_READY & (2 << which)) != 0 {
        gamepad_resolve(which);
    }
    GAMEPAD_STICKS.as_array()[which].strength
}
pub fn gamepad_stick_valid(which: u16) -> bool {
    if which >= 2 {
        return false;
    }
    if (GAMEPAD_READY & (2 << which)) != 0 {
        gamepad_resolve(which);
    }
    GAMEPAD_STICKS.as_array()[which].valid != 0
}
