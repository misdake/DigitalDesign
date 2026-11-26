//! Qualification-only access to the original private ground response.
use super::*;
pub fn effect_config(
    world: &SinglePlayer,
    id: TankId,
) -> (
    spray::Tuning,
    smoke::Tuning,
    pickup::Tuning,
    [i32; 3],
    u32,
    bool,
) {
    let p = &world.profiles[id.0];
    (
        p.spray.clone(),
        p.smoke.clone(),
        p.pickup,
        p.light_offset,
        world.settings.light_material,
        world.settings.special,
    )
}
pub fn physics<D: Devices>(
    world: &mut SinglePlayer,
    id: TankId,
    frame: &vehicle::Frame,
    context: Option<&mut factory::Context<'_, Handle>>,
    devices: &mut D,
    limit: u32,
    prepare: bool,
) -> Result<(), i32> {
    let hit = world
        .registry
        .hit_environment(world.objects.components.fall_tuning, false, 65536);
    let environment = world.registry.environment(limit);
    let mut execution = Execution {
        input: frame.input,
        keys: None,
        race_active: world.race_active,
        finish_entity: world.finish_entity,
        finish_started: false,
        vehicles: &mut world.vehicles,
        objects: &mut world.objects,
        shared: &mut world.shared,
        terrain: &world.terrain,
        surfaces: &world.surfaces,
        profiles: &world.profiles,
        player: world.player,
        player_class: world.registry.player,
        settings: world.settings,
        frame: vehicle::Frame {
            dt: frame.dt,
            number: frame.number,
            mode: frame.mode,
            input: frame.input,
            drive: frame.drive,
        },
        camera: (world.camera_position, world.camera_forward),
        camera_basis: world.camera_basis,
        devices,
        hit,
        breakable_class: world.registry.fall,
    };
    if let Some(context) = context {
        prepared_physics(&mut execution, id, context, environment, prepare)
    } else {
        prepared_physics(
            &mut execution,
            id,
            &mut world.identity.context(),
            environment,
            prepare,
        )
    }
}
fn prepared_physics<D: Devices>(
    execution: &mut Execution<'_, D>,
    id: TankId,
    context: &mut factory::Context<'_, Handle>,
    env: factory::Environment<'_>,
    prepare: bool,
) -> Result<(), i32> {
    if prepare {
        let player = execution.vehicles.entries[id.0].as_mut().ok_or(-1)?;
        let result = super::prepare(
            player,
            execution.shared,
            &execution.profiles[id.0],
            &execution.frame,
            execution.camera.0,
            context,
            env,
        );
        player.physics.pose_input.timer = player.prefix.temporary_timer as u16;
        player.physics.blast.protection_timer = player.prefix.spin.speed;
        player.model.config.tank.drive.sidegrip_fraction = player.prefix.sidegrip;
        execution.frame.drive.boost_charge = player.prefix.boost.timer;
        execution.frame.drive.boost_power = player.prefix.boost.extra_speed;
        execution.frame.drive.extra_drive = player.prefix.boost.power;
        result?;
    }
    execution.physics(id, context, env)
}
pub fn inactive<D: Devices>(
    world: &mut SinglePlayer,
    id: TankId,
    frame: &vehicle::Frame,
    devices: &mut D,
) -> Result<(), i32> {
    let hit = world
        .registry
        .hit_environment(world.objects.components.fall_tuning, false, 65536);
    let mut execution = Execution {
        input: frame.input,
        keys: None,
        race_active: world.race_active,
        finish_entity: world.finish_entity,
        finish_started: false,
        vehicles: &mut world.vehicles,
        objects: &mut world.objects,
        shared: &mut world.shared,
        terrain: &world.terrain,
        surfaces: &world.surfaces,
        profiles: &world.profiles,
        player: world.player,
        player_class: world.registry.player,
        settings: world.settings,
        frame: vehicle::Frame {
            dt: frame.dt,
            number: frame.number,
            mode: frame.mode,
            input: frame.input,
            drive: frame.drive,
        },
        camera: (world.camera_position, world.camera_forward),
        camera_basis: world.camera_basis,
        devices,
        hit,
        breakable_class: world.registry.fall,
    };
    execution.inactive_car(id, &world.identity.entities)
}
pub fn fire(
    world: &mut SinglePlayer,
    id: TankId,
    mode: i32,
    limit: u32,
) -> Result<Option<crate::object_query::EntityId>, i32> {
    let car = world.vehicles.entries[id.0].as_mut().ok_or(-1)?;
    let env = world.registry.environment(limit);
    let shot = super::super::weapons::Fire {
        objects: &mut world.objects,
        shared: &mut world.shared,
        context: world.identity.context(),
        environment: env,
        mode,
    }
    .apply(&mut car.physics, &mut car.inventory, &mut car.prefix)?;
    if let Some(entity) = shot {
        let object = world
            .identity
            .entities
            .record(entity)
            .and_then(|r| r.object)
            .and_then(Handle::obstacle)
            .ok_or(-4)?;
        world
            .objects
            .components
            .projectiles
            .get_mut(object)
            .ok_or(-4)?
            .select_target(&world.vehicles)?;
    }
    Ok(shot)
}
pub fn ground(world: &mut SinglePlayer, id: TankId, dt: i32, number: u32) -> Result<(), i32> {
    let player = world
        .vehicles
        .entries
        .get_mut(id.0)
        .and_then(Option::as_mut)
        .ok_or(-1)?;
    super::ground(
        player,
        &mut world.shared,
        &vehicle::Frame {
            dt,
            number,
            mode: 0,
            input: Default::default(),
            drive: Default::default(),
        },
    )
}
pub fn prepare(
    world: &mut SinglePlayer,
    id: TankId,
    frame: &vehicle::Frame,
    camera: [i32; 3],
) -> Result<(), i32> {
    prepare_with_context(world, id, frame, camera, None)
}
pub fn prepare_with_context(
    world: &mut SinglePlayer,
    id: TankId,
    frame: &vehicle::Frame,
    camera: [i32; 3],
    context: Option<&mut factory::Context<'_, Handle>>,
) -> Result<(), i32> {
    let player = world
        .vehicles
        .entries
        .get_mut(id.0)
        .and_then(Option::as_mut)
        .ok_or(-1)?;
    let profile = world.profiles.get(id.0).ok_or(-1)?;
    let classes = &world.objects.world.classes;
    let template = world.registry.environment(100000);
    let env = factory::Environment {
        classes,
        ..template
    };
    if let Some(context) = context {
        super::prepare(
            player,
            &mut world.shared,
            profile,
            frame,
            camera,
            context,
            env,
        )
    } else {
        super::prepare(
            player,
            &mut world.shared,
            profile,
            frame,
            camera,
            &mut world.identity.context(),
            env,
        )
    }
}
pub fn prefix_config(world: &SinglePlayer, id: TankId) -> [i32; 6] {
    let player = world.vehicles.entries[id.0].as_ref().unwrap();
    let profile = &world.profiles[id.0];
    [
        profile.drive.gear_limits[0],
        player.model.config.tank.drive.turbo_amount,
        profile.turbo.player_power,
        profile.turbo.player_loss,
        profile.turbo.ai_power,
        profile.turbo.ai_loss,
    ]
}
pub fn pickup_tuning(world: &SinglePlayer, id: TankId) -> crate::vehicle_scene::pickup::Tuning {
    world.profiles[id.0].pickup
}
pub fn projectile_target(
    state: &mut crate::projectile::State,
    world: &SinglePlayer,
) -> Result<(), i32> {
    state.select_target(&world.vehicles)
}
pub fn tank_hit_unown(car: &mut crate::vehicle_startup::Player, part: usize) {
    car.model.parts[part].as_mut().unwrap().owned = None;
}
pub fn explosion_contact(
    state: &mut crate::explosion::State,
    car: &crate::vehicle_startup::Player,
    contact: crate::explosion::Contact,
    kind: crate::projectile::Kind,
    seed: &mut u32,
) -> Result<(), i32> {
    state.contact(car, contact, kind, seed)
}
pub fn explosion_tick(
    state: &mut crate::explosion::State,
    origin: [i32; 3],
    dt: i32,
    seed: &mut u32,
    resources: crate::explosion::Resources,
    owner: Option<crate::pose::Pose>,
) -> Result<bool, i32> {
    crate::explosion::tick_with(state, origin, dt, seed, resources, owner, |state, _| {
        if state.band != u32::MAX || state.ring != u32::MAX {
            Err(-5)
        } else {
            Ok(())
        }
    })
}

