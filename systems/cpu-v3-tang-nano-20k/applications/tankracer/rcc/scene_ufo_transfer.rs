//! UFO transfer Tick over the shared component, vehicle, route and grid owners.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, integer, ordered, scene_creation, scene_lifecycle, scene_obstacle_transfer,
    scene_query, scene_vehicle_transfer, scene_world,
};
pub struct Tick {
    pub call: scene_creation::Call,
    pub dt: fix32,
    pub resources_present: bool,
    pub height: fix32,
    pub vehicles: Array<scene_vehicle_transfer::View>,
    // Per vehicle: entity, player, body presence, unsigned body radius (i32 pairs).
    pub car_info: Array<u16>,
    pub racers: Array<u16>,
    pub visible: Array<ordered::List>,
    // Live resource-owner result for the object's current mesh.
    pub shapes: Array<u16>,
    // Presence and four byte columns, shared with projectile screen effects.
    pub overlay: Array<u16>,
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
fn at(v: Array<u16>, w: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(w as i16).as_u16_array() }
}
fn error(code: u16) -> fix32 {
    if code == 3 {
        return n(-3);
    }
    if code == 4 {
        return n(-4);
    }
    n(-1)
}
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
fn flags(s: Array<u16>) -> u16 {
    g(s, 1).lo_bits()
}
fn set_flags(s: Array<u16>, value: u16) {
    let f = g(s, 1);
    p(s, 1, fix32::from_words(value, f.hi_bits()));
}
fn live(weapons: &scene_creation::Weapons, id: u16) -> bool {
    id < weapons.object_capacity && scene_creation::component_kind(weapons, id) == 4
}
fn state(weapons: &scene_creation::Weapons, id: u16) -> Array<u16> {
    at(scene_creation::record(weapons, id, 3), 30)
}
fn car_present(tick: &Tick, id: fix32) -> bool {
    tick.vehicles[0u16].present
        && id.hi_bits() == 0
        && id.lo_bits() < tick.vehicles[0u16].count
        && tick.vehicles[0u16].cars[id.lo_bits()].present
}
fn position(
    world: &scene_world::World,
    tick: &Tick,
    kind: u16,
    id: fix32,
    out: Array<u16>,
) -> fix32 {
    if kind == 1 {
        let collider: Buf<u16, 60> = Buf::new([0; 60]);
        let row = collider.as_array();
        if id.hi_bits() != 0 || !scene_world::load_collider(world, id.lo_bits(), row) {
            return n(-4);
        }
        let mut i = 0u16;
        while i < 3 {
            p(out, i, g(at(row, 2), i));
            i += 1;
        }
    } else if kind == 2 {
        if !car_present(tick, id) {
            return n(-4);
        }
        let from = tick.vehicles[0u16].cars[id.lo_bits()]
            .transfer
            .pose
            .base_position;
        let mut i = 0u16;
        while i < 3 {
            p(out, i, g(from, i));
            i += 1;
        }
    } else {
        return n(-5);
    }
    n(0)
}
// Packed 0x07010003: signed low-word step and low-byte damping factor.
fn approach(value: fix32, goal: fix32, dt: fix32) -> fix32 {
    let amount = n((dt.lo_bits() * 3) as i16);
    let factor = fix32::from_words(dt.lo_bits() & 255, 0);
    let mut delta = value - goal;
    if delta > amount {
        delta = delta - amount;
    } else if delta < -amount {
        delta = delta + amount;
    } else {
        delta = n(0);
    }
    let mut result = delta - integer::trunc_shift(fmul_lo32(delta, factor), 7);
    if (delta < n(0) && result > n(0)) || (delta > n(0) && result < n(0)) {
        result = n(0);
    }
    result + goal
}
fn visible(
    runtime: &mut scene_world::Runtime,
    tick: &Tick,
    kind: u16,
    id: fix32,
    show: bool,
    limit: fix32,
) -> fix32 {
    let mut code = 0u16;
    if kind == 1 {
        if show {
            code = ordered::append(&mut runtime.special, id);
        } else {
            code = ordered::remove_first(&mut runtime.special, id, limit);
        }
    } else if kind == 2 {
        if !tick.vehicles[0u16].present {
            return n(-4);
        }
        let mut rows = tick.visible;
        if show {
            code = ordered::append(&mut rows[0u16], id);
        } else {
            code = ordered::remove_first(&mut rows[0u16], id, limit);
        }
    } else {
        return n(-5);
    }
    if code != 0 {
        return error(code);
    }
    n(0)
}
pub struct TargetCall {
    pub entity: fix32,
    pub kind: u16,
    pub slot: u16,
    pub payload: u16,
    pub value: fix32,
    pub position: Array<u16>,
    pub vehicles: Array<scene_vehicle_transfer::View>,
    pub shapes: Array<u16>,
}
fn target(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    call: &TargetCall,
) -> fix32 {
    if call.kind == 2 {
        return scene_vehicle_transfer::dispatch(
            scene,
            store,
            runtime,
            world,
            &call.vehicles[0u16],
            &scene_vehicle_transfer::Call {
                entity: call.entity,
                slot: call.slot,
                payload: call.payload,
                value: call.value,
            },
        );
    }
    let id = entities::handle(&store.identities, call.entity);
    let mut shape = false;
    if id.hi_bits() == 0 && id.lo_bits() < store.object_count {
        shape = call.shapes[id.lo_bits()] != 0;
    }
    scene_obstacle_transfer::dispatch(
        scene,
        store,
        runtime,
        world,
        weapons,
        &scene_obstacle_transfer::Call {
            entity: call.entity,
            slot: call.slot,
            payload: call.payload,
            value: call.value,
            position: call.position,
            shape_present: shape,
            result: call.vehicles[0u16].result,
        },
    )
}
// Returns 0/1 for clear/overlapping, or a negative source error.
fn overlaps(world: &scene_world::World, tick: &Tick, kind: u16, id: fix32) -> fix32 {
    let xyz: Buf<u16, 6> = Buf::new([0; 6]);
    let point = xyz.as_array();
    let code = position(world, tick, kind, id, point);
    if code != n(0) {
        return code;
    }
    let mut entity = n(0);
    let mut radius = n(0);
    if kind == 1 {
        let collider: Buf<u16, 60> = Buf::new([0; 60]);
        let row = collider.as_array();
        if !scene_world::load_collider(world, id.lo_bits(), row) {
            return n(-4);
        }
        entity = g(row, 0);
        radius = fix32::from_words(g(row, 28).lo_bits(), 0);
    } else {
        if !car_present(tick, id) {
            return n(-4);
        }
        let info = at(tick.car_info, id.lo_bits() * 8);
        if g(info, 2) == n(0) {
            return n(-4);
        }
        entity = g(info, 0);
        radius = fix32::from_words(g(info, 3).lo_bits(), 0);
    }
    if !tick.vehicles[0u16].present {
        return n(-5);
    }
    let mut slot = 0u16;
    while slot < 9 {
        let car = tick.racers[slot];
        if car != 65535 {
            if car >= tick.vehicles[0u16].count || !tick.vehicles[0u16].cars[car].present {
                return n(-4);
            }
            let info = at(tick.car_info, car * 8);
            if g(info, 0) != entity {
                if g(info, 2) == n(0) {
                    return n(-4);
                }
                let limit = fmul_lo32(radius + fix32::from_words(g(info, 3).lo_bits(), 0), n(8));
                let peer = tick.vehicles[0u16].cars[car].transfer.pose.base_position;
                if abs(g(point, 0) - g(peer, 0)) < limit && abs(g(point, 2) - g(peer, 2)) < limit {
                    return n(1);
                }
            }
        }
        slot += 1;
    }
    n(0)
}
fn update(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    tick: &mut Tick,
) -> fix32 {
    if !tick.resources_present {
        return n(-5);
    }
    let object = scene_lifecycle::object(store, tick.call.entity);
    if object < n(0) {
        return n(-1);
    }
    let id = object.lo_bits();
    if !live(weapons, id) {
        return n(-1);
    }
    let s = state(weapons, id);
    let entity = g(s, 0);
    let incoming = flags(s);
    let kind = entities::handle_kind(&store.identities, entity);
    if kind == 0 {
        return n(-4);
    }
    let handle = entities::handle(&store.identities, entity);
    let xyz: Buf<u16, 6> = Buf::new([0; 6]);
    let point = xyz.as_array();
    let mut code = position(world, tick, kind, handle, point);
    if code != n(0) {
        return code;
    }
    let mut player = false;
    if kind == 2 {
        player = g(at(tick.car_info, handle.lo_bits() * 8), 1) != n(0);
    }
    let destination: Buf<u16, 6> = Buf::new([0; 6]);
    let dest = destination.as_array();
    let dt = tick.dt;
    let mut call = TargetCall {
        entity,
        kind,
        slot: 9,
        payload: 1,
        value: n(1),
        position: dest,
        vehicles: tick.vehicles,
        shapes: tick.shapes,
    };
    if incoming & 1 != 0 {
        p(s, 5, g(point, 0));
        p(s, 7, g(point, 2));
        p(
            s,
            6,
            approach(g(s, 6), g(point, 1) + tick.height - n(1500), dt),
        );
        let time = n((g(s, 10).lo_bits() + dt.lo_bits()) as i16);
        p(s, 10, time);
        if time > n(800) {
            if incoming & 8 == 0 {
                code = target(scene, store, runtime, world, weapons, &call);
                if code != n(0) {
                    return code;
                }
                call.slot = 11;
                call.value = n(0);
                code = target(scene, store, runtime, world, weapons, &call);
                if code != n(0) {
                    return code;
                }
                if !live(weapons, id) {
                    return n(-1);
                }
                set_flags(s, flags(s) | 12);
            }
            let mut threshold = n(2500);
            if player {
                threshold = n(1000);
            }
            if time > threshold && (incoming & 32 == 0 || g(s, 11) == n(255)) {
                call.slot = 29;
                call.value = n(0);
                code = target(scene, store, runtime, world, weapons, &call);
                if code != n(0) {
                    return code;
                }
                call.slot = 30;
                call.payload = 0;
                code = target(scene, store, runtime, world, weapons, &call);
                if code != n(0) {
                    return code;
                }
                if !live(weapons, id) {
                    return n(-1);
                }
                let mut i = 0u16;
                while i < 3 {
                    p(dest, i, g(s, 2 + i));
                    i += 1;
                }
                call.slot = 10;
                call.payload = 2;
                code = target(scene, store, runtime, world, weapons, &call);
                if code != n(0) {
                    return code;
                }
                if !live(weapons, id) {
                    return n(-1);
                }
                p(s, 3, g(tick.vehicles[0u16].result, 0));
                if kind == 2 {
                    if !car_present(tick, handle) {
                        return n(-4);
                    }
                    let checkpoint = tick.vehicles[0u16].cars[handle.lo_bits()]
                        .transfer
                        .checkpoint;
                    if g(checkpoint, 0) == n(0) {
                        return n(-4);
                    }
                    p(s, 13, n(1));
                    p(s, 14, g(checkpoint, 1));
                    let mut owners = tick.vehicles[0u16].routes;
                    let status = ordered::append(&mut owners[0u16].occupied, g(checkpoint, 1));
                    if status != 0 {
                        return error(status);
                    }
                }
                p(s, 10, n(0));
                set_flags(s, (flags(s) & 65530) | 2);
                code = visible(runtime, tick, kind, handle, false, store.limit);
                if code != n(0) {
                    return code;
                }
            }
            if !live(weapons, id) {
                return n(-1);
            }
            if flags(s) & 32 != 0 && g(s, 10) > n(1400) {
                let mut fade = n((g(s, 11).lo_bits() + dt.lo_bits()) as i16);
                if fade > n(255) {
                    fade = n(255);
                }
                p(s, 11, fade);
                let mut out = tick.overlay;
                out[0u16] = 1;
                out[1u16] = fade.lo_bits() & 255;
                out[2u16] = 255;
                out[3u16] = 255;
                out[4u16] = 255;
            }
        }
    } else if incoming & 2 != 0 {
        if g(s, 9) == n(0) {
            if player {
                let timer = n((g(s, 10).lo_bits() + dt.lo_bits()) as i16);
                p(s, 10, timer);
                if timer > n(200) {
                    p(s, 9, n(1));
                    set_flags(s, flags(s) | 4);
                    p(s, 10, n(0));
                }
            } else {
                let mut x = g(s, 5) - g(point, 0);
                let mut z = g(s, 7) - g(point, 2);
                let mut a = integer::sar(x, 2);
                let mut b = integer::sar(z, 2);
                while a > n(3000) || b > n(3000) {
                    a = integer::sar(a, 1);
                    b = integer::sar(b, 1);
                }
                if a < n(-1500) {
                    a = n(-1500);
                }
                if a > n(1500) {
                    a = n(1500);
                }
                if b < n(-1500) {
                    b = n(-1500);
                }
                if b > n(1500) {
                    b = n(1500);
                }
                x = x - a;
                z = z - b;
                let old_x = x;
                let old_z = z;
                let length = attitude::length2(x, z);
                if length != n(0) {
                    let dx = fmul_lo32(fmul_lo32(x, dt), n(-20));
                    let dz = fmul_lo32(fmul_lo32(z, dt), n(-20));
                    if dx == fix32::from_words(0, 32768) && length == n(-1) {
                        return n(-2);
                    }
                    x = x + integer::div(dx, length);
                    if dz == fix32::from_words(0, 32768) && length == n(-1) {
                        return n(-2);
                    }
                    z = z + integer::div(dz, length);
                }
                if (x < n(0) && old_x > n(0)) || (x > n(0) && old_x < n(0)) {
                    x = n(0);
                }
                if (z < n(0) && old_z > n(0)) || (z > n(0) && old_z < n(0)) {
                    z = n(0);
                }
                p(s, 5, g(point, 0) + x);
                p(s, 7, g(point, 2) + z);
                p(s, 6, approach(g(s, 6), g(s, 3) + tick.height - n(1500), dt));
                if x == n(0) && z == n(0) {
                    p(s, 9, n(1));
                    set_flags(s, flags(s) | 4);
                }
            }
        } else {
            p(s, 5, g(point, 0));
            p(s, 7, g(point, 2));
            p(s, 6, approach(g(s, 6), g(s, 3) + tick.height - n(1500), dt));
            if flags(s) & 16 == 0 {
                call.slot = 29;
                call.value = n(1);
                code = target(scene, store, runtime, world, weapons, &call);
                if code != n(0) {
                    return code;
                }
                if !live(weapons, id) {
                    return n(-1);
                }
                set_flags(s, flags(s) | 16);
                code = visible(runtime, tick, kind, handle, true, store.limit);
                if code != n(0) {
                    return code;
                }
            }
            if !live(weapons, id) {
                return n(-1);
            }
            let timer = n((g(s, 10).lo_bits() + dt.lo_bits()) as i16);
            p(s, 10, timer);
            if timer > n(700) && g(s, 11) == n(0) {
                code = overlaps(world, tick, kind, handle);
                if code < n(0) {
                    return code;
                }
                if code == n(0) {
                    call.slot = 11;
                    call.value = n(1);
                    code = target(scene, store, runtime, world, weapons, &call);
                    if code != n(0) {
                        return code;
                    }
                    call.slot = 30;
                    call.payload = 0;
                    code = target(scene, store, runtime, world, weapons, &call);
                    if code != n(0) {
                        return code;
                    }
                    call.slot = 9;
                    call.payload = 1;
                    call.value = n(0);
                    code = target(scene, store, runtime, world, weapons, &call);
                    if code != n(0) {
                        return code;
                    }
                    if !live(weapons, id) {
                        return n(-1);
                    }
                    set_flags(s, flags(s) & 65529);
                }
            }
            if !live(weapons, id) {
                return n(-1);
            }
            if flags(s) & 32 != 0 && g(s, 11) != n(0) {
                let mut fade = n((g(s, 11).lo_bits() - dt.lo_bits()) as i16);
                if fade < n(0) {
                    fade = n(0);
                }
                p(s, 11, fade);
                let mut out = tick.overlay;
                out[0u16] = 0;
                out[1u16] = 0;
                out[2u16] = 0;
                out[3u16] = 0;
                out[4u16] = 0;
                if fade != n(0) {
                    out[0u16] = 1;
                    out[1u16] = fade.lo_bits() & 255;
                    out[2u16] = 255;
                    out[3u16] = 255;
                    out[4u16] = 255;
                }
            }
        }
    } else {
        let speed = g(s, 8) + fmul_lo32(dt, n(2));
        p(s, 8, speed);
        p(s, 6, g(s, 6) + integer::sar(fmul_lo32(dt, speed), 5));
        if g(s, 6) > fix32::from_words(36864, 1) {
            if g(s, 13) != n(0) {
                let mut owners = tick.vehicles[0u16].routes;
                let status =
                    ordered::remove_first(&mut owners[0u16].occupied, g(s, 14), store.limit);
                if status != 0 {
                    return error(status);
                }
            }
            code = scene_creation::dispatch(
                scene,
                store,
                runtime,
                world,
                weapons,
                &scene_creation::Call {
                    entity: tick.call.entity,
                    slot: 8,
                },
            );
            if code != n(0) {
                return code;
            }
        }
    }
    if !live(weapons, id) {
        return n(-1);
    }
    let sprite = at(s, 34);
    let mut i = 0u16;
    while i < 3 {
        p(sprite, 1 + i, g(s, 5 + i));
        i += 1;
    }
    if flags(s) & 4 != 0 {
        let collider: Buf<u16, 60> = Buf::new([0; 60]);
        let row = collider.as_array();
        if !scene_world::load_collider(world, id, row) {
            return n(-1);
        }
        attitude::rotate(
            at(row, 8),
            0,
            1,
            integer::sar(fmul_lo32(n(9085), dt), 8),
            tick.vehicles[0u16].sine,
        );
        p(at(row, 2), 0, g(s, 5));
        p(at(row, 2), 1, g(s, 6) - tick.height);
        p(at(row, 2), 2, g(s, 7));
        i = 0;
        let mut raw = at(row, 26);
        while i < 18 {
            raw[i] = at(row, 8)[i];
            i += 1;
        }
        p(row, 29, n(1));
        if !scene_world::store_collider(world, id, row) {
            return n(-1);
        }
    }
    n(0)
}
pub fn dispatch(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    tick: &mut Tick,
) -> fix32 {
    let class = entities::class(&store.identities, tick.call.entity);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let method = scene.method(class.lo_bits(), tick.call.slot);
    if method.status != 1 || method.id != 125 {
        return scene_creation::dispatch(scene, store, runtime, world, weapons, &tick.call);
    }
    let saved = g(runtime.slot, 0);
    p(runtime.slot, 0, fix32::from_words(tick.call.slot, 0));
    let id = scene_lifecycle::object(store, tick.call.entity);
    if id < n(0) {
        return n(-1);
    }
    // Resource availability precedes component/target validation in the source.
    if !tick.resources_present {
        return n(-5);
    }
    if !live(weapons, id.lo_bits()) {
        return n(-1);
    }
    let previous = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(
                weapons,
                4,
                weapons.directory[id.lo_bits() * weapons.directory_stride + 1],
            ),
        );
    }
    let code = update(scene, store, runtime, world, weapons, tick);
    unsafe {
        write_dseg_page(3, previous);
    }
    if code == n(0) {
        p(runtime.slot, 0, saved);
    }
    code
}
