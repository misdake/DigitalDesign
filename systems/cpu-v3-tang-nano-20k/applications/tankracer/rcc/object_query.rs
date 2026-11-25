//! Ordered scene and tank queries over caller-owned contiguous collider records.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer};
pub struct Collider {
    pub entity: fix32,
    pub position: Buf<u16, 6>,
    pub basis: Buf<u16, 18>,
    pub external: Buf<u16, 18>,
    pub bounds: Buf<u16, 12>,
    pub radius: fix32,
    pub has_external: fix32,
}
pub struct Tank {
    pub collider: Collider,
    pub slot: fix32,
    pub active: fix32,
    pub disabled: fix32,
    pub ready: fix32,
    pub overlap_bounds: Buf<u16, 12>,
    pub contacts: Buf<u16, 24>,
}
pub struct Hit {
    pub entity: fix32,
    pub object: fix32,
    pub code: fix32,
    pub point: Buf<u16, 6>,
    pub normal: Buf<u16, 6>,
}
pub struct SegmentHit {
    pub point: Buf<u16, 6>,
    pub normal: Buf<u16, 6>,
}
pub struct Query {
    pub start: Array<u16>,
    pub end: Array<u16>,
    pub self_entity: fix32,
    pub object_count: u16,
    pub deformable: bool,
}
static IDENTITY: Buf<u16, 18> = Buf::new([
    4096, 0, 0, 0, 0, 0, 0, 0, 4096, 0, 0, 0, 0, 0, 0, 0, 4096, 0,
]);
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
fn copy(to: Array<u16>, from: Array<u16>) {
    let mut i = 0u16;
    while i < 3 {
        p(to, i, g(from, i));
        i += 1;
    }
}
pub fn collider_zero() -> Collider {
    Collider {
        entity: n(0),
        position: Buf::new([0; 6]),
        basis: Buf::new([0; 18]),
        external: Buf::new([0; 18]),
        bounds: Buf::new([0; 12]),
        radius: n(0),
        has_external: n(0),
    }
}
pub fn tank_zero() -> Tank {
    Tank {
        collider: collider_zero(),
        slot: n(0),
        active: n(0),
        disabled: n(0),
        ready: n(0),
        overlap_bounds: Buf::new([0; 12]),
        contacts: Buf::new([0; 24]),
    }
}
pub fn hit_zero() -> Hit {
    Hit {
        entity: n(-1),
        object: n(-1),
        code: n(0),
        point: Buf::new([0; 6]),
        normal: Buf::new([0; 6]),
    }
}
fn segment_zero() -> SegmentHit {
    SegmentHit {
        point: Buf::new([0; 6]),
        normal: Buf::new([0; 6]),
    }
}
pub fn cell(start: Array<u16>, origin: Array<u16>, shift_x: u16, shift_z: u16) -> u16 {
    let mut x = integer::sar(g(start, 0) - g(origin, 0), shift_x);
    let mut z = integer::sar(g(start, 2) - g(origin, 1), shift_z);
    if x < n(0) {
        x = n(0);
    } else if x > n(7) {
        x = n(7);
    }
    if z < n(0) {
        z = n(0);
    } else if z > n(7) {
        z = n(7);
    }
    x.lo_bits() + z.lo_bits() * 8
}
// 0 miss, 1 crossing, 2 inside, 3 arithmetic. A miss may publish point.
pub fn segment_box(a: Array<u16>, b: Array<u16>, bounds: Array<u16>, out: &mut SegmentHit) -> u16 {
    let raw: Buf<u16, 6> = Buf::new([0; 6]);
    let direction: Buf<u16, 6> = Buf::new([0; 6]);
    let sides: Buf<u16, 3> = Buf::new([2; 3]);
    let mut side = sides.as_array();
    let candidate: Buf<u16, 6> = Buf::new([0; 6]);
    let mut inside = true;
    let mut i = 0u16;
    while i < 3 {
        p(raw.as_array(), i, g(b, i) - g(a, i));
        p(direction.as_array(), i, g(raw.as_array(), i));
        if g(a, i) < g(bounds, i) {
            side[i] = 1;
            p(candidate.as_array(), i, g(bounds, i));
            inside = false;
        } else if g(a, i) > g(bounds, 3 + i) {
            side[i] = 0;
            p(candidate.as_array(), i, g(bounds, 3 + i));
            inside = false;
        }
        i += 1;
    }
    collision::normalize(direction.as_array());
    if inside {
        copy(out.point.as_array(), a);
        return 2;
    }
    let distances: Buf<u16, 6> = Buf::new([61440, 65535, 61440, 65535, 61440, 65535]);
    let d = distances.as_array();
    i = 0;
    while i < 3 {
        let denominator = g(direction.as_array(), i);
        if side[i] != 2 && denominator != n(0) {
            let numerator = fmul_lo32(g(candidate.as_array(), i) - g(a, i), n(4096));
            if numerator == fix32::from_words(0, 32768) && denominator == n(-1) {
                return 3;
            }
            p(d, i, integer::div(numerator, denominator));
        }
        i += 1;
    }
    let mut axis = 0u16;
    if g(d, 0) < g(d, 1) {
        axis = 1;
    }
    if g(d, axis) < g(d, 2) {
        axis = 2;
    }
    if g(d, axis) < n(0) {
        return 0;
    }
    i = 0;
    while i < 3 {
        p(
            out.point.as_array(),
            i,
            g(a, i) + integer::sar(fmul_lo32(g(direction.as_array(), i), g(d, axis)), 12),
        );
        i += 1;
    }
    i = 0;
    while i < 3 {
        if g(out.point.as_array(), i) < g(bounds, i) - n(2)
            || g(out.point.as_array(), i) > g(bounds, 3 + i) + n(2)
        {
            return 0;
        }
        i += 1;
    }
    if g(d, axis) > collision::distance(raw.as_array()) {
        return 0;
    }
    i = 0;
    while i < 3 {
        p(out.normal.as_array(), i, n(0));
        i += 1;
    }
    let mut normal = n(4096);
    if side[axis] == 1 {
        normal = n(-4096);
    }
    p(out.normal.as_array(), axis, normal);
    1
}
fn relative(out: Array<u16>, point: Array<u16>, collider: &Collider, external: bool) {
    let mut i = 0u16;
    while i < 3 {
        p(out, i, g(point, i) - g(collider.position.as_array(), i));
        i += 1;
    }
    if !external {
        collision::transform(out, collider.basis.as_array(), out, true);
    } else if collider.has_external != n(0) {
        collision::transform(out, collider.external.as_array(), out, true);
    }
}
fn world_hit(
    hit: &mut SegmentHit,
    basis: Array<u16>,
    origin: Array<u16>,
    delta: Array<u16>,
) -> bool {
    collision::transform(hit.normal.as_array(), basis, hit.normal.as_array(), false);
    let direction: Buf<u16, 6> = Buf::new([0; 6]);
    copy(direction.as_array(), delta);
    collision::normalize(direction.as_array());
    let mut projection = n(0);
    let mut i = 0u16;
    while i < 3 {
        projection = projection
            + integer::trunc_shift(
                fmul_lo32(g(direction.as_array(), i), g(hit.normal.as_array(), i)),
                12,
            );
        i += 1;
    }
    if projection >= n(0) {
        return false;
    }
    collision::transform(hit.point.as_array(), basis, hit.point.as_array(), false);
    i = 0;
    while i < 3 {
        p(
            hit.point.as_array(),
            i,
            g(hit.point.as_array(), i) + g(origin, i),
        );
        i += 1;
    }
    true
}
fn set(out: &mut Hit, index: u16, entity: fix32, code: u16) {
    out.object = fix32::from_words(index, 0);
    out.entity = entity;
    out.code = fix32::from_words(code, 0);
}
// Candidate order wins. 0 miss, 1 hit, 3 arithmetic, 4 missing object.
// The producer bounds cell lists and collider indices to the target u16 space.
pub fn query_list(
    ids: Array<u16>,
    count: u16,
    objects: Array<Collider>,
    q: &Query,
    out: &mut Hit,
) -> u16 {
    let delta: Buf<u16, 6> = Buf::new([0; 6]);
    let mut i = 0u16;
    while i < 3 {
        p(delta.as_array(), i, g(q.end, i) - g(q.start, i));
        i += 1;
    }
    let length = collision::distance(delta.as_array());
    let mut cursor = 0u16;
    while cursor < count {
        let index = ids[cursor];
        if index >= q.object_count {
            return 4;
        }
        let status = obstacle_candidate(&objects[index], index, q, out, delta.as_array(), length);
        if status != 0 {
            return status;
        }
        cursor += 1;
    }
    0
}
/// One narrow phase in source order; the caller may stream collider records.
pub fn obstacle_candidate(
    object: &Collider,
    index: u16,
    q: &Query,
    out: &mut Hit,
    delta: Array<u16>,
    length: fix32,
) -> u16 {
    let mut i = 0u16;
    let radius = fmul_lo32(object.radius + n(96), n(16));
    let mut broad = true;
    i = 0;
    while i < 3 {
        if abs(g(object.position.as_array(), i) - g(q.start, i)) - length >= radius {
            broad = false;
        }
        i += 1;
    }
    if broad && (q.deformable || object.entity != q.self_entity) {
        let a: Buf<u16, 6> = Buf::new([0; 6]);
        let b: Buf<u16, 6> = Buf::new([0; 6]);
        relative(a.as_array(), q.start, object, q.deformable);
        relative(b.as_array(), q.end, object, q.deformable);
        let mut hit = segment_zero();
        let mut code = segment_box(
            a.as_array(),
            b.as_array(),
            object.bounds.as_array(),
            &mut hit,
        );
        if code == 3 {
            return 3;
        }
        if !q.deformable && code == 2 {
            let direction: Buf<u16, 6> = Buf::new([0; 6]);
            i = 0;
            while i < 3 {
                p(
                    direction.as_array(),
                    i,
                    g(b.as_array(), i) - g(a.as_array(), i),
                );
                i += 1;
            }
            if collision::distance(direction.as_array()) > n(0) {
                collision::normalize(direction.as_array());
                i = 0;
                while i < 3 {
                    p(
                        b.as_array(),
                        i,
                        g(a.as_array(), i)
                            - fmul_lo32(g(direction.as_array(), i), integer::sar(radius, 11)),
                    );
                    i += 1;
                }
                code = segment_box(
                    b.as_array(),
                    a.as_array(),
                    object.bounds.as_array(),
                    &mut hit,
                );
                if code == 3 {
                    return 3;
                }
            }
        }
        if q.deformable && code != 0 {
            if code == 2 {
                copy(hit.normal.as_array(), a.as_array());
                collision::normalize(hit.normal.as_array());
            }
            set(out, index, object.entity, code);
            copy(out.point.as_array(), hit.point.as_array());
            copy(out.normal.as_array(), hit.normal.as_array());
            return 1;
        }
        if !q.deformable
            && code == 1
            && world_hit(
                &mut hit,
                object.basis.as_array(),
                object.position.as_array(),
                delta,
            )
        {
            set(out, index, object.entity, code);
            copy(out.point.as_array(), hit.point.as_array());
            copy(out.normal.as_array(), hit.normal.as_array());
            return 1;
        }
    }
    0
}

