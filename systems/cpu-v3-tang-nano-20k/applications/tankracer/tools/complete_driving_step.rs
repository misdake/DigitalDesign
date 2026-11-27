//! The complete original physics method; diagnostic work runs after it returns.
use super::*;
pub fn step(
    world: &mut SinglePlayer,
    setup: &driving_step::Setup<'_>,
    own: usize,
    input: drive::Input,
    number: u32,
    mut context: Option<
        &mut tankracer_game::entity_factory::Context<'_, tankracer_game::scene_entity::Handle>,
    >,
) -> driving_step::Step {
    let dt = setup.dt;
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
    let mut full = false;
    let mut devices = driving_devices::Devices {
        models: setup.models,
    };
    if setup.sample_controls != 0 && own == 0 {
        let stage = if setup.sample_controls == 4
            || setup.sample_controls == 7
            || setup.sample_controls == 9
        {
            setup.start_stage
        } else if setup.sample_controls >= 2 {
            4
        } else if number < 3 {
            number as i32
        } else {
            4
        };
        let sequence = if setup.sample_controls == 9 {
            [4u16, 4, 4, 132, 36, 4]
        } else if setup.sample_controls == 8 {
            [4u16, 4, 4, 4, 4, 0]
        } else if setup.sample_controls == 7 {
            [0u16; 6]
        } else if setup.sample_controls == 6 {
            [4u16, 4, 4, 0, 0, 0]
        } else if setup.sample_controls == 5 {
            [260u16, 4, 260, 0, 0, 0]
        } else if setup.sample_controls == 4 {
            [4u16, 4, 132, 68, 4, 0]
        } else if setup.sample_controls == 3 {
            [132u16, 68, 4, 0, 0, 0]
        } else if setup.sample_controls == 2 {
            [80u16, 4, 36, 0, 0, 0]
        } else {
            [0u16, 4, 68, 18, 36, 0]
        };
        let keys = sequence[((number - 1) as usize).min(5)];
        active = stage == 4;
        if setup.sample_controls == 4 || setup.sample_controls == 7 || setup.sample_controls == 9 {
            active = world.vehicles.entries[own].as_ref().unwrap().race.driving;
        } else {
            world.vehicles.entries[own].as_mut().unwrap().race.driving = active;
        }
        let keys = tankracer_game::player_input::Keys {
            left: keys & 1 != 0,
            right: keys & 2 != 0,
            throttle: keys & 4 != 0,
            brake: keys & 8 != 0,
            auxiliary: keys & 16 != 0,
            turret_left: keys & 32 != 0,
            turret_right: keys & 64 != 0,
            fire: keys & 128 != 0,
            horn: keys & 256 != 0,
            ..Default::default()
        };
        if setup.sample_controls >= 3 {
            tankracer_game::vehicle_runtime::rcc_player_probe::update(
                world,
                Some((keys, stage)),
                dt,
                number,
                100000,
                &mut launch,
                &mut devices,
                tankracer_game::vehicle_runtime::rcc_player_probe::Stage::Complete,
                context.as_deref_mut(),
            )
            .unwrap_or_else(|code| {
                panic!(
                    "Player stage {number}: code={code}, route={:?}, projectiles={}, slot={}",
                    world.vehicles.entries[own].as_ref().unwrap().route,
                    world.objects.components.projectiles.entries().count(),
                    world.identity.current_slot
                )
            });
            full = true;
        } else {
            rcc_car_probe::controls(world, own, keys, stage, dt, true).unwrap();
        }
        let car = world.vehicles.entries[own].as_ref().unwrap();
        input = car.input.drive;
        launch = car.input.fast_start != 0;
        let packed_keys = (u16::from(keys.left))
            | (u16::from(keys.right) << 1)
            | (u16::from(keys.throttle) << 2)
            | (u16::from(keys.brake) << 3)
            | (u16::from(keys.auxiliary) << 4)
            | (u16::from(keys.turret_left) << 5)
            | (u16::from(keys.turret_right) << 6)
            | (u16::from(keys.fire) << 7)
            | (u16::from(keys.horn) << 8);
        sampled = if setup.sample_controls >= 3 {
            131072
        } else {
            65536
        } | i32::from(packed_keys)
            | (stage << 9);
    }
    if own != 0 && setup.sample_controls == 9 {
        active = world.vehicles.entries[own].as_ref().unwrap().race.driving;
        tankracer_game::vehicle_runtime::rcc_player_probe::update_car(
            world,
            tankracer_game::object_query::TankId(own),
            None,
            dt,
            number,
            100000,
            &mut launch,
            &mut devices,
            tankracer_game::vehicle_runtime::rcc_player_probe::Stage::Complete,
            context.as_deref_mut(),
        )
        .unwrap();
        full = true;
    }
    let env = drive::Environment {
        dt,
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
    let result = if full {
        Ok(())
    } else if visible {
        rcc_car_probe::physics(
            world,
            tankracer_game::object_query::TankId(own),
            &vehicle::Frame {
                dt,
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
                dt,
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
        peer_stage: None,
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
