//! Sampling and compression of the live deformable mesh, with ordered writes.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, object_query, random};
pub struct Record {
    pub position: Buf<u16, 6>,
    pub world: Buf<u16, 6>,
    pub timer: fix32,
    pub state: fix32,
    pub red: fix32,
    pub green: fix32,
    pub blue: fix32,
}
pub struct Surface {
    pub enabled: bool,
    pub origin: Array<u16>,
    pub basis: Array<u16>,
    pub bounds: Array<u16>,
    pub edges: Array<u16>,
    pub cap: fix32,
    // Other fields retain their existing mappings; 65535 keeps CPU vertices.
    pub vertex_page: u16,
    pub vertices: Array<i16>,
    pub vertex_count: u16,
    pub records: Array<Record>,
    pub record_count: u16,
    pub attached: bool,
    pub dirty: u16,
}
pub fn disabled(data: Array<u16>, records: Array<Record>) -> Surface {
    Surface {
        enabled: false,
        origin: data,
        basis: data,
        bounds: data,
        edges: data,
        cap: n(0),
        vertex_page: 65535,
        vertices: unsafe { data.as_ptr().as_i16_array() },
        vertex_count: 0,
        records: records,
        record_count: 0,
        attached: false,
        dirty: 0,
    }
}
pub struct Sample {
    pub local: Buf<u16, 6>,
    pub clamped: Buf<u16, 6>,
    pub height: fix32,
    pub weight: fix32,
    pub offset: fix32,
    pub activated: fix32,
}
pub struct HitState {
    pub dirty: fix32,
    pub last_vehicle: fix32,
    pub sound_timer: fix32,
}
pub struct HitConfig {
    pub floor: fix32,
    pub compression: fix32,
    pub drift_shift: u16,
    pub material: i16,
    pub has_sound: bool,
}
pub struct Mesh {
    // Other fields retain their existing mappings; 65535 keeps CPU vertices.
    pub vertex_page: u16,
    pub vertices: Array<i16>,
    pub vertex_count: u16,
    pub flags: Array<u16>,
    pub flag_count: u16,
}
pub struct Vehicle {
    pub entity: fix32,
    pub position: Array<u16>,
    pub material: Array<u16>,
    pub seed: Array<u16>,
    pub dt: fix32,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
pub fn record_zero() -> Record {
    Record {
        position: Buf::new([0; 6]),
        world: Buf::new([0; 6]),
        timer: n(0),
        state: n(0),
        red: n(0),
        green: n(0),
        blue: n(0),
    }
}
pub fn edge_weight(local: Array<u16>, edges: Array<u16>, cap: fix32) -> fix32 {
    let mut distance = abs(g(local, 0) - g(edges, 0));
    let mut i = 1u16;
    while i < 4 {
        let mut axis = 2u16;
        if i == 1 {
            axis = 0;
        }
        let d = abs(g(local, axis) - g(edges, i));
        if d < distance {
            distance = d;
        }
        i += 1;
    }
    distance = fmul_lo32(distance, n(10));
    if distance > cap {
        return cap;
    }
    distance
}
pub fn sample(surface: &mut Surface, world: Array<u16>) -> Sample {
    let mut out = Sample {
        local: Buf::new([0; 6]),
        clamped: Buf::new([0; 6]),
        height: n(0),
        weight: n(0),
        offset: n(0),
        activated: n(0),
    };
    let mut i = 0u16;
    while i < 3 {
        p(out.local.as_array(), i, g(world, i) - g(surface.origin, i));
        i += 1;
    }
    collision::transform(
        out.local.as_array(),
        surface.basis,
        out.local.as_array(),
        true,
    );
    i = 0;
    while i < 3 {
        p(
            out.clamped.as_array(),
            i,
            integer::trunc_shift(g(out.local.as_array(), i), 3),
        );
        i += 1;
    }
    i = 0;
    while i < 2 {
        let axis = i * 2;
        let mut value = g(out.clamped.as_array(), axis);
        if value < g(surface.bounds, i * 2) {
            value = g(surface.bounds, i * 2);
        }
        if value > g(surface.bounds, i * 2 + 1) {
            value = g(surface.bounds, i * 2 + 1);
        }
        p(out.clamped.as_array(), axis, value);
        i += 1;
    }
    let saved_page = read_dseg_page(3);
    if surface.vertex_page != 65535 && surface.vertex_page != saved_page {
        unsafe {
            write_dseg_page(3, surface.vertex_page);
        }
    }
    i = 0;
    while i < surface.vertex_count {
        let x = integer::from_i16(surface.vertices[i * 3]);
        let z = integer::from_i16(surface.vertices[i * 3 + 2]);
        let dx = g(out.clamped.as_array(), 0) - x;
        let dz = g(out.clamped.as_array(), 2) - z;
        if dx > n(-128) && dx < n(128) && dz > n(-128) && dz < n(128) {
            let y = integer::from_i16(surface.vertices[i * 3 + 1]);
            if y > out.height {
                out.height = y;
            }
        }
        i += 1;
    }
    if surface.vertex_page != 65535 && surface.vertex_page != saved_page {
        unsafe {
            write_dseg_page(3, saved_page);
        }
    }
    i = 0;
    let mut records = surface.records;
    while surface.attached && i < surface.record_count {
        let state = records[i].state;
        let mut activate = state == n(0);
        if state == n(1) {
            let dx = g(out.local.as_array(), 0) - g(records[i].position.as_array(), 0);
            let dz = g(out.local.as_array(), 2) - g(records[i].position.as_array(), 2);
            activate = dx > n(-750) && dx < n(750) && dz > n(-750) && dz < n(750);
        }
        if activate {
            records[i].state = n(2);
            records[i].red = n(127);
            records[i].green = n(127);
            records[i].blue = n(127);
            surface.dirty = 1;
            out.activated = out.activated + n(1);
        }
        i += 1;
    }
    out.weight = edge_weight(out.local.as_array(), surface.edges, surface.cap);
    out.offset = integer::trunc_shift(fmul_lo32(out.weight, fmul_lo32(out.height, n(8))), 16);
    out
}
// 0 no sound trigger, 1 sound trigger, 2 missing vertex flag. Playback is IO;
// both original random draws remain part of a successful trigger.
pub fn hit(
    state: &mut HitState,
    config: &HitConfig,
    collider: &object_query::Collider,
    mesh: &Mesh,
    vehicle: &mut Vehicle,
) -> u16 {
    state.dirty = n(1);
    state.last_vehicle = vehicle.entity;
    let local: Buf<u16, 6> = Buf::new([0; 6]);
    let point = local.as_array();
    let mut i = 0u16;
    while i < 3 {
        p(
            point,
            i,
            g(vehicle.position, i) - g(collider.position.as_array(), i),
        );
        i += 1;
    }
    if collider.has_external != n(0) {
        collision::transform(point, collider.external.as_array(), point, true);
    }
    i = 0;
    while i < 3 {
        p(point, i, integer::trunc_shift(g(point, i), 3));
        i += 1;
    }
    let saved_page = read_dseg_page(3);
    if mesh.vertex_page != 65535 && mesh.vertex_page != saved_page {
        unsafe {
            write_dseg_page(3, mesh.vertex_page);
        }
    }
    let mut vertices = mesh.vertices;
    let mut changed = false;
    i = 0;
    while i < mesh.vertex_count {
        let dx = g(point, 0) - integer::from_i16(vertices[i * 3]);
        let dz = g(point, 2) - integer::from_i16(vertices[i * 3 + 2]);
        let height = integer::from_i16(vertices[i * 3 + 1]);
        if dx > n(-256) && dx < n(256) && dz > n(-256) && dz < n(256) && height > config.floor {
            let mut level = integer::sar(height, 5);
            if level < n(0) {
                level = n(0);
            }
            if level > n(14) {
                level = n(14);
            }
            let amount = integer::sar(
                fmul_lo32(fmul_lo32(config.compression, level + n(1)), vehicle.dt),
                6,
            );
            vertices[i * 3 + 1] = (height - amount).lo_bits() as i16;
            if integer::from_i16(vertices[i * 3 + 1]) > config.floor {
                changed = true;
            } else {
                vertices[i * 3 + 1] = config.floor.lo_bits() as i16;
            }
            if i >= mesh.flag_count {
                if mesh.vertex_page != 65535 && mesh.vertex_page != saved_page {
                    unsafe {
                        write_dseg_page(3, saved_page);
                    }
                }
                return 2;
            }
            let flag = mesh.flags[i];
            if flag & 4 != 0 {
                let drift = integer::sar(vehicle.dt, config.drift_shift).lo_bits() as i16;
                let mut x = vertices[i * 3];
                let mut z = vertices[i * 3 + 2];
                if flag & 1 != 0 {
                    x = x - drift;
                } else {
                    x = x + drift;
                }
                if flag & 2 != 0 {
                    z = z - drift;
                } else {
                    z = z + drift;
                }
                vertices[i * 3] = x;
                vertices[i * 3 + 2] = z;
            }
        }
        i += 1;
    }
    if mesh.vertex_page != 65535 && mesh.vertex_page != saved_page {
        unsafe {
            write_dseg_page(3, saved_page);
        }
    }
    p(vehicle.material, 0, integer::from_i16(config.material));
    if config.has_sound {
        if state.sound_timer < vehicle.dt {
            state.sound_timer = n(0);
        } else {
            state.sound_timer = state.sound_timer - vehicle.dt;
        }
        if state.sound_timer == n(0) && changed {
            p(vehicle.seed, 0, random::next(g(vehicle.seed, 0)));
            p(vehicle.seed, 0, random::next(g(vehicle.seed, 0)));
            let seed = g(vehicle.seed, 0);
            let word = (seed.lo_bits() >> 4) | (seed.hi_bits() << 12);
            state.sound_timer = fix32::from_words((word & 511) + 400, 0);
            return 1;
        }
    }
    0
}