pub fn contains(tank: &Tank, point: Array<u16>) -> bool {
    let local: Buf<u16, 6> = Buf::new([0; 6]);
    relative(local.as_array(), point, &tank.collider, false);
    let b = tank.overlap_bounds.as_array();
    g(local.as_array(), 0) > g(b, 0)
        && g(local.as_array(), 0) < g(b, 3)
        && g(local.as_array(), 2) > g(b, 2)
        && g(local.as_array(), 2) < g(b, 5)
}
pub fn overlap(a: &Tank, b: &Tank) -> bool {
    let mut i = 0u16;
    while i < 4 {
        let point = unsafe {
            b.contacts
                .as_array()
                .as_ptr()
                .add((i * 6) as i16)
                .as_u16_array()
        };
        if contains(a, point) {
            return true;
        }
        i += 1;
    }
    i = 0;
    while i < 4 {
        let point = unsafe {
            a.contacts
                .as_array()
                .as_ptr()
                .add((i * 6) as i16)
                .as_u16_array()
        };
        if contains(b, point) {
            return true;
        }
        i += 1;
    }
    false
}
pub fn tank_candidate(own: &Tank, object: &Tank, q: &Query, out: &mut SegmentHit) -> u16 {
    if object.disabled != n(0) || object.ready == n(0) {
        return 0;
    }
    let radius = fmul_lo32(own.collider.radius, n(64));
    let origin = object.collider.position.as_array();
    if !(g(origin, 0) < g(q.start, 0) + radius
        && g(origin, 0) > g(q.start, 0) - radius
        && g(origin, 2) < g(q.start, 2) + radius
        && g(origin, 2) > g(q.start, 2) - radius)
    {
        return 0;
    }
    if overlap(own, object) {
        return 2;
    }
    let a: Buf<u16, 6> = Buf::new([0; 6]);
    let b: Buf<u16, 6> = Buf::new([0; 6]);
    relative(a.as_array(), q.start, &object.collider, true);
    relative(b.as_array(), q.end, &object.collider, true);
    let mut hit = segment_zero();
    let code = segment_box(
        a.as_array(),
        b.as_array(),
        object.collider.bounds.as_array(),
        &mut hit,
    );
    if code == 3 || code == 2 {
        return code;
    }
    if code == 1 {
        let delta: Buf<u16, 6> = Buf::new([0; 6]);
        let mut i = 0u16;
        while i < 3 {
            p(delta.as_array(), i, g(q.end, i) - g(q.start, i));
            i += 1;
        }
        let mut basis = IDENTITY.as_array();
        if object.collider.has_external != n(0) {
            basis = object.collider.external.as_array();
        }
        if world_hit(&mut hit, basis, origin, delta.as_array()) {
            copy(out.point.as_array(), hit.point.as_array());
            copy(out.normal.as_array(), hit.normal.as_array());
            return 1;
        }
    }
    0
}
// Slots contain nine stable indices, with 65535 for absent. Geometric code or error.
pub fn tanks(own: &Tank, objects: Array<Tank>, slots: Array<u16>, q: &Query, out: &mut Hit) -> u16 {
    let mut slot = 0u16;
    while slot < 9 {
        let index = slots[slot];
        if index != 65535 && fix32::from_words(slot, 0) != own.slot && own.active != n(0) {
            if index >= q.object_count {
                return 4;
            }
            let mut hit = segment_zero();
            let code = tank_candidate(own, &objects[index], q, &mut hit);
            if code == 3 {
                return 3;
            }
            if code != 0 {
                set(out, index, objects[index].collider.entity, code);
                if code == 1 {
                    copy(out.point.as_array(), hit.point.as_array());
                    copy(out.normal.as_array(), hit.normal.as_array());
                }
                return code;
            }
        }
        slot += 1;
    }
    out.entity = n(-1);
    out.object = n(-1);
    out.code = n(0);
    0
}
