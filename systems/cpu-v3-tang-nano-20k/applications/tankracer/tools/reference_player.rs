//! Call the extracted original Player stage on the race's authoritative owners.
use super::*;
pub enum Stage {
    BeforeTank,
    Complete,
    AfterPhysics {
        before: Option<[Option<i16>; 4]>,
        result: Result<(), i32>,
    },
}
pub fn gun_z(world: &SinglePlayer) -> Option<[Option<i16>; 4]> {
    super::super::car::gun_z(
        &world.vehicles.entries[world.player.0]
            .as_ref()
            .unwrap()
            .model,
    )
}
pub fn initialize_routes(world: &mut SinglePlayer) -> Result<(), super::super::Error> {
    world.initialize_player_route(100000)
}
struct Device;
impl scene_objects::collision::Services for Device {
    fn sound(
        &mut self,
        _: scene_objects::collision::Sound,
        _: &mut scene_objects::collision::Collision<'_>,
        _: obstacle::Invocation<'_>,
    ) -> Result<(), i32> {
        Ok(())
    }
    fn feedback(
        &mut self,
        _: i32,
        _: &mut scene_objects::collision::Collision<'_>,
        _: obstacle::Invocation<'_>,
    ) -> Result<(), i32> {
        Ok(())
    }
}
impl Devices for Device {
    fn surface_feedback(&mut self) -> Result<(), i32> {
        Ok(())
    }
    fn impact_feedback(&mut self, _: i32) -> Result<(), i32> {
        Ok(())
    }
}
pub fn before_tank(
    world: &mut SinglePlayer,
    keys: Option<(player_input::Keys, i32)>,
    dt: i32,
    number: u32,
    limit: u32,
    launch: &mut bool,
) -> Result<(), i32> {
    update(
        world,
        keys,
        dt,
        number,
        limit,
        launch,
        &mut Device,
        Stage::BeforeTank,
    )
}
pub fn update<D: Devices>(
    world: &mut SinglePlayer,
    keys: Option<(player_input::Keys, i32)>,
    dt: i32,
    number: u32,
    limit: u32,
    launch: &mut bool,
    devices: &mut D,
    stage: Stage,
) -> Result<(), i32> {
    world.frame = number;
    world.objects.components.dt = dt;
    world.objects.components.frame_group = number;
    world.objects.components.mode = world.settings.mode;
    world.objects.components.camera_position = world.camera_position;
    world.objects.components.camera_basis = world.camera_basis;
    let id = world.player;
    let input = world.vehicles.entries[id.0].as_ref().ok_or(-1)?.input.drive;
    let hit = world
        .registry
        .hit_environment(world.objects.components.fall_tuning, false, 65536);
    let environment = world.registry.environment(limit);
    let mut execution = Execution {
        input,
        keys,
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
            dt,
            number,
            mode: world.settings.mode,
            input,
            drive: drive::Environment::default(),
        },
        camera: (world.camera_position, world.camera_forward),
        camera_basis: world.camera_basis,
        devices,
        hit,
        breakable_class: world.registry.fall,
    };
    let entity = execution.vehicles.entries[id.0]
        .as_ref()
        .ok_or(-1)?
        .physics
        .entity;
    let mut result = 0;
    let mut call = factory::Invocation {
        entity,
        payload: None,
        result: &mut result,
        context: world.identity.context(),
    };
    let check_result;
    let status = match stage {
        Stage::BeforeTank => {
            check_result = false;
            execution.rcc_before_tank(id, &mut call, environment)
        }
        Stage::Complete => {
            check_result = true;
            execution.update_car(id, &mut call, environment)
        }
        Stage::AfterPhysics { before, result } => {
            check_result = true;
            execution.rcc_after_physics(id, &mut call, before, result)
        }
    };
    if check_result && status.is_ok() {
        assert_eq!(result, entity.0);
    }
    *launch = execution.frame.drive.launch_eligible;
    status
}
pub fn effects(world: &mut SinglePlayer, dt: i32, number: u32) -> Result<(), i32> {
    let id = world.player;
    let input = world.vehicles.entries[id.0].as_ref().ok_or(-1)?.input.drive;
    super::super::car::effects(
        &mut world.vehicles,
        id,
        &mut world.shared,
        &world.profiles[id.0],
        world.settings,
        &vehicle::Frame {
            dt,
            number,
            mode: world.settings.mode,
            input,
            drive: Default::default(),
        },
        (world.camera_position, world.camera_forward),
    )
}
pub fn effect_tuning(
    world: &SinglePlayer,
) -> (
    spray::Tuning,
    smoke::Tuning,
    pickup::Tuning,
    [i32; 3],
    u32,
    bool,
) {
    let p = &world.profiles[world.player.0];
    (
        p.spray.clone(),
        p.smoke.clone(),
        p.pickup.clone(),
        p.light_offset,
        world.settings.light_material,
        world.settings.special,
    )
}
pub fn special_effects(world: &mut SinglePlayer, enabled: bool) {
    world.settings.special = enabled;
}
