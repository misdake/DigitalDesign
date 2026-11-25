//! Live airborne scenery hit response, using explicit collider/state views.
use crate::dsl_rt::*;
use crate::{
    attitude, collision, ground, integer, lifecycle, ordered, random, scene_world, spatial, terrain,
};
pub struct Motion {
    // Velocity XYZ, spin, ground, timer, age, airborne, reverse, special, raised.
    // Timer and age hold their original u16/i16 values in raw i32 pairs.
    pub state: Array<u16>,
    pub properties: Array<u16>,
    pub collider: Array<u16>,
    pub reference_alias: bool,
    pub cell: spatial::Cell,
    // PlacedObstacle::impact_threshold is independent of collision threshold.
    pub impact_threshold: fix32,
}
pub struct Config {
    // Elasticity, friction, height factor, signed hit-spin multiplier/shift,
    // signed slow-spin multiplier/shift, deflection, gravity, height period.
    pub data: Array<u16>,
    pub sine: Array<i16>,
    pub angle: Array<i16>,
}
pub struct Hit {
    pub position: Array<u16>,
    pub velocity: Array<u16>,
    pub forward: fix32,
    pub reference: Array<u16>,
    pub external: Array<u16>,
    pub has_external: bool,
}
pub struct Frame {
    pub entity: fix32,
    pub limit: fix32,
    pub dt: fix32,
    pub id: fix32,
    pub has_mesh: bool,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
pub fn rotate(m: &mut Motion, c: &Config, axis: u16, angle: fix32) {
    if attitude::get(m.collider, 29) == n(0) {
        return;
    }
    attitude::rotate(m.collider, 13, axis, angle, c.sine);
    if m.reference_alias {
        let mut row = m.collider;
        let mut i = 0u16;
        while i < 18 {
            row[8 + i] = row[26 + i];
            i += 1;
        }
    }
}
// The authored Bounce activation is EnableTick. Failures retain velocity,
// matrices, airborne and activity, before age/spin/RNG publication.
pub fn hit(
    m: &mut Motion,
    c: &Config,
    h: &Hit,
    runtime: &mut scene_world::Runtime,
    frame: &Frame,
) -> u16 {
    let relative: Buf<u16, 6> = Buf::new([0; 6]);
    let v = relative.as_array();
    let mut i = 0u16;
    while i < 3 {
        attitude::put(
            v,
            i,
            attitude::get(m.collider, 1 + i) - attitude::get(h.position, i),
        );
        i += 1;
    }
    if h.has_external {
        collision::transform(v, h.external, v, true);
    }
    let angle = integer::sar(
        -fmul_lo32(attitude::get(c.data, 7), attitude::get(v, 0)),
        11,
    );
    let elasticity = attitude::get(c.data, 0);
    let split = attitude::get(c.data, 2);
    let mut forward = h.forward;
    if forward < n(0) {
        forward = -forward;
    }
    attitude::put(
        m.state,
        0,
        integer::sar(
            fmul_lo32(attitude::get(h.velocity, 0), integer::sar(elasticity, 4)),
            6,
        ),
    );
    attitude::put(
        m.state,
        1,
        integer::sar(
            fmul_lo32(fmul_lo32(integer::sar(forward, 6), split), elasticity),
            11,
        ),
    );
    attitude::put(
        m.state,
        2,
        integer::sar(
            fmul_lo32(
                fmul_lo32(
                    integer::sar(attitude::get(h.velocity, 2), 6),
                    n(128) - split,
                ),
                elasticity,
            ),
            11,
        ),
    );
    collision::yaw(m.state, angle, c.sine);
    let row = m.collider;
    i = 0;
    while i < 9 {
        attitude::put(row, 4 + i, attitude::get(h.reference, i));
        i += 1;
    }
    if m.reference_alias && attitude::get(row, 29) != n(0) {
        i = 0;
        while i < 9 {
            attitude::put(row, 13 + i, attitude::get(h.reference, i));
            i += 1;
        }
    }
    rotate(m, c, 1, angle);
    attitude::put(m.state, 7, n(1));
    if attitude::get(m.properties, 3) == n(0) {
        attitude::put(m.properties, 3, n(1));
        let saved = attitude::get(runtime.slot, 0);
        attitude::put(runtime.slot, 0, n(4));
        if frame.limit == n(0) {
            return 2;
        }
        let status = lifecycle::enable(&mut runtime.ticks, frame.entity);
        if status >= 2 {
            return status;
        }
        attitude::put(runtime.slot, 0, saved);
    }
    attitude::put(m.state, 6, n(0));
    attitude::put(
        m.state,
        3,
        integer::sar(
            -fmul_lo32(collision::distance(m.state), attitude::get(c.data, 3)),
            attitude::get(c.data, 4).lo_bits() & 31,
        ),
    );
    let mut reverse = n(0);
    if forward > fix32::from_words(53392, 3) {
        let seed = random::next(attitude::get(runtime.seed, 0));
        attitude::put(runtime.seed, 0, seed);
        reverse = fix32::from_words((seed.lo_bits() >> 4) & 1, 0);
    }
    attitude::put(m.state, 8, reverse);
    0
}
// Authored Bounce Deactivate calls Generic QueueDestroy, then removes grid,
// ordinary and special memberships. The pending queue is drained by the owner.
fn deactivate(
    m: &Motion,
    runtime: &mut scene_world::Runtime,
    frame: &Frame,
    grid: &mut spatial::Grid,
) -> u16 {
    let saved = attitude::get(runtime.slot, 0);
    attitude::put(runtime.slot, 0, n(8));
    let status = lifecycle::queue(
        &mut runtime.queues,
        &mut runtime.ticks,
        frame.entity,
        true,
        frame.limit,
    );
    if status != 0 {
        return status;
    }
    let status = spatial::remove(grid, frame.id, &m.cell, frame.limit);
    if status != 0 {
        return status;
    }
    let status = ordered::remove_first(&mut runtime.ordinary, frame.id, frame.limit);
    if status != 0 {
        return status;
    }
    let status = ordered::remove_first(&mut runtime.special, frame.id, frame.limit);
    if status != 0 {
        return status;
    }
    attitude::put(runtime.slot, 0, saved);
    0
}
fn query_error(status: u16) -> u16 {
    if status == 2 {
        return 5;
    }
    8
}
// 0 success; 2 invalid limit, 3 work limit, 4 capacity, 5 arithmetic,
// 6 missing tick entity, 7 missing external basis, 8 terrain residency/bucket.
pub fn tick(
    m: &mut Motion,
    c: &Config,
    runtime: &mut scene_world::Runtime,
    frame: &Frame,
    grid: &mut spatial::Grid,
    map: &mut ground::World,
) -> u16 {
    if frame.limit == n(0) {
        return 2;
    }
    let s = m.state;
    if attitude::get(m.properties, 3) == n(0) {
        return 0;
    }
    if attitude::get(s, 6) > n(20000) {
        return deactivate(m, runtime, frame, grid);
    }
    let age = (attitude::get(s, 6).lo_bits() as i16) + (frame.dt.lo_bits() as i16);
    attitude::put(s, 6, n(age));
    let mut i = 0u16;
    while i < 3 {
        attitude::put(
            m.collider,
            1 + i,
            attitude::get(m.collider, 1 + i)
                + integer::sar(fmul_lo32(frame.dt, attitude::get(s, i)), 8),
        );
        i += 1;
    }
    let mut spin = attitude::get(s, 3)
        + integer::sar(
            fmul_lo32(attitude::get(c.data, 5), frame.dt),
            attitude::get(c.data, 6).lo_bits() & 31,
        );
    if spin > n(0) {
        spin = n(0);
    }
    attitude::put(s, 3, spin);
    if attitude::get(s, 8) != n(0) {
        spin = -spin;
    }
    rotate(m, c, 0, spin);
    let timer = attitude::get(s, 5).lo_bits() + 1;
    attitude::put(s, 5, fix32::from_words(timer, 0));
    let period = attitude::get(c.data, 9).lo_bits();
    if period == 0 {
        return 5;
    }
    let mut ground = attitude::get(s, 4);
    let miss = fix32::from_words(1, 32768);
    let x = attitude::get(m.collider, 1);
    let z = attitude::get(m.collider, 3);
    if timer % period == 0 {
        map.bind(x, z);
        let query = map.terrain.height(x, z);
        if query.status >= 2 {
            return query_error(query.status);
        }
        ground = query.height;
        if ground != miss {
            let mut raised = n(0);
            if attitude::get(s, 10) != n(0) {
                raised = n(750);
            }
            attitude::put(s, 4, ground + raised);
        }
    }
    let friction = attitude::get(c.data, 1);
    let elasticity = attitude::get(c.data, 0);
    if attitude::get(s, 7) == n(0) {
        let limit = fmul_lo32(friction, m.impact_threshold);
        attitude::put(m.collider, 2, attitude::get(s, 4));
        if attitude::get(s, 0) <= limit && attitude::get(s, 2) <= limit {
            map.bind(x, z);
            let mut normal = terrain::Normal {
                x: n(0),
                y: n(0),
                z: n(0),
            };
            let query = map.terrain.normal_height(x, z, &mut normal);
            if query.status >= 2 {
                return query_error(query.status);
            }
            ground = query.height;
            if ground == miss {
                let status = deactivate(m, runtime, frame, grid);
                if status != 0 {
                    return status;
                }
            } else {
                if attitude::get(s, 10) == n(0) {
                    if attitude::get(m.collider, 29) == n(0) {
                        return 7;
                    }
                    let scratch: Buf<u16, 6> = Buf::new([0; 6]);
                    let local = scratch.as_array();
                    attitude::put(local, 0, normal.x);
                    attitude::put(local, 1, normal.y);
                    attitude::put(local, 2, normal.z);
                    collision::transform(
                        local,
                        unsafe { m.collider.as_ptr().add(26).as_u16_array() },
                        local,
                        true,
                    );
                    let angle =
                        attitude::angle(c.angle, attitude::get(local, 1), attitude::get(local, 2));
                    if angle == n(-1) {
                        return 5;
                    }
                    rotate(m, c, 0, angle);
                }
                attitude::put(m.properties, 3, n(0));
                let status = spatial::remove(grid, frame.id, &m.cell, frame.limit);
                if status != 0 {
                    return status;
                }
                let inserted = spatial::insert(
                    grid,
                    frame.id,
                    unsafe { m.collider.as_ptr().add(2).as_u16_array() },
                    frame.has_mesh,
                    attitude::get(m.collider, 28).lo_bits(),
                );
                if inserted.status != 0 {
                    return inserted.status;
                }
                m.cell = inserted.cell;
                let status = lifecycle::queue(
                    &mut runtime.queues,
                    &mut runtime.ticks,
                    frame.entity,
                    false,
                    frame.limit,
                );
                if status != 0 {
                    return status;
                }
            }
        }
    } else if attitude::get(m.collider, 2) < attitude::get(s, 4) {
        attitude::put(m.collider, 2, attitude::get(s, 4));
        attitude::put(
            s,
            1,
            integer::sar(-fmul_lo32(attitude::get(s, 1), elasticity), 7),
        );
        if attitude::get(s, 1) < fmul_lo32(m.impact_threshold, elasticity) {
            attitude::put(s, 1, n(0));
            attitude::put(s, 7, n(0));
        }
    }
    if ground == miss {
        map.bind(x, z);
        let mut normal = terrain::Normal {
            x: n(0),
            y: n(0),
            z: n(0),
        };
        let status = map
            .terrain
            .fallback(x, attitude::get(m.collider, 2), z, 0, &mut normal);
        if status >= 2 {
            return 8;
        }
        if status == 1 {
            let dot = integer::trunc_shift(fmul_lo32(normal.x, attitude::get(s, 0)), 11)
                + integer::trunc_shift(fmul_lo32(normal.z, attitude::get(s, 2)), 11);
            if dot < n(0) {
                attitude::put(
                    s,
                    0,
                    integer::sar(
                        fmul_lo32(
                            attitude::get(s, 0)
                                - integer::trunc_shift(fmul_lo32(dot, normal.x), 12),
                            elasticity,
                        ),
                        7,
                    ),
                );
                attitude::put(
                    s,
                    2,
                    integer::sar(
                        fmul_lo32(
                            attitude::get(s, 2)
                                - integer::trunc_shift(fmul_lo32(dot, normal.z), 12),
                            elasticity,
                        ),
                        7,
                    ),
                );
                if attitude::get(m.collider, 29) == n(0) {
                    return 7;
                }
                let scratch: Buf<u16, 6> = Buf::new([0; 6]);
                let local = scratch.as_array();
                collision::transform(
                    local,
                    unsafe { m.collider.as_ptr().add(26).as_u16_array() },
                    s,
                    true,
                );
                let angle =
                    attitude::angle(c.angle, attitude::get(local, 0), attitude::get(local, 2));
                if angle == n(-1) {
                    return 5;
                }
                rotate(m, c, 1, -angle);
            }
        }
    }
    attitude::put(
        s,
        1,
        attitude::get(s, 1) - fmul_lo32(attitude::get(c.data, 8), frame.dt),
    );
    attitude::put(
        s,
        0,
        attitude::get(s, 0) - integer::sar(fmul_lo32(friction, attitude::get(s, 0)), 7),
    );
    attitude::put(
        s,
        2,
        attitude::get(s, 2) - integer::sar(fmul_lo32(friction, attitude::get(s, 2)), 7),
    );
    0
}
