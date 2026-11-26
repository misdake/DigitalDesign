//! Banked projectile flight and ordered retirement on the live scene owners.
use crate::dsl_rt::*;
use crate::{
    attitude, ground, integer, ordered, projectile, projectile_flight, projectile_ricochet,
    projectile_sweep, projectile_trail, scene_creation, scene_lifecycle, scene_query, scene_world,
};
pub struct Frame {
    pub object: u16,
    // The caller resolves an attached UFO's live target before this flight phase.
    pub flight: projectile_flight::Frame,
    pub context: projectile::Context,
    pub map: Array<ground::World>,
    pub terrain_present: bool,
    pub resources_present: bool,
}
pub struct Finish {
    pub call: scene_creation::Call,
    pub object: u16,
    pub dt: fix32,
    pub action: u16,
}
pub struct Reflection {
    pub object: u16,
    pub vehicle: u16,
    pub cars: projectile_sweep::Cars,
    // Live spin speed of the selected vehicle.
    pub spin: Array<u16>,
    pub context: projectile::Context,
    pub simulation: bool,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn at(data: Array<u16>, word: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(word as i16).as_u16_array() }
}
// The caller restores DSEG3 after accessing the selected collision row.
fn collider(world: &scene_world::World, index: u16) -> Array<u16> {
    if index < world.fixed_count {
        return at(world.colliders, index * 60);
    }
    unsafe {
        write_dseg_page(3, world.dynamic_page);
    }
    at(
        unsafe { Ptr::from_addr(0xc000).as_u16_array() },
        (index - world.fixed_count) * 76,
    )
}
pub fn advance(
    world: &mut scene_world::World,
    weapons: &scene_creation::Weapons,
    frame: &Frame,
    action: &mut projectile_flight::Action,
) -> fix32 {
    if !frame.terrain_present {
        return n(-1);
    }
    if !frame.resources_present {
        return n(-5);
    }
    let id = frame.object;
    if id >= weapons.object_capacity || weapons.directory[id * 2] != 1 {
        return n(-4);
    }
    if id >= world.count {
        return n(-1);
    }
    if id >= world.fixed_count
        && (id - world.fixed_count >= world.dynamic_capacity
            || id - world.fixed_count >= scene_world::DYNAMIC_ROWS)
    {
        return n(-1);
    }
    let saved2 = read_dseg_page(2);
    let saved3 = read_dseg_page(3);
    let pose: Buf<u16, 24> = Buf::new([0; 24]);
    let mut words = pose.as_array();
    let raw = collider(world, id);
    let mut i = 0u16;
    while i < 24 {
        words[i] = raw[2 + i];
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved3);
        write_dseg_page(
            2,
            scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
        );
    }
    let state = projectile::view(at(scene_creation::record(weapons, id, 2), 30));
    let view = projectile::Pose {
        position: pose.as_array(),
        basis: at(pose.as_array(), 6),
    };
    let mut maps = frame.map;
    let status = projectile_flight::advance(
        &state,
        &view,
        &frame.flight,
        &frame.context,
        &mut maps[0u16],
        action,
    );
    unsafe {
        write_dseg_page(2, saved2);
    }
    // Pose writes precede an advance failure; external basis commits only on success.
    let mut raw = collider(world, id);
    i = 0;
    while i < 24 {
        raw[2 + i] = words[i];
        i += 1;
    }
    if status == n(0) {
        i = 0;
        while i < 18 {
            raw[26 + i] = words[6 + i];
            i += 1;
        }
        attitude::put(raw, 29, n(1));
    }
    unsafe {
        write_dseg_page(3, saved3);
    }
    status
}
pub fn finish(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    frame: &Finish,
) -> fix32 {
    if frame.action != 1 && frame.action != 2 {
        return n(0);
    }
    if frame.action == 2 {
        let status = scene_creation::dispatch(
            scene,
            store,
            runtime,
            world,
            weapons,
            &scene_creation::Call {
                entity: frame.call.entity,
                slot: 8,
            },
        );
        if status != n(0) {
            return status;
        }
        let id = fix32::from_words(frame.object, 0);
        let removed = ordered::remove_first(&mut runtime.special, id, store.limit);
        if removed != 0 {
            if removed == 3 {
                return n(-3);
            }
            if removed == 4 {
                return n(-4);
            }
            return n(-1);
        }
    }
    let id = frame.object;
    if id >= weapons.object_capacity || weapons.directory[id * 2] != 1 {
        return n(-4);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
        );
    }
    let state = projectile::view(at(scene_creation::record(weapons, id, 3), 30));
    let status = projectile_trail::fade(&state, frame.dt);
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
// 0 keeps the impact, 1 reflects it; negative values retain source error prefixes.
pub fn reflect(
    world: &mut scene_world::World,
    weapons: &scene_creation::Weapons,
    frame: &Reflection,
) -> fix32 {
    if !frame.cars.present
        || frame.vehicle >= frame.cars.count
        || !frame.cars.records[frame.vehicle].present
    {
        return n(-4);
    }
    let id = frame.object;
    if id >= weapons.object_capacity || weapons.directory[id * 2] != 1 {
        return n(-4);
    }
    let saved3 = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
        );
    }
    let probe = projectile::view(at(scene_creation::record(weapons, id, 3), 30));
    let kind = attitude::get(probe.core, 2);
    unsafe {
        write_dseg_page(3, saved3);
    }
    if attitude::get(frame.spin, 0) <= n(0) || kind == n(2) || kind == n(3) {
        return n(0);
    }
    if !frame.simulation {
        return n(-5);
    }
    if id >= world.count {
        return n(-1);
    }
    if id >= world.fixed_count
        && (id - world.fixed_count >= world.dynamic_capacity
            || id - world.fixed_count >= scene_world::DYNAMIC_ROWS)
    {
        return n(-1);
    }
    let saved2 = read_dseg_page(2);
    let pose: Buf<u16, 24> = Buf::new([0; 24]);
    let mut words = pose.as_array();
    let raw = collider(world, id);
    let mut i = 0u16;
    while i < 24 {
        words[i] = raw[2 + i];
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved3);
        write_dseg_page(
            2,
            scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
        );
    }
    let state = projectile::view(at(scene_creation::record(weapons, id, 2), 30));
    let view = projectile::Pose {
        position: pose.as_array(),
        basis: at(pose.as_array(), 6),
    };
    let mut status =
        projectile_ricochet::reflect(&state, &view, frame.vehicle, &frame.cars, &frame.context);
    if status == n(0) {
        status = n(1);
    }
    unsafe {
        write_dseg_page(2, saved2);
    }
    // Reflection changes the pose directly; the prior external basis remains live.
    let mut raw = collider(world, id);
    i = 0;
    while i < 24 {
        raw[2 + i] = words[i];
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved3);
    }
    status
}
