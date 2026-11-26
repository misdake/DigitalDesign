//! The complete original physics method; diagnostic work runs after it returns.
use super::*;
pub fn step(
    world: &mut SinglePlayer,
    setup: &driving_step::Setup<'_>,
    own: usize,
    input: drive::Input,
    number: u32,
    context: Option<
        &mut tankracer_game::entity_factory::Context<'_, tankracer_game::scene_entity::Handle>,
    >,
) -> driving_step::Step {
    let before = world.vehicles.entries[own]
        .as_ref()
        .unwrap()
        .physics
        .collision
        .timer;
    let player = world.vehicles.entries[own].as_ref().unwrap().physics.player;
    let before_aux = world
        .objects
        .components
        .objects
        .iter()
        .flatten()
        .filter(|o| o.auxiliary_mesh.is_some())
        .count();
    let mut input = input;
    let mut sampled = 0i32;
    let mut active = true;
    let mut launch = false;
    if setup.sample_controls != 0 && own == 0 {
        let stage = if setup.sample_controls == 2 {
            4
        } else if number < 3 {
            number as i32
        } else {
            4
        };
        let sequence = if setup.sample_controls == 2 {
            [80u16, 4, 36, 0, 0, 0]
        } else {
            [0u16, 4, 68, 18, 36, 0]
        };
        let keys = sequence[((number - 1) as usize).min(5)];
        active = stage == 4;
        world.vehicles.entries[own].as_mut().unwrap().race.driving = active;
        rcc_car_probe::controls(
            world,
            own,
            tankracer_game::player_input::Keys {
                left: keys & 1 != 0,
                right: keys & 2 != 0,
                throttle: keys & 4 != 0,
                brake: keys & 8 != 0,
                auxiliary: keys & 16 != 0,
                turret_left: keys & 32 != 0,
                turret_right: keys & 64 != 0,
                ..Default::default()
            },
            stage,
            33,
            true,
        )
        .unwrap();
        let car = world.vehicles.entries[own].as_ref().unwrap();
        input = car.input.drive;
        launch = car.input.fast_start != 0;
        sampled = 65536 | i32::from(keys) | (stage << 9);
    }
    let env = drive::Environment {
        dt: 33,
        player,
        active,
        launch_eligible: launch,
        ..Default::default()
    };
    let mut control = reference::drive_fixture::frame(&input, &env);
    control.extend([number as i32, 0, own as i32, 0]);
    let visible = world.vehicles.entries[own].as_ref().unwrap().visible;
    control[27] = 256 | i32::from(visible);
    control[29] = sampled;
    let mut devices = driving_devices::Devices {
        models: setup.models,
    };
    let result = if visible {
        rcc_car_probe::physics(
            world,
            tankracer_game::object_query::TankId(own),
            &vehicle::Frame {
                dt: 33,
                number,
                mode: 0,
                input,
                drive: env,
            },
            context,
            &mut devices,
            100000,
            setup.prepare,
        )
    } else {
        rcc_car_probe::inactive(
            world,
            tankracer_game::object_query::TankId(own),
            &vehicle::Frame {
                dt: 33,
                number,
                mode: 0,
                input,
                drive: env,
            },
            &mut devices,
        )
    };
    result.unwrap_or_else(|error| {
        panic!(
            "complete physics: own={own}, frame={number}, ordinary={}, slot={}, error={error}",
            setup.ordinary, world.identity.current_slot
        )
    });
    let v = &world.vehicles.entries[own].as_ref().unwrap().physics;
    let mut cells = std::collections::BTreeSet::new();
    for p in v
        .contact
        .sampling
        .primary
        .points
        .iter()
        .map(|p| p.world)
        .chain(
            v.contact
                .sampling
                .auxiliary
                .iter()
                .filter(|p| p.cached != 0)
                .map(|p| p.world),
        )
    {
        let x = ((p[0] >> 3) - setup.terrain.origin[0]) >> setup.terrain.shifts[0];
        let z = ((p[2] >> 3) - setup.terrain.origin[1]) >> setup.terrain.shifts[1];
        if (0..8).contains(&x) && (0..8).contains(&z) {
            cells.insert((x + z * 8) as usize);
        }
    }
    driving_step::Step {
        control,
        stage: reference::drive_fixture::flatten(&v.powertrain.drive_state(&v.motion)),
        cells,
        tank_hits: usize::from(v.collision.timer != before),
        deform_hits: usize::from(v.collision.deformable.is_some()),
        sampled_points: 0,
        spark_events: 0,
        ordinary_hits: usize::from(
            v.collision.obstacle_impulse != 0
                || world
                    .objects
                    .components
                    .objects
                    .iter()
                    .flatten()
                    .filter(|o| o.auxiliary_mesh.is_some())
                    .count()
                    > before_aux,
        ),
        scenery_ticks: 0,
    }
}
