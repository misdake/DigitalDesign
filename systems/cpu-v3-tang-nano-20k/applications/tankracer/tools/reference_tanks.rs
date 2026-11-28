//! Qualification-only access to the original private car collision entry.
use super::*;

struct NoIo;
impl scene_objects::collision::Services for NoIo {
    fn auxiliary_model(
        &mut self,
        _: &str,
    ) -> Result<Option<Arc<tankracer_assets::model::Model>>, i32> {
        panic!("unexpected tank collision resource IO")
    }
    fn sound(
        &mut self,
        sound: scene_objects::collision::Sound,
        _: &mut scene_objects::collision::Collision<'_>,
        _: obstacle::Invocation<'_>,
    ) -> Result<(), i32> {
        match sound {
            scene_objects::collision::Sound::Deformable
            | scene_objects::collision::Sound::Obstacle => Ok(()),
            _ => panic!("unexpected collision audio"),
        }
    }
    fn feedback(
        &mut self,
        _: i32,
        _: &mut scene_objects::collision::Collision<'_>,
        _: obstacle::Invocation<'_>,
    ) -> Result<(), i32> {
        panic!("unexpected tank collision feedback")
    }
}
impl Devices for NoIo {
    fn surface_feedback(&mut self) -> Result<(), i32> {
        panic!("unexpected tank collision surface feedback")
    }
    fn impact_feedback(&mut self, _: i32) -> Result<(), i32> {
        panic!("unexpected tank collision impact feedback")
    }
}
pub fn geometry(world: &SinglePlayer, id: usize) -> object_query::Tank {
    tank_geometry(world.vehicles.entries[id].as_ref().unwrap())
}
pub fn tanks(world: &mut SinglePlayer, id: TankId, dt: i32, mode: i32) -> Result<(), i32> {
    collide(world, id, dt, mode, 0, 100000)
}
pub fn deformables(world: &mut SinglePlayer, id: TankId, dt: i32, mode: i32) -> Result<(), i32> {
    collide(world, id, dt, mode, 1, 100000)
}
pub fn obstacles(world: &mut SinglePlayer, id: TankId, dt: i32, mode: i32) -> Result<(), i32> {
    collide(world, id, dt, mode, 2, 100000)
}
pub fn obstacles_limit(
    world: &mut SinglePlayer,
    id: TankId,
    dt: i32,
    mode: i32,
    limit: u32,
) -> Result<(), i32> {
    collide(world, id, dt, mode, 2, limit)
}
pub fn surface<'a>(
    surfaces: &'a mut crate::scene_collision::Surfaces,
    colliders: &[object_query::Collider],
    entity: Option<object_query::EntityId>,
) -> Result<Option<crate::contact_pipeline::Deformable<'a>>, i32> {
    surfaces.sampling(entity, colliders)
}
fn collide(
    world: &mut SinglePlayer,
    id: TankId,
    dt: i32,
    mode: i32,
    kind: u16,
    work_limit: u32,
) -> Result<(), i32> {
    contacts(world, id, dt, mode, kind, work_limit, None)
}
/// A scheduler invocation supplies its live Context. Standalone qualification
/// borrows the same fields directly from the world's identity owner instead.
pub fn contacts(
    world: &mut SinglePlayer,
    id: TankId,
    dt: i32,
    mode: i32,
    kind: u16,
    work_limit: u32,
    context: Option<&mut factory::Context<'_, Handle>>,
) -> Result<(), i32> {
    contacts_with_devices(world, id, dt, mode, kind, work_limit, context, &mut NoIo)
}
pub fn contacts_with_devices<D: Devices>(
    world: &mut SinglePlayer,
    id: TankId,
    dt: i32,
    mode: i32,
    kind: u16,
    work_limit: u32,
    context: Option<&mut factory::Context<'_, Handle>>,
    devices: &mut D,
) -> Result<(), i32> {
    assert!(kind <= 2);
    let hit = world
        .registry
        .hit_environment(world.objects.components.fall_tuning, false, 65536);
    let environment = world.registry.environment(work_limit);
    let mut execution = Execution {
        input: Default::default(),
        keys: None,
        race_active: true,
        finish_entity: None,
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
            number: world.frame,
            mode,
            input: Default::default(),
            drive: Default::default(),
        },
        camera: ([0; 3], [0; 3]),
        camera_basis: crate::math::Matrix::IDENTITY,
        devices,
        hit,
        breakable_class: world.registry.fall,
    };
    if kind == 0 {
        return execution.tanks(id);
    }
    if let Some(context) = context {
        contact_entry(&mut execution, context, environment, id, kind)
    } else {
        contact_entry(
            &mut execution,
            &mut world.identity.context(),
            environment,
            id,
            kind,
        )
    }
}
fn contact_entry<D: Devices>(
    execution: &mut Execution<'_, D>,
    context: &mut factory::Context<'_, Handle>,
    environment: factory::Environment<'_>,
    id: TankId,
    kind: u16,
) -> Result<(), i32> {
    if kind == 1 {
        execution.deformables(id, context, environment)
    } else {
        execution.obstacles(id, context, environment)
    }
}
