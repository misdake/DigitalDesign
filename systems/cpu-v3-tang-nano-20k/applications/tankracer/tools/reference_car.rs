//! Qualification-only access to the original private ground response.
use super::*;
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
