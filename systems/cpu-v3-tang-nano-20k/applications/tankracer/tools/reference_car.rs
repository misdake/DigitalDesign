//! Qualification-only access to the original private ground response.
use super::*;
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
