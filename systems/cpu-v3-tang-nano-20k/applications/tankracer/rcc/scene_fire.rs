//! Player weapon application and target selection on the live scene owners.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, integer, projectile, projectile_sweep, scene_creation, scene_lifecycle,
    scene_mines, scene_query, scene_world, spin,
};
pub struct Car {
    pub player: bool,
    // Original u32 weapon and u8 ammunition word.
    pub inventory: Array<u16>,
    pub cooldown: Array<u16>,
    pub impact: Array<u16>,
    pub collision_size: Array<u16>,
    pub recoil_rate: Array<u16>,
    pub pitch_rate: Array<u16>,
    pub recoil_mode: Array<u16>,
    pub boost_control: Array<u16>,
    pub spin: Array<u16>,
}
pub struct Fire {
    pub car: Car,
    pub mode: fix32,
    // These descriptors borrow this car's live poses/velocity/physical position.
    pub launch: Array<scene_creation::Launch>,
    pub mine: Array<scene_mines::Placement>,
    pub boost_feedback: Array<u16>,
    pub spin_shared: Array<u16>,
    pub cars: projectile_sweep::Cars,
    // Presence followed by entity i32. Commits only after target selection.
    pub shot: Array<u16>,
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
fn decrement(v: Array<u16>) {
    let mut out = v;
    out[2u16] = (out[2u16] - 1) & 255;
}
pub fn apply(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    fire: &mut Fire,
) -> fix32 {
    let car = &fire.car;
    if g(car.cooldown, 0) >= n(1) {
        let mut out = fire.shot;
        out[0u16] = 0;
        p(at(out, 1), 0, n(0));
        return n(0);
    }
    if car.player {
        p(car.impact, 0, g(car.impact, 0) + n(75));
        p(car.collision_size, 0, g(car.collision_size, 0) + n(50));
    }
    // Audio is an external successful operation at this point in the source.
    let weapon = g(car.inventory, 0);
    let mut fired = false;
    let mut entity = n(0);
    let mut has_shot = false;
    if weapon == n(0)
        || weapon == n(3)
        || weapon == n(4)
        || weapon == n(5)
        || weapon == n(9)
        || weapon == n(10)
    {
        let mut launch = fire.launch;
        let mut kind = 0u16;
        if weapon == n(3) {
            kind = 2;
        } else if weapon == n(4) {
            kind = 3;
        } else if weapon == n(5) {
            kind = 1;
        } else if weapon == n(10) {
            kind = 5;
        }
        launch[0u16].kind = kind;
        entity =
            scene_creation::spawn_projectile(scene, store, runtime, world, weapons, &launch[0u16]);
        if entity < n(0) {
            return entity;
        }
        p(car.recoil_mode, 0, weapon);
        fired = true;
        has_shot = true;
        if weapon != n(0) {
            decrement(car.inventory);
        }
    } else if weapon == n(1) || weapon == n(2) {
        let mut placement = fire.mine;
        placement[0u16].repeat = true;
        placement[0u16].kind = weapon - n(1);
        let created = scene_mines::spawn(scene, store, runtime, world, weapons, &placement[0u16]);
        if created < n(0) {
            return created;
        }
        decrement(car.inventory);
    } else if weapon == n(7) {
        let owner = fire.launch[0u16].owner.entity;
        p(car.boost_control, 0, n(0));
        p(car.boost_control, 1, n(0));
        p(car.boost_control, 2, owner);
        p(car.boost_control, 5, n(1));
        p(car.boost_control, 4, n(0));
        p(car.boost_control, 3, n(1));
        p(fire.boost_feedback, 0, n(-16));
        p(fire.boost_feedback, 1, n(1300));
        p(fire.boost_feedback, 2, n(110));
        decrement(car.inventory);
    } else if weapon == n(8) {
        spin::start(
            car.spin,
            car.player,
            fire.mode,
            fire.spin_shared,
            runtime.seed,
        );
        decrement(car.inventory);
    } else {
        p(car.cooldown, 0, n(300));
        let mut out = fire.shot;
        out[0u16] = 0;
        p(at(out, 1), 0, n(0));
        return n(0);
    }
    if car.inventory[2u16] == 0 {
        p(car.inventory, 0, n(0));
    }
    if fired {
        p(car.recoil_rate, 0, g(car.recoil_rate, 0) - n(600));
        p(car.pitch_rate, 0, g(car.pitch_rate, 0) - n(900));
        p(car.cooldown, 0, n(1000));
    } else {
        p(car.cooldown, 0, n(300));
    }
    if has_shot {
        if entities::handle_kind(&store.identities, entity) != 1 {
            return n(-4);
        }
        let object = entities::handle(&store.identities, entity);
        if object.hi_bits() != 0
            || object.lo_bits() >= weapons.object_capacity
            || weapons.directory[object.lo_bits() * 2] != 1
        {
            return n(-4);
        }
        let id = object.lo_bits();
        let saved = read_dseg_page(2);
        unsafe {
            write_dseg_page(
                2,
                scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
            );
        }
        let state = projectile::view(at(scene_creation::record(weapons, id, 2), 30));
        let code = projectile::select_target(&state, &fire.cars, &fire.launch[0u16].context.tables);
        unsafe {
            write_dseg_page(2, saved);
        }
        if code != n(0) {
            return code;
        }
    }
    let mut out = fire.shot;
    out[0u16] = 0;
    p(at(out, 1), 0, n(0));
    if has_shot {
        out[0u16] = 1;
        p(at(out, 1), 0, entity);
    }
    n(0)
}
