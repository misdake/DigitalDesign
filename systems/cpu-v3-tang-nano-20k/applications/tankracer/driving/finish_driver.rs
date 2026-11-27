//! ENGLAND finish AITank stages around the existing ordered Tank update.
#![link_cseg = "prefix"]
use super::*;
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
fn g(data: Array<u16>, column: u16) -> fix32 {
    attitude::get(data, column)
}
fn p(data: Array<u16>, column: u16, value: fix32) {
    attitude::put(data, column, value);
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn abs(value: fix32) -> fix32 {
    if value < n(0) {
        return -value;
    }
    value
}
fn ai() -> Array<u16> {
    ptr(0xc000 + complete_check::FINISH_AI_WORD)
}
fn effects() -> Array<u16> {
    ptr(0xc000 + complete_check::CAR_WORD + 1912)
}
fn flags(mask: u16, set: u16) {
    let value = g(ai(), 2);
    p(
        ai(),
        2,
        fix32::from_words((value.lo_bits() & mask) | set, value.hi_bits()),
    );
}
// Caller holds ACTORS in DSEG2 and the shared effects in DSEG3.
fn routes(collision_page: u16) -> finish_drive::Routes {
    let h = ptr(0x8000 + finish_world::WORD);
    finish_drive::Routes {
        indices: at(h, h[5u16] as i16),
        points: at(h, h[6u16] as i16),
        count: h[3u16],
        point_count: h[4u16],
        collider_page: collision_page,
    }
}
fn config(player: &player_world::Player) -> finish_drive::Config {
    let h = ptr(0x8000 + finish_world::WORD);
    finish_drive::Config {
        values: at(h, h[7u16] as i16),
        angle: player.angle,
    }
}
fn near(car: &driving::Car, limit: i16) -> bool {
    let camera = ptr(0xc000 + 10174);
    let mut i = 0u16;
    while i < 3 {
        if i != 1 && abs(integer::sar(g(at(car.motion, 18), i), 3) - g(camera, i)) >= n(limit) {
            return false;
        }
        i += 1;
    }
    true
}
fn pose(car: &driving::Car, player: &player_world::Player) -> fix32 {
    let b = effects();
    let travel: Buf<u16, 8> = Buf::new([0; 8]);
    let env: Buf<u16, 36> = Buf::new([0; 36]);
    copy(env.as_array(), at(b, 902), 36);
    p(env.as_array(), 6, g(car.articulation.as_array(), 0));
    p(env.as_array(), 7, g(car.articulation.as_array(), 2));
    let mut i = 0u16;
    while i < 4 {
        p(travel.as_array(), i, g(car.suspension, i * 6 + 3));
        i += 1;
    }
    vehicle_pose::update(
        &vehicle_pose::State {
            physical_position: at(car.motion, 6),
            reference: car.attitude,
            base_position: at(car.motion, 18),
            body_basis: at(car.attitude, 18),
            body_position: at(car.attitude, 36),
            tracks: car.tracks,
            turret: b,
            gun: at(b, 24),
            attachments: at(b, 48),
            bottom_position: at(b, 96),
            recoil: at(b, 102),
        },
        at(b, 848),
        travel.as_array(),
        &vehicle_pose::Environment {
            timer: g(env.as_array(), 0).lo_bits(),
            values: env.as_array(),
        },
        player.sine,
        player.angle,
    )
}
fn normalize(data: Array<u16>) {
    let a = abs(g(data, 0));
    let b = abs(g(data, 1));
    let c = abs(g(data, 2));
    let mut length = c + integer::sar(a, 2) + integer::sar(b, 2);
    if b > a && b > c {
        length = b + integer::sar(a, 2) + integer::sar(c, 2);
    } else if a >= b && a > c {
        length = a + integer::sar(b, 2) + integer::sar(c, 2);
    }
    if length <= n(0) {
        return;
    }
    let mut i = 0u16;
    while i < 3 {
        let mut numerator = integer::mul(g(data, i), n(4096));
        let mut denominator = length;
        if length > fix32::from_words(0, 4) {
            numerator = g(data, i);
            denominator = integer::sar(length, 12);
        }
        p(data, i, integer::div(numerator, denominator));
        i += 1;
    }
}
fn far(
    state: &scene_parent::State,
    world: &scene_world::World,
    player: &player_world::Player,
    route: &finish_drive::Routes,
) -> fix32 {
    let a = ai();
    if g(a, 9) == n(0) {
        return n(-4);
    }
    let id = finish_drive::point(route, &state.lifetime.identities, g(a, 10));
    if id < n(0) {
        return id;
    }
    let side = g(a, 12);
    if side.hi_bits() != 0 || side.lo_bits() > 1 {
        return n(-4);
    }
    let entity = finish_drive::next(route, id.lo_bits(), side.lo_bits());
    if entity == n(-1) {
        return n(-4);
    }
    if entity < n(-1) {
        return entity;
    }
    let id = finish_drive::point(route, &state.lifetime.identities, entity);
    if id < n(0) {
        return id;
    }
    let row: Buf<u16, 60> = Buf::new([0; 60]);
    if !finish_drive::load_collider(route, world, id.lo_bits(), row.as_array()) {
        return n(-4);
    }
    let car = &state.cars[2u16];
    let direction: Buf<u16, 6> = Buf::new([0; 6]);
    let d = direction.as_array();
    let mut i = 0u16;
    while i < 3 {
        p(d, i, g(row.as_array(), 1 + i) - g(at(car.motion, 18), i));
        i += 1;
    }
    normalize(d);
    let h = ptr(0x8000 + finish_world::WORD);
    let mut speed = fix32::from_words(h[11u16], h[12u16]);
    let c = config(player);
    let spacing = g(a, 3);
    if spacing == n(1) || spacing == n(2) {
        let mut offset = 58u16;
        if spacing == n(2) {
            offset = 60;
        }
        speed = integer::sar(
            integer::mul(speed, g(c.values, offset)),
            g(c.values, offset + 1).lo_bits(),
        );
    }
    // The finish owner is the primary car. Its rank cannot equal primary rank
    // minus one, so the source catch-up multiplier has no iterations here.
    speed = integer::sar(speed, 12);
    i = 0;
    while i < 3 {
        p(car.motion, i, integer::mul(g(d, i), speed));
        i += 1;
    }
    let basis = at(car.attitude, 18);
    i = 0;
    while i < 3 {
        let mut value = n(0);
        let mut j = 0u16;
        while j < 3 {
            value = value + integer::mul(integer::sar(g(car.motion, j), 8), g(basis, i * 3 + j));
            j += 1;
        }
        p(d, i, integer::sar(value, 4));
        i += 1;
    }
    let mut shifts = 0u16;
    while abs(g(d, 0)) > fix32::from_words(32768, 0)
        || abs(g(d, 1)) > fix32::from_words(32768, 0)
        || abs(g(d, 2)) > fix32::from_words(32768, 0)
    {
        if shifts >= 32 {
            return n(-3);
        }
        i = 0;
        while i < 3 {
            p(d, i, integer::sar(g(d, i), 1));
            i += 1;
        }
        shifts += 1;
    }
    let yaw = attitude::angle(player.angle, g(d, 0), g(d, 2));
    if yaw == n(-1) {
        return n(-2);
    }
    attitude::rotate(car.attitude, 9, 1, -yaw, player.sine);
    n(0)
}
fn wake(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    frame: Array<u16>,
    collision_page: u16,
) -> fix32 {
    let seed = random::next(g(state.runtime.seed, 0));
    p(state.runtime.seed, 0, seed);
    let number = g(frame, 26);
    if number.hi_bits() == 0 && number.lo_bits() <= 499 {
        return n(0);
    }
    let h = ptr(0x8000 + finish_world::WORD);
    let mut period = 7u16;
    if h[9u16] == 1 {
        period = 5;
    } else if h[9u16] == 2 {
        period = 4;
    }
    let word = (seed.lo_bits() >> 4) | (seed.hi_bits() << 12);
    let car = &state.cars[2u16];
    let pose_input = at(effects(), 902);
    if word % period == 0 && g(pose_input, 4) != n(1) {
        if g(car.articulation.as_array(), 0) != n(0) {
            return n(0);
        }
        let event: Buf<u16, 4> = Buf::new([7, 0, 2, 0]);
        let mut e = event.as_array();
        if (word & 7) == 0 && h[9u16] != 0 {
            e[3u16] = 1;
        }
        let saved = read_dseg_page(2);
        unsafe {
            write_dseg_page(2, collision_page);
        }
        let status = weapon_mines::place(scene, state, world, player, frame, e);
        effect_pool::restore(saved);
        if status != n(0) {
            return status;
        }
    } else if g(car.articulation.as_array(), 0) != n(0) {
        return n(0);
    }
    p(pose_input, 4, n(1));
    n(0)
}
fn visibility(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    complete: &complete_check::State,
    frame: Array<u16>,
) -> fix32 {
    let collision_page = DRIVING_WORLD.first_sector() >> 8;
    let mut cars = state.cars;
    if near(&cars[2u16], 8320) {
        if !cars[2u16].visible {
            cars[2u16].visible = true;
            let status = wake(scene, state, world, player, frame, collision_page);
            if status != n(0) {
                return status;
            }
            let status = pose(&cars[2u16], player);
            if status != n(0) {
                return status;
            }
            flags(65535, 32);
        }
        flags(65534, 0);
        return n(0);
    }
    cars[2u16].visible = false;
    let old = g(ai(), 2).lo_bits();
    if !near(&cars[2u16], 19200) {
        flags(65535, 1);
        if old & 2 == 0 {
            p(ai(), 9, g(ai(), 7));
            p(ai(), 10, g(ai(), 8));
            flags(65535, 3);
            p(ai(), 12, n(0));
            p(ai(), 11, n(0));
            let route = routes(collision_page);
            let status = far(state, world, player, &route);
            if status != n(0) {
                return status;
            }
            let core = at(player.prefixes, 100);
            if g(core, 4) != n(0) {
                p(core, 4, n(0));
                p(core, 8, g(core, 6));
                p(core, 9, g(core, 7));
                p(player.drive, 18, g(core, 6));
                p(player.drive, 19, g(core, 7));
                let status = temporary::expire(
                    &complete.shared.temporary,
                    g(core, 5).lo_bits() as i16,
                    &temporary::Context {
                        pool: complete.prepared[0u16].pool,
                        seed: state.runtime.seed,
                        sine: player.sine,
                    },
                );
                if status != n(0) {
                    return status;
                }
            }
            let spin = at(core, 20);
            if g(spin, 7) > n(0) {
                p(spin, 8, n(0));
                p(spin, 7, n(0));
            }
            let boost = at(core, 38);
            if g(boost, 3) != n(0) {
                p(cars[2u16].boost, 0, n(0));
                p(cars[2u16].boost, 3, n(0));
                p(cars[2u16].boost, 1, n(0));
                p(boost, 3, n(0));
            }
        }
    } else if old & 2 != 0 {
        p(ai(), 11, n(0));
        flags(65532, 0);
        p(ai(), 12, n(0));
        p(at(effects(), 902), 5, n(0));
        copy(at(cars[2u16].motion, 6), at(cars[2u16].motion, 18), 6);
        copy(at(cars[2u16].attitude, 36), at(cars[2u16].motion, 18), 6);
        copy(at(ai(), 8), at(cars[2u16].motion, 18), 6);
        p(ai(), 7, g(ai(), 9));
        p(ai(), 8, g(ai(), 10));
        copy(cars[2u16].attitude, at(cars[2u16].attitude, 18), 18);
        let status = wake(scene, state, world, player, frame, collision_page);
        if status != n(0) {
            return status;
        }
    } else if old & 32 != 0 {
        flags(65503, 0);
        p(at(player.inventory, 8), 0, n(0));
    }
    n(0)
}
pub fn before(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    complete: &complete_check::State,
    frame: Array<u16>,
) -> fix32 {
    if state.car_count != 3 || state.slots[0u16] != 2 {
        return n(-1);
    }
    let race = at(player.fields, 60);
    if g(race, 13) != n(0) {
        let score = g(race, 7);
        let mut rank = 1u16;
        let mut slot = 0u16;
        while slot < 9 {
            let peer = state.slots[slot];
            if peer != 65535 && peer != 2 {
                if peer >= state.car_count {
                    return n(-1);
                }
                if score < g(at(player.fields, (peer * 30) as i16), 7) {
                    rank = (rank + 1) & 255;
                }
            }
            slot += 1;
        }
        player.finish_rank = rank;
    }
    let saved2 = read_dseg_page(2);
    let saved3 = read_dseg_page(3);
    unsafe {
        write_dseg_page(2, DRIVING_ACTORS.first_sector() >> 8);
        write_dseg_page(3, complete.page);
    }
    let mut status = visibility(scene, state, world, player, complete, frame);
    if status == n(0) && g(ai(), 2).lo_bits() & 1 != 0 {
        status = n(1);
    }
    effect_pool::restore(saved2);
    unsafe {
        write_dseg_page(3, saved3);
    }
    status
}
fn progress(
    state: &scene_parent::State,
    world: &scene_world::World,
    player: &player_world::Player,
) -> fix32 {
    let header: Buf<u16, 56> = Buf::new([0; 56]);
    let h = header.as_array();
    read(h, player.context, fix32::zero(), 56);
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, world.grid.page);
    }
    let fields = at(player.fields, 60);
    let result: Buf<u16, 4> = Buf::new([0; 4]);
    let root = ptr(0xc000 + 10000);
    let route = player_route::Routes {
        header: at(root, h[8u16] as i16),
        points: at(root, h[10u16] as i16),
        count: h[11u16] / 14,
        ranges: at(root, h[12u16] as i16),
        items: at(root, h[14u16] as i16),
    };
    let mut status = n(-4);
    if g(fields, 0) != n(0) {
        status = player_route::search(
            &route,
            world,
            &state.lifetime.identities,
            &player_route::Query {
                position: at(state.cars[2u16].motion, 18),
                current: g(fields, 1),
                lap: g(fields, 12),
                limit: state.lifetime.limit,
                nearest: false,
                result: result.as_array(),
            },
        );
        if status == n(0) {
            p(fields, 7, g(result.as_array(), 1));
        }
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
pub fn after(
    state: &scene_parent::State,
    world: &scene_world::World,
    player: &player_world::Player,
    frame: Array<u16>,
) -> fix32 {
    let saved2 = read_dseg_page(2);
    let saved3 = read_dseg_page(3);
    unsafe {
        write_dseg_page(2, DRIVING_ACTORS.first_sector() >> 8);
        write_dseg_page(3, player.weapons.effect_page);
    }
    let dt = g(frame, 0);
    let car = &state.cars[2u16];
    let fields = at(player.fields, 60);
    let route = routes(saved2);
    let c = config(player);
    let tail = ptr(0xc000 + player_world::TAIL_WORD + 14);
    let view = finish_drive::Car {
        present: true,
        state: ai(),
        controls: ptr(0xc000 + complete_check::METADATA_WORD + 58),
        horn: tail,
        position: at(car.motion, 18),
        velocity: car.motion,
        reference: car.attitude,
        local_speed: g(car.drive, 2),
        laps: g(fields, 12).lo_bits() as i16,
        visible: car.visible,
    };
    let f = finish_drive::Frame {
        dt,
        seed: state.runtime.seed,
    };
    let mut status = n(0);
    if g(frame, 3) != n(0) {
        if g(ai(), 2).lo_bits() & 1 == 0 {
            status =
                finish_drive::controls(&view, &route, &state.lifetime.identities, world, &c, &f);
        } else {
            let mut i = 0u16;
            while i < 3 {
                p(
                    view.position,
                    i,
                    g(view.position, i) + integer::sar(integer::mul(g(car.motion, i), dt), 14),
                );
                i += 1;
            }
            if g(ai(), 2).lo_bits() & 8 != 0 {
                status = far(state, world, player, &route);
                if status == n(0) {
                    flags(65527, 0);
                }
            }
        }
        if status == n(0) {
            status = finish_routes::waypoints(
                &view,
                &player_route::Car {
                    route: fields,
                    race: at(fields, 22),
                },
                &route,
                &state.lifetime.identities,
                world,
                &f,
            );
            if status == n(1) {
                status = far(state, world, player, &route);
            }
        }
    }
    effect_pool::restore(saved2);
    if status == n(0) && g(frame, 3) != n(0) {
        status = progress(state, world, player);
    }
    if status == n(0) {
        let turret = ptr(0xc000 + complete_check::INPUT_WORD + 52);
        if g(turret, 9) > n(0) {
            let velocity = g(turret, 5);
            p(
                at(effects(), 902),
                5,
                n(
                    (g(at(effects(), 902), 5) + integer::sar(integer::mul(dt, velocity), 6))
                        .lo_bits() as i16,
                ),
            );
            p(
                turret,
                5,
                integer::decay(integer::rate_step(velocity, n(1), dt), 7, dt),
            );
        }
        if car.visible {
            let mut bytes = tail;
            bytes[0u16] = 0;
            bytes[1u16] = 0;
            let timer = bytes[2u16];
            if timer != 0 {
                if dt < n(timer as i16) {
                    bytes[2u16] = timer - dt.lo_bits();
                } else {
                    bytes[2u16] = 0;
                }
            }
        }
    }
    unsafe {
        write_dseg_page(3, saved3);
    }
    status
}
