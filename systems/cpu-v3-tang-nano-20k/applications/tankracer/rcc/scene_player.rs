//! Player control, lap events and fire on the same owners before Tank dispatch.
use crate::dsl_rt::*;
use crate::{
    attitude, integer, player_control, player_route, scene_creation, scene_fire, scene_laps,
    scene_lifecycle, scene_query, scene_world,
};

pub struct Car {
    pub input: player_control::Input,
    pub collision: player_control::Collision,
    pub route: player_route::Car,
    pub brake: player_control::Brake,
    pub turret: Array<u16>,
    pub angle: Array<u16>,
    pub speed: Array<u16>,
    pub effect_timer: Array<u16>,
    pub temporary_timer: Array<u16>,
    pub material: Array<u16>,
    pub weather: Array<u16>,
    pub position: Array<u16>,
    pub rotational_drag: i16,
}
pub struct Frame {
    pub entity: fix32,
    pub number: fix32,
    pub dt: fix32,
    pub keys: u16,
    pub has_keys: bool,
    pub start_stage: fix32,
    pub race_active: bool,
    pub mode: fix32,
}
pub struct Stage {
    pub car: Car,
    pub frame: Frame,
    pub routes: player_route::Routes,
    pub route_count: u16,
    pub route_page: u16,
    pub weather_table: Array<u16>,
    pub slides: Array<u16>,
    pub slide_count: u16,
    pub tuning: Array<u16>,
    pub laps: Array<scene_laps::Context>,
    pub fire: Array<scene_fire::Fire>,
    pub launch_eligible: Array<u16>,
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
fn route_page(stage: &Stage) -> u16 {
    let saved = read_dseg_page(3);
    if stage.route_page != 65535 {
        unsafe {
            write_dseg_page(3, stage.route_page);
        }
    }
    saved
}
pub fn prepare(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    stage: &Stage,
) -> fix32 {
    let car = &stage.car;
    let f = &stage.frame;
    p(stage.launch_eligible, 0, n(0));
    if f.has_keys {
        let frame: Buf<u16, 20> = Buf::new([0; 20]);
        let v = frame.as_array();
        p(v, 0, f.dt);
        p(v, 1, g(car.route.race, 2));
        p(v, 3, g(car.speed, 0));
        p(v, 4, f.start_stage);
        let axes: Buf<u16, 4> = Buf::new([0; 4]);
        let outcome = player_control::input(&car.input, v, stage.tuning, f.keys, axes.as_array());
        if outcome & 1 != 0 {
            player_control::collision_decay(&car.collision, f.dt, f.race_active);
            if f.race_active && stage.route_count != 0 {
                let changed: Buf<u16, 2> = Buf::new([0; 2]);
                let result = changed.as_array();
                let previous = route_page(stage);
                let status = player_route::cross(
                    &stage.routes,
                    world,
                    &store.identities,
                    &car.route,
                    car.position,
                    result,
                );
                unsafe {
                    write_dseg_page(3, previous);
                }
                if status != n(0) {
                    return status;
                }
                let mut laps = stage.laps;
                if g(result, 0) != n(0) {
                    laps[0u16].lap = g(car.route.race, 1).lo_bits() as i16;
                    laps[0u16].flush = false;
                    let status = scene_laps::step(scene, store, runtime, world, weapons, laps);
                    if status != n(0) {
                        return status;
                    }
                }
                laps[0u16].flush = true;
                let status = scene_laps::step(scene, store, runtime, world, weapons, laps);
                if status != n(0) {
                    return status;
                }
            }
            p(car.turret, 4, n(g(car.effect_timer, 0).lo_bits() as i16));
            player_control::turret(car.turret, car.angle, (f.keys >> 5) & 3, f.dt);
            if f.keys & 128 != 0 {
                let mut fire = stage.fire;
                fire[0u16].mode = f.mode;
                let status =
                    scene_fire::apply(scene, store, runtime, world, weapons, &mut fire[0u16]);
                if status != n(0) {
                    return status;
                }
            }
            let fast = outcome & 2 != 0 && abs(g(car.speed, 0)) > fix32::from_words(53392, 3);
            let restoring =
                g(car.input.drive, 4) != n(0) && (g(car.temporary_timer, 0) != n(0) || !fast);
            let mut slide = 0u16;
            if restoring {
                let material = g(car.material, 0);
                if material.hi_bits() != 0 || material.lo_bits() >= stage.slide_count {
                    return n(-1);
                }
                slide = stage.slides[material.lo_bits()];
            }
            let brake_frame: Buf<u16, 10> = Buf::new([0; 10]);
            let b = brake_frame.as_array();
            p(b, 0, f.dt);
            p(b, 1, g(car.speed, 0));
            p(b, 2, f.mode);
            p(b, 3, g(car.temporary_timer, 0));
            p(b, 4, fix32::from_words((outcome >> 1) & 1, 0));
            let reference: Buf<u16, 4> = Buf::new([0; 4]);
            let r = reference.as_array();
            p(r, 0, fix32::from_words(slide, 0));
            p(r, 1, n(car.rotational_drag));
            player_control::brake(&car.brake, b, r);
        }
        if g(car.input.state, 1) != n(0) {
            p(stage.launch_eligible, 0, n(1));
        }
    }
    if f.race_active && stage.route_count != 0 {
        let previous = route_page(stage);
        let status = player_route::progress(
            &stage.routes,
            world,
            &store.identities,
            &car.route,
            &player_route::Frame {
                position: car.position,
                entity: f.entity,
                number: f.number,
                dt: f.dt,
                limit: store.limit,
            },
        );
        unsafe {
            write_dseg_page(3, previous);
        }
        if status != n(0) {
            return status;
        }
    }
    if ((f.entity.lo_bits() ^ f.number.lo_bits()) & 15 == 0 || f.number == n(1))
        && stage.route_count != 0
    {
        let mut score = g(car.route.route, 7);
        if f.number == n(1) {
            score = n(0);
        }
        let mut flags = fix32::from_words(65535, 65535);
        // Scores below three laps are positive, so the ordinary divider is exact.
        if score.hi_bits() < 468 || (score.hi_bits() == 468 && score.lo_bits() < 49152) {
            let lap = integer::div(score, fix32::from_words(16384, 156));
            let part = integer::div(
                score - integer::mul(lap, fix32::from_words(16384, 156)),
                fix32::from_words(8192, 3),
            );
            flags = fix32::from_words(stage.weather_table[lap.lo_bits() * 64 + part.lo_bits()], 0);
        }
        let first = f.number == n(1);
        if flags != fix32::from_words(65535, 65535) && (flags != g(car.weather, 0) || first) {
            if flags != n(0) {
                return n(-5);
            }
            p(car.weather, 0, flags);
        }
    }
    n(0)
}
