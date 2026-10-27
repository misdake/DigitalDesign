//! Bounded integer game rules. World X/Z match the reference game, not camera axes.
use crate::dsl_rt::*;

// State words: x,z,orientation,bridge mask,phase,moves,other x/z,active,split.
// Phases: playing=0, success=1, fall=2. Commands: +X,-X,+Z,-Z,restart,undo,swap.
// Level: eight header words, then eight words per cell. See docs/core.md.
pub const BLOX_HISTORY: u16 = 256;

fn blox_cell(level: Array<u16>, x: i16, z: i16) -> u16 {
    if x < 0 || z < 0 || x as u16 >= level[0u16] || z as u16 >= level[1u16] {
        return 0;
    }
    8 + ((z as u16) * level[0u16] + x as u16) * 8
}
fn blox_tile(level: Array<u16>, x: i16, z: i16) -> u16 {
    let cell = blox_cell(level, x, z);
    if cell == 0 {
        return 0;
    }
    level[cell]
}
fn blox_switch(level: Array<u16>, mut state: Array<u16>, x: i16, z: i16) {
    let cell = blox_cell(level, x, z);
    if cell != 0 {
        state[3u16] = (state[3u16] & level[cell + 2]) ^ level[cell + 3];
    }
}
fn blox_support(level: Array<u16>, x: i16, z: i16, mask: u16, heavy: bool) -> bool {
    let cell = blox_cell(level, x, z);
    if cell == 0 {
        return false;
    }
    let tile = level[cell];
    tile != 0 && !(tile == 2 && heavy) && !(tile == 5 && mask & level[cell + 1] == 0)
}
fn blox_whole_rest(level: Array<u16>, mut state: Array<u16>) {
    let x = state[0u16] as i16;
    let z = state[1u16] as i16;
    let orient = state[2u16];
    let tile = blox_tile(level, x, z);
    if tile == 3 || (tile == 4 && orient == 0) {
        blox_switch(level, state, x, z);
    }
    let second_x = x + (if orient == 1 { 1i16 } else { 0i16 });
    let second_z = z + (if orient == 2 { 1i16 } else { 0i16 });
    if orient != 0 && blox_tile(level, second_x, second_z) == 3 {
        blox_switch(level, state, second_x, second_z);
    }
    state[4u16] = 0;
    if orient == 0 && blox_tile(level, x, z) == 7 {
        let cell = blox_cell(level, x, z);
        if level[cell + 4] != 65535 {
            state[0u16] = level[cell + 4];
            state[1u16] = level[cell + 5];
            state[6u16] = level[cell + 6];
            state[7u16] = level[cell + 7];
            state[8u16] = 0;
            state[9u16] = 1;
            return;
        }
    }
    let goal_tile = blox_tile(level, x, z);
    if orient == 0 && goal_tile == 6 {
        state[4u16] = 1;
    } else {
        if !blox_support(level, x, z, state[3u16], orient == 0) {
            state[4u16] = 2;
        }
        if orient != 0 && !blox_support(level, second_x, second_z, state[3u16], false) {
            state[4u16] = 2;
        }
    }
    state[6u16] = 0;
    state[7u16] = 0;
    state[8u16] = 0;
    state[9u16] = 0;
}
fn blox_split_rest(level: Array<u16>, mut state: Array<u16>) {
    let active: u16 = if state[8u16] == 0 { 0 } else { 6 };
    let ax = state[active] as i16;
    let az = state[active + 1] as i16;
    if blox_tile(level, ax, az) == 3 {
        blox_switch(level, state, ax, az);
    }
    let x = state[0u16] as i16;
    let z = state[1u16] as i16;
    let ox = state[6u16] as i16;
    let oz = state[7u16] as i16;
    let dx = if x < ox { ox - x } else { x - ox };
    let dz = if z < oz { oz - z } else { z - oz };
    if dx + dz == 1 {
        state[0u16] = (if x < ox { x } else { ox }) as u16;
        state[1u16] = (if z < oz { z } else { oz }) as u16;
        state[2u16] = if z == oz { 1 } else { 2 };
        state[9u16] = 0;
        // Merged footprint evaluates switches again, matching the original rules.
        blox_whole_rest(level, state);
        return;
    }
    state[4u16] = 0;
    if !blox_support(level, x, z, state[3u16], false)
        || !blox_support(level, ox, oz, state[3u16], false)
    {
        state[4u16] = 2;
    }
}
fn blox_rest(level: Array<u16>, state: Array<u16>) {
    if state[9u16] == 0 {
        blox_whole_rest(level, state);
    }
    if state[9u16] != 0 {
        blox_split_rest(level, state);
    }
}
pub fn blox_reset(level: Array<u16>, mut state: Array<u16>, mut history_state: Array<u16>) {
    let mut i: u16 = 0;
    while i < 10 {
        state[i] = 0;
        i += 1;
    }
    state[0u16] = level[2u16];
    state[1u16] = level[3u16];
    state[2u16] = level[4u16];
    state[3u16] = level[5u16];
    history_state[0u16] = 0;
    history_state[1u16] = 0;
    blox_rest(level, state);
}
pub fn blox_pack(state: Array<u16>, mut out: Array<u16>, at: u16) {
    out[at] = state[0u16];
    out[at + 1] = state[1u16];
    out[at + 2] = state[2u16] | (state[3u16] << 2) | (state[4u16] << 14);
    out[at + 3] = state[5u16];
    out[at + 4] = (state[6u16] & 32767) | (state[7u16] << 15);
    out[at + 5] = ((state[7u16] & 32767) >> 1) | (state[8u16] << 14) | (state[9u16] << 15);
}
fn blox_unpack(mut state: Array<u16>, words: Array<u16>, at: u16) {
    state[0u16] = words[at];
    state[1u16] = words[at + 1];
    state[2u16] = words[at + 2] & 3;
    state[3u16] = (words[at + 2] >> 2) & 4095;
    state[4u16] = words[at + 2] >> 14;
    state[5u16] = words[at + 3];
    let ox = words[at + 4] & 32767;
    let oz = ((words[at + 5] & 16383) << 1) | (words[at + 4] >> 15);
    state[6u16] = ox | (if ox & 16384 != 0 { 32768 } else { 0 });
    state[7u16] = oz | (if oz & 16384 != 0 { 32768 } else { 0 });
    state[8u16] = (words[at + 5] >> 14) & 1;
    state[9u16] = words[at + 5] >> 15;
}
// A fixed 256-snapshot ring retains the latest moves; no heap allocation.
pub fn blox_dispatch(
    level: Array<u16>,
    mut state: Array<u16>,
    history: Array<u16>,
    mut hs: Array<u16>,
    command: u16,
) -> bool {
    if command == 4 {
        blox_reset(level, state, hs);
        return true;
    }
    if command == 5 {
        if hs[1u16] == 0 {
            return false;
        }
        hs[0u16] = (hs[0u16] + 255) & 255;
        hs[1u16] -= 1;
        blox_unpack(state, history, hs[0u16] * 6);
        return true;
    }
    if command > 6 || state[4u16] != 0 || state[5u16] == 65535 || (command == 6 && state[9u16] == 0)
    {
        return false;
    }
    blox_pack(state, history, hs[0u16] * 6);
    hs[0u16] = (hs[0u16] + 1) & 255;
    if hs[1u16] < BLOX_HISTORY {
        hs[1u16] += 1;
    }
    state[5u16] += 1;
    if command == 6 {
        state[8u16] ^= 1;
    } else {
        let mut x = state[0u16] as i16;
        let mut z = state[1u16] as i16;
        let mut orient = state[2u16];
        if state[9u16] != 0 {
            let at: u16 = if state[8u16] == 0 { 0 } else { 6 };
            if command == 0 {
                state[at] += 1;
            }
            if command == 1 {
                state[at] -= 1;
            }
            if command == 2 {
                state[at + 1] += 1;
            }
            if command == 3 {
                state[at + 1] -= 1;
            }
        } else {
            if orient == 0 {
                if command == 0 {
                    x += 1;
                    orient = 1;
                }
                if command == 1 {
                    x -= 2;
                    orient = 1;
                }
                if command == 2 {
                    z += 1;
                    orient = 2;
                }
                if command == 3 {
                    z -= 2;
                    orient = 2;
                }
            } else {
                if orient == 1 {
                    if command == 0 {
                        x += 2;
                        orient = 0;
                    }
                    if command == 1 {
                        x -= 1;
                        orient = 0;
                    }
                    if command == 2 {
                        z += 1;
                    }
                    if command == 3 {
                        z -= 1;
                    }
                } else {
                    if command == 0 {
                        x += 1;
                    }
                    if command == 1 {
                        x -= 1;
                    }
                    if command == 2 {
                        z += 2;
                        orient = 0;
                    }
                    if command == 3 {
                        z -= 1;
                        orient = 0;
                    }
                }
            }
            state[0u16] = x as u16;
            state[1u16] = z as u16;
            state[2u16] = orient;
        }
    }
    blox_rest(level, state);
    true
}
