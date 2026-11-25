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
            scene_objects::collision::Sound::Deformable => Ok(()),
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
    collide(world, id, dt, mode, false)
}
pub fn deformables(world: &mut SinglePlayer, id: TankId, dt: i32, mode: i32) -> Result<(), i32> {
    collide(world, id, dt, mode, true)
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
    deformable: bool,
) -> Result<(), i32> {
    let hit = world
        .registry
        .hit_environment(world.objects.components.fall_tuning, false, 65536);
    let environment = world.registry.environment(100000);
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
        devices: &mut NoIo,
        hit,
        breakable_class: world.registry.fall,
    };
    if deformable {
        execution.deformables(id, &mut world.identity.context(), environment)
    } else {
        execution.tanks(id)
    }
}
