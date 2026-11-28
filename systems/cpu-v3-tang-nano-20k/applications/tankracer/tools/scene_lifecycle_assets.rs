//! Actual ordinary scene destruction and shared identity/list snapshots.
use super::identity_assets;
use tankracer_game::{
    dispatch, entity_factory as factory,
    object_query::EntityId,
    scene_loading,
    scene_objects::{rcc_lifecycle_probe, Execution, View},
    vehicle_runtime::SinglePlayer,
};
pub fn row(world: &SinglePlayer, id: usize) -> [u16; 8] {
    let c = &world.objects.components;
    let fall = c.falls.get(id).and_then(Option::as_ref);
    let bounce = c.bounces.get(id).and_then(Option::as_ref);
    let mut r = [0u16; 8];
    r[0] = if bounce.is_some() {
        2
    } else if fall.is_some() {
        1
    } else {
        0
    };
    if fall.is_some_and(|s| s.attached.is_some()) {
        r[7] |= 64;
    }
    if bounce.is_some_and(|s| s.reference_alias) {
        r[3] |= 1;
    }
    if let Some(object) = c.objects.get(id).and_then(Option::as_ref) {
        r[7] |= 1
            | (u16::from(object.transient) << 1)
            | (u16::from(object.data.auxiliary.is_some()) << 2)
            | (u16::from(object.data.debris.is_some()) << 3)
            | (u16::from(object.auxiliary_mesh.is_some()) << 4)
            | (u16::from(object.auxiliary_motion.is_some()) << 5);
        if object.auxiliary_active != 0 {
            r[7] |= 128;
        }
        if object.data.mesh.is_some() {
            r[3] |= 2;
        }
        if let Some(cell) = object.data.cell {
            r[4..7].copy_from_slice(&[cell.x as u16, cell.z as u16, 1]);
        }
    }
    r
}
pub fn lists(world: &SinglePlayer) -> Vec<Vec<i32>> {
    let mut lists = vec![
        world
            .identity
            .ticks
            .order
            .iter()
            .map(|e| e.0 as i32)
            .collect(),
        world
            .identity
            .queues
            .notifications
            .iter()
            .map(|e| e.0 as i32)
            .collect(),
        world
            .identity
            .queues
            .destruction
            .iter()
            .map(|e| e.0 as i32)
            .collect(),
        world
            .objects
            .components
            .ordinary
            .iter()
            .map(|e| e.0 as i32)
            .collect(),
        world
            .objects
            .components
            .special
            .iter()
            .map(|e| e.0 as i32)
            .collect(),
    ];
    lists.extend(rcc_lifecycle_probe::lists(&world.objects.components));
    lists
}
pub fn state(world: &SinglePlayer, id: usize) -> Vec<i32> {
    let mut out = vec![
        world.identity.current_slot as i32,
        world.shared.random as i32,
        world.identity.ticks.next.map_or(-1, |e| e.0 as i32),
        world.identity.ticks.flags.len() as i32,
    ];
    out.extend(packed(&identity_assets::encode(&world.identity.entities)));
    out.extend(world.identity.ticks.flags.iter().map(|v| *v as i32));
    for ids in lists(world) {
        out.push(ids.len() as i32);
        out.extend(ids);
    }
    out.extend(
        world
            .objects
            .world
            .grid
            .cells
            .iter()
            .map(|v| v.len() as i32),
    );
    out.extend(
        world
            .objects
            .world
            .grid
            .cells
            .iter()
            .flatten()
            .map(|id| id.0 as i32),
    );
    out.extend(packed(&row(world, id)));
    out
}
pub fn input(
    world: &SinglePlayer,
    id: usize,
    action: usize,
    limit: u32,
    scene_words: usize,
) -> Vec<i32> {
    let mut out = vec![
        action as i32,
        limit as i32,
        id as i32,
        world.objects.world.objects.len() as i32,
        world.registry.generic.0 as i32,
        world.registry.obstacle.0 as i32,
        world.objects.world.colliders[id].entity.0 as i32,
        scene_words as i32,
    ];
    out.extend(state(world, id));
    out
}
fn code(error: factory::Error) -> i32 {
    match error {
        factory::Error::Dispatch(dispatch::Error::Method(code)) => code,
        _ => -1,
    }
}
pub fn run(world: &mut SinglePlayer, id: usize, action: usize, limit: u32) -> i32 {
    let entity = world.objects.world.colliders[id].entity;
    let env = world.registry.environment(limit);
    let mut view = View {
        execution: Execution::Simulation {
            random: &mut world.shared.random,
        },
        world: &mut world.objects.world,
        components: &mut world.objects.components,
        vehicles: Some(&mut world.vehicles),
        effect_pool: Some(&mut world.shared.pool),
        temporary: Some(&mut world.shared.temporary),
    };
    let mut context = world.identity.context();
    let result = if action == 0 {
        context.drain(scene_loading::STOP, env, &mut view)
    } else if action == 1 {
        context.destroy(entity, env, &mut view)
    } else if action == 2 {
        context
            .dispatch(entity, scene_loading::STOP, None, env, &mut view)
            .map(|_| ())
    } else {
        let slot = if action == 3 {
            scene_loading::DEACTIVATE
        } else {
            scene_loading::AUXILIARY_COMPLETE
        };
        context
            .dispatch(entity, slot, None, env, &mut view)
            .map(|_| ())
    };
    result.err().map_or(0, code)
}
pub fn prepare(world: &mut SinglePlayer, id: usize, action: usize, variant: usize) {
    let entity = world.objects.world.colliders[id].entity;
    world.identity.current_slot = 17;
    if variant == 1 || variant == 2 {
        world.identity.ticks.enable(entity).unwrap();
        world.identity.ticks.next = Some(entity);
    }
    if variant == 2 {
        world
            .identity
            .queues
            .queue(&mut world.identity.ticks, entity, false, 100000)
            .unwrap();
    }
    if variant == 3 {
        world.objects.components.objects[id]
            .as_mut()
            .unwrap()
            .data
            .cell = None;
    }
    if variant == 4 {
        world.identity.ticks.flags.truncate(entity.0 as usize);
    }
    if variant == 5 {
        world.objects.components.objects[id] = None;
    }
    if variant == 6 {
        world.objects.components.falls[id] = None;
        world.objects.components.bounces[id] = None;
    }
    if variant == 7 {
        world.identity.entities.record_mut(entity).unwrap().class = None;
    }
    if variant == 8 {
        world.identity.entities.detach(entity).unwrap();
    }
    if variant == 9 {
        world.objects.components.objects[id]
            .as_mut()
            .unwrap()
            .transient = true;
        let index = tankracer_game::object_query::ObstacleId(id);
        world.objects.components.ordinary.extend([index, index]);
        world.objects.components.special.extend([index, index]);
        world.objects.components.lap_objects.extend([index, index]);
        world.objects.components.signs.extend([index, index]);
        world
            .objects
            .components
            .lap_events
            .enter
            .extend([entity, EntityId(1), entity]);
        world
            .objects
            .components
            .lap_events
            .leave
            .extend([entity, EntityId(2), entity]);
    }
    if variant == 10 {
        world.identity.entities.record_mut(entity).unwrap().class = Some(world.registry.explosion);
    }
    if variant == 11 {
        world.identity.entities.record_mut(entity).unwrap().object = None;
    }
    if action == 0 {
        // Queue creation is part of this case's initial state, so a zero drain
        // budget still observes the original queue/flag writes.
        if variant == 4 {
            world.identity.ticks.flags.resize(256, 0);
        }
        world
            .identity
            .queues
            .queue(&mut world.identity.ticks, entity, true, 100000)
            .unwrap();
        if variant == 4 {
            world.identity.ticks.flags.truncate(entity.0 as usize);
        }
    }
}

fn packed(words: &[u16]) -> Vec<i32> {
    assert!(words.len().is_multiple_of(2));
    words
        .chunks_exact(2)
        .map(|v| i32::from(v[0]) | (i32::from(v[1]) << 16))
        .collect()
}