pub fn drive_tuning(world: &SinglePlayer, id: TankId) -> crate::drive::Tuning {
    world.profiles[id.0].drive.clone()
}
pub fn numerics(world: &mut SinglePlayer, value: crate::attitude::Numerics) {
    world.set_numerics(value);
}

// Qualification of sampled controls followed by continuous Tank physics.
// Route/lap/fire dispatch is qualified separately by the full Player probe.
pub fn controls(
    world: &mut SinglePlayer,
    own: usize,
    mut keys: crate::player_input::Keys,
    stage: i32,
    dt: i32,
    race_active: bool,
) -> Result<(), i32> {
    assert!(!keys.fire);
    let car = world.vehicles.entries[own].as_mut().ok_or(-1)?;
    let v = &mut car.physics;
    let outcome = car.input.step(
        &mut car.prefix.countdown_throttle,
        &mut world.shared.input,
        crate::player_input::Frame {
            dt,
            driving: car.race.driving,
            player: 0,
            speed: v.powertrain.local_velocity[2],
            start_stage: stage,
            analog: false,
            analog_longitudinal: false,
            twin_tracks: false,
            track_values: [0; 2],
        },
        crate::player_input::Tuning::default(),
        &mut keys,
    )?;
    if outcome.driving {
        crate::player_collision::step(&mut v.collision, dt, race_active);
        car.turret.center_lock = v.contact.sampling.effect.timer as i16;
        car.turret.tick(
            &mut v.pose_input.turret_yaw,
            crate::turret::Keys {
                left: keys.turret_left,
                right: keys.turret_right,
            },
            dt,
        );
        let fast = outcome.auxiliary_down && v.powertrain.local_velocity[2].wrapping_abs() > 250000;
        let restoring = car.input.drive.alternate && (car.prefix.temporary_timer != 0 || !fast);
        let slide = if restoring {
            let material = usize::try_from(v.contact.sampling.materials[0]).map_err(|_| -1)?;
            world.surfaces.entries.get(material).ok_or(-1)?.slide[0].multiplier
        } else {
            0
        };
        car.brake.step(
            crate::player_brake::Vehicle {
                input: &mut car.input.drive,
                max_turn: &mut car.model.config.tank.drive.max_turn,
                yaw_lock: &mut v.yaw_lock,
            },
            crate::player_brake::Frame {
                dt,
                speed: v.powertrain.local_velocity[2],
                mode: world.settings.mode,
                temporary_timer: car.prefix.temporary_timer,
                down: outcome.auxiliary_down,
            },
            crate::player_brake::Reference {
                slide,
                rotation: car.model.config.tank.drive.rotational_drag.step,
            },
        );
    }
    Ok(())
}
