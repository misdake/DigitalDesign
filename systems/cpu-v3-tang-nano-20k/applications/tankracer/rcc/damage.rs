//! One-shot mesh dents, track wear and incident-face normal repair.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer};
pub struct Mesh {
    pub present: bool,
    // 65535 keeps the current mapping. A banked mesh borrows DSEG3 only while
    // changing its geometry; poses, one-shot flags and revisions stay mapped.
    pub page: u16,
    pub vertices: Array<i16>,
    pub vertex_count: u16,
    pub indices: Array<u16>,
    pub normals: Array<i16>,
    pub face_count: u16,
}
pub struct Target {
    // Body quadrants, turret quadrants, two track wear counters.
    pub state: Array<u16>,
    pub player: bool,
    pub basis: Array<u16>,
    pub position: Array<u16>,
    pub selected: Array<u16>,
    pub mesh: Mesh,
    pub revision: Array<u16>,
    pub has_revision: bool,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
pub fn quadrant(basis: Array<u16>, position: Array<u16>, world: Array<u16>) -> u16 {
    let relative: Buf<u16, 6> = Buf::new([0; 6]);
    let local = relative.as_array();
    let mut i = 0u16;
    while i < 3 {
        attitude::put(
            local,
            i,
            attitude::get(world, i) - attitude::get(position, i),
        );
        i += 1;
    }
    collision::transform(local, basis, local, true);
    let mut q = 0u16;
    if attitude::get(local, 2) <= n(0) {
        q = 2;
    }
    if attitude::get(local, 0) > n(0) {
        q += 1;
    }
    q
}
// Status: 0 success/unchanged, 1 success/dent, 2 invalid vertex, 3 missing mesh.
fn repair(mesh: &Mesh, vertex: u16) -> u16 {
    let points: Buf<u16, 18> = Buf::new([0; 18]);
    let v = points.as_array();
    let a = unsafe { v.as_ptr().add(6).as_u16_array() };
    let b = unsafe { v.as_ptr().add(12).as_u16_array() };
    let mut face = 0u16;
    while face < mesh.face_count {
        let base = face * 3;
        if mesh.indices[base] == vertex
            || mesh.indices[base + 1] == vertex
            || mesh.indices[base + 2] == vertex
        {
            let mut corner = 0u16;
            while corner < 3 {
                let index = mesh.indices[base + corner];
                if index >= mesh.vertex_count {
                    return 2;
                }
                let mut axis = 0u16;
                while axis < 3 {
                    attitude::put(v, corner * 3 + axis, n(mesh.vertices[index * 3 + axis]));
                    axis += 1;
                }
                corner += 1;
            }
            let mut axis = 0u16;
            while axis < 3 {
                let origin = attitude::get(v, axis);
                let p1 = attitude::get(v, 3 + axis);
                let p2 = attitude::get(v, 6 + axis);
                // Scratch aliases the points: read all three before this write.
                attitude::put(a, axis, p2 - origin);
                attitude::put(b, axis, p1 - origin);
                axis += 1;
            }
            collision::normalize(a);
            collision::normalize(b);
            axis = 0;
            while axis < 3 {
                let j = (axis + 1) % 3;
                let k = (axis + 2) % 3;
                attitude::put(
                    v,
                    axis,
                    integer::sar(
                        fmul_lo32(attitude::get(a, j), attitude::get(b, k))
                            - fmul_lo32(attitude::get(b, j), attitude::get(a, k)),
                        12,
                    ),
                );
                axis += 1;
            }
            collision::normalize(v);
            let mut normals = mesh.normals;
            axis = 0;
            while axis < 3 {
                normals[base + axis] = attitude::get(v, axis).lo_bits() as i16;
                axis += 1;
            }
        }
        face += 1;
    }
    0
}
pub fn hit(target: &mut Target, world: Array<u16>, turret: bool) -> u16 {
    let q = quadrant(target.basis, target.position, world);
    let mut slot = q;
    if turret {
        slot += 4;
    }
    if target.state[slot] != 0 {
        return 0;
    }
    target.state[slot] = 1;
    if !target.mesh.present {
        return 3;
    }
    let vertex = target.selected[q];
    if vertex >= target.mesh.vertex_count {
        return 2;
    }
    let saved = read_dseg_page(3);
    if target.mesh.page != 65535 && target.mesh.page != saved {
        unsafe {
            write_dseg_page(3, target.mesh.page);
        }
    }
    let mut axis = 0u16;
    let mut vertices = target.mesh.vertices;
    while axis < 3 {
        let mut gain = n(3500);
        if axis == 2 {
            gain = n(2500);
        }
        if turret {
            gain = n(3000);
            if axis == 1 {
                gain = n(2200);
                if q & 2 != 0 {
                    gain = n(1200);
                }
            } else if axis == 2 {
                gain = n(3900);
            }
        }
        if turret || axis != 1 {
            let index = vertex * 3 + axis;
            vertices[index] =
                integer::sar(fmul_lo32(n(vertices[index]), gain), 12).lo_bits() as i16;
        }
        axis += 1;
    }
    let status = repair(&target.mesh, vertex);
    if read_dseg_page(3) != saved {
        unsafe {
            write_dseg_page(3, saved);
        }
    }
    if status != 0 {
        return status;
    }
    1
}
pub fn track(target: &mut Target, kind: fix32, world: Array<u16>) -> u16 {
    let mut slot = 9u16;
    if kind == n(2) {
        slot = 8;
    }
    if target.state[slot] >= 10 {
        return 0;
    }
    target.state[slot] += 1;
    if target.player {
        let status = hit(target, world, false);
        // The track callback dirties an existing body when its one-shot flag
        // changes, including a later mesh error. Direct projectile hits publish
        // their part revision only after successful geometry repair.
        if status != 0 && target.has_revision {
            let mut words = target.revision;
            let mut i = 0u16;
            while i < 4 {
                words[i] += 1;
                if words[i] != 0 {
                    break;
                }
                i += 1;
            }
        }
        return status;
    }
    0
}
