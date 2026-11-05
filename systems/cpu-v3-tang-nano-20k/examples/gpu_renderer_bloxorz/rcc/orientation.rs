//! Exact right-handed cube orientations, independent of occupancy and animation.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;

pub const ROTATION_IDENTITY: u16 = 8;
// Two object orientations, animation-start orientation, undo storage owner.
static ROTATIONS: Buf<u16, 4> = Buf::new([8, 8, 8, 0]);

fn rotation_cross(a: u16, b: u16) -> u16 {
    let x = a >> 1;
    let y = b >> 1;
    ((3 - x - y) << 1) | ((a ^ b ^ if (x + 1) % 3 == y { 0u16 } else { 1u16 }) & 1)
}
// Orientation = image of local Y (six signed axes) times four twists.
// A signed axis is axis*2 + negative; no floating-point state is accumulated.
pub fn rotation_axis(rotation: u16, column: u16) -> u16 {
    let y = rotation / 4;
    let first = if y >> 1 == 0 { 2u16 } else { 0u16 };
    let third = rotation_cross(first, y);
    let twist = rotation & 3;
    let x = if twist == 0 {
        first
    } else {
        if twist == 1 {
            third
        } else {
            if twist == 2 {
                first ^ 1
            } else {
                third ^ 1
            }
        }
    };
    if column == 0 {
        x
    } else {
        if column == 1 {
            y
        } else {
            rotation_cross(x, y)
        }
    }
}
fn rotation_quarter_axis(axis: u16, direction: u16) -> u16 {
    let coordinate = axis >> 1;
    let negative = axis & 1;
    if direction < 2 {
        if coordinate == 0 {
            2 | (negative ^ if direction == 0 { 1u16 } else { 0u16 })
        } else {
            if coordinate == 1 {
                negative ^ if direction == 1 { 1u16 } else { 0u16 }
            } else {
                axis
            }
        }
    } else {
        if coordinate == 1 {
            4 | (negative ^ if direction == 3 { 1u16 } else { 0u16 })
        } else {
            if coordinate == 2 {
                2 | (negative ^ if direction == 2 { 1u16 } else { 0u16 })
            } else {
                axis
            }
        }
    }
}
pub fn rotation_quarter(rotation: u16, direction: u16) -> u16 {
    let x = rotation_quarter_axis(rotation_axis(rotation, 0), direction);
    let y = rotation_quarter_axis(rotation_axis(rotation, 1), direction);
    let mut twist: u16 = 0;
    while twist < 4 && rotation_axis(y * 4 + twist, 0) != x {
        twist += 1;
    }
    if twist == 4 {
        halt(0xbad);
    }
    y * 4 + twist
}
pub fn rotation_begin() {
    if ROTATIONS[3u16] == 0 {
        ROTATIONS[3u16] = malloc(512).addr();
    }
}
pub fn rotation_end() {
    free(Ptr::from_addr(ROTATIONS[3u16]));
    ROTATIONS[3u16] = 0;
}
pub fn rotation_reset() {
    ROTATIONS[0u16] = ROTATION_IDENTITY;
    ROTATIONS[1u16] = ROTATION_IDENTITY;
    ROTATIONS[2u16] = ROTATION_IDENTITY;
}
pub fn rotation_current(actor: u16) -> u16 {
    ROTATIONS[actor]
}
pub fn rotation_animation() -> u16 {
    ROTATIONS[2u16]
}
pub fn rotation_start(actor: u16) {
    ROTATIONS[2u16] = ROTATIONS[actor];
}
pub fn rotation_save(index: u16) {
    let mut history = unsafe { Ptr::from_addr(ROTATIONS[3u16]).as_u16_array() };
    history[index * 2] = ROTATIONS[0u16];
    history[index * 2 + 1] = ROTATIONS[1u16];
}
pub fn rotation_undo(index: u16) {
    let history = unsafe { Ptr::from_addr(ROTATIONS[3u16]).as_u16_array() };
    ROTATIONS[0u16] = history[index * 2];
    ROTATIONS[1u16] = history[index * 2 + 1];
}
pub fn rotation_finish(direction: u16, from_split: bool, to_split: bool, actor: u16) {
    let value = rotation_quarter(ROTATIONS[2u16], direction);
    if from_split && to_split {
        ROTATIONS[actor] = value;
    } else {
        ROTATIONS[0u16] = value;
        if to_split {
            ROTATIONS[1u16] = value;
        }
    }
    ROTATIONS[2u16] = value;
}
fn rotation_component(row: Vec3, axis: u16) -> fix32 {
    let value = if axis >> 1 == 0 {
        row.x
    } else {
        if axis >> 1 == 1 {
            row.y
        } else {
            row.z
        }
    };
    if axis & 1 != 0 {
        -value
    } else {
        value
    }
}
fn rotation_row(row: Vec3, x: u16, y: u16, z: u16) -> Vec3 {
    Vec3 {
        x: rotation_component(row, x),
        y: rotation_component(row, y),
        z: rotation_component(row, z),
    }
}
fn rotation_world_row(row: Vec4, x: u16, y: u16, z: u16) -> Vec4 {
    let value = rotation_row(
        Vec3 {
            x: row.x,
            y: row.y,
            z: row.z,
        },
        x,
        y,
        z,
    );
    Vec4 {
        x: value.x,
        y: value.y,
        z: value.z,
        w: row.w,
    }
}
pub fn rotation_pose(model: &mut Model, pose: Transform, orientation: u16) {
    model.set_pose(pose);
    if orientation == ROTATION_IDENTITY {
        return;
    }
    let x = rotation_axis(orientation, 0);
    let y = rotation_axis(orientation, 1);
    let z = rotation_axis(orientation, 2);
    let world = model.world();
    let normal = model.normal();
    model.set_matrices(
        Mat3x4 {
            row0: rotation_world_row(world.row0, x, y, z),
            row1: rotation_world_row(world.row1, x, y, z),
            row2: rotation_world_row(world.row2, x, y, z),
        },
        Mat3x3 {
            row0: rotation_row(normal.row0, x, y, z),
            row1: rotation_row(normal.row1, x, y, z),
            row2: rotation_row(normal.row2, x, y, z),
        },
    );
}
