//! Qualification-only snapshots of original component pool ownership.
use super::*;
use crate::object_query::{EntityId, ObstacleId};
pub fn tick_projectile(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    entity: EntityId,
    dt: i32,
    limit: u32,
    simulation: bool,
    has_vehicles: bool,
    has_pool: bool,
    has_temporary: bool,
) -> Result<(), i32> {
    world.objects.components.dt = dt;
    let env = world.registry.environment(limit);
    let mut view = world.objects.view();
    if simulation {
        view.execution = Execution::Simulation {
            random: &mut world.shared.random,
        };
    }
    if has_vehicles {
        view.vehicles = Some(&mut world.vehicles);
    }
    if has_pool {
        view.effect_pool = Some(&mut world.shared.pool);
    }
    if has_temporary {
        view.temporary = Some(&mut world.shared.temporary);
    }
    world
        .identity
        .context()
        .dispatch(entity, 12, None, env, &mut view)
        .map(|_| ())
        .map_err(|e| match e {
            crate::entity_factory::Error::Dispatch(crate::dispatch::Error::Method(code)) => code,
            _ => -1,
        })
}
pub fn reflect_projectile(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    id: ObstacleId,
    vehicle: crate::object_query::TankId,
    simulation: bool,
    has_vehicles: bool,
) -> Result<bool, i32> {
    let mut view = world.objects.view();
    if simulation {
        view.execution = Execution::Simulation {
            random: &mut world.shared.random,
        };
    }
    if has_vehicles {
        view.vehicles = Some(&mut world.vehicles);
    }
    view.rcc_reflect_projectile(id, vehicle)
}
pub fn projectile_state(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    id: ObstacleId,
) -> &mut crate::projectile::State {
    world.objects.components.projectiles.get_mut(id).unwrap()
}
pub fn projectile_flight(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    id: ObstacleId,
    frame: crate::projectile::flight::Frame,
    limit: u32,
    output: &mut Option<crate::projectile::flight::Action>,
) -> Result<(), i32> {
    use crate::projectile::flight::Action;
    let action = world.objects.components.projectiles.advance(
        id,
        &mut world.objects.world.colliders[id.0].pose,
        frame,
        world.objects.components.terrain.as_deref().ok_or(-1)?,
        &mut world.objects.components.overlay,
        Some(&world.vehicles),
    )?;
    *output = Some(action);
    let collider = &mut world.objects.world.colliders[id.0];
    collider.external_basis = Some(collider.pose.matrix);
    if matches!(action, Action::Trails | Action::Retire) {
        if action == Action::Retire {
            let env = world.registry.environment(limit);
            world
                .identity
                .context()
                .scene_action(
                    frame.entity,
                    crate::scene_objects::Action::Deactivate,
                    env,
                    &mut world.objects.view(),
                )
                .map_err(dispatch_code)?;
            crate::spatial::remove_first(&mut world.objects.components.special, &id, limit)
                .map_err(spatial_code)?;
        }
        world
            .objects
            .components
            .projectiles
            .get_mut(id)
            .ok_or(-4)?
            .fade_trails(frame.dt)?;
    }
    Ok(())
}
pub fn remove_mine_component(world: &mut crate::vehicle_runtime::SinglePlayer, id: ObstacleId) {
    world.objects.components.mines.remove(id);
}
pub fn hit_mine(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    target: ObstacleId,
    frame: u32,
    limit: u32,
    has_peers: bool,
) -> Result<(), i32> {
    world.objects.components.frame_group = frame;
    let env = world.registry.environment(limit);
    let mut current = world.vehicles.entries[0].take().ok_or(-4)?;
    let result = (|| {
        let mut cars = collision::Cars {
            current: current.slot,
            visible: current.visible,
            ai: current.ai.as_mut(),
            config: &world.vehicles.ai_config,
            peers: Default::default(),
        };
        if has_peers {
            for (i, entry) in world.vehicles.entries.iter_mut().enumerate() {
                let Some(peer) = entry else {
                    continue;
                };
                let slot = usize::from(peer.slot);
                if slot >= 9 {
                    return Err(-1);
                }
                if world.vehicles.racers[slot] == Some(crate::object_query::TankId(i)) {
                    cars.peers[slot] = Some(peer);
                }
            }
        }
        let (lifecycle, current_slot) = world.identity.split();
        let mut collision = collision::Collision {
            components: &mut world.objects.components,
            lifecycle,
            environment: env,
            cars: has_peers.then_some(cars),
        };
        let mut result = 0;
        collision.hit_mine(
            crate::obstacle_collision::Invocation {
                dt: 0,
                target,
                vehicle: &mut current.physics,
                world: &mut world.objects.world,
                random: &mut world.shared.random,
                current_slot,
                result: &mut result,
            },
            &mut world.shared.pool,
            &mut world.shared.shield,
        )
    })();
    world.vehicles.entries[0] = Some(current);
    result
}
pub fn spawn_mine_explosion(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    position: [i32; 3],
    kind: u32,
    limit: u32,
    has_pool: bool,
) -> Result<EntityId, i32> {
    let env = world.registry.environment(limit);
    let car = &world.vehicles.entries[0].as_ref().ok_or(-4)?.physics;
    let mut view = world.objects.view();
    if has_pool {
        view.effect_pool = Some(&mut world.shared.pool);
    }
    view.spawn_mine_explosion(
        &mut world.identity.context(),
        position,
        car,
        kind,
        &mut world.shared.random,
        env,
    )?;
    let id = view
        .components
        .explosion_world
        .slots
        .iter()
        .find(|slot| slot.entity.is_some() && slot.frame == view.components.frame_group)
        .and_then(|slot| slot.entity)
        .ok_or(-4)?;
    Ok(id)
}
pub fn spawn_weapon_explosion(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    position: [i32; 3],
    kind: crate::projectile::Kind,
    contact: Option<crate::explosion::Contact>,
    limit: u32,
    has_vehicles: bool,
    has_pool: bool,
) -> Result<EntityId, i32> {
    let env = world.registry.environment(limit);
    let mut view = world.objects.view();
    if has_vehicles {
        view.vehicles = Some(&mut world.vehicles);
    }
    if has_pool {
        view.effect_pool = Some(&mut world.shared.pool);
    }
    view.spawn_weapon_explosion(
        &mut world.identity.context(),
        position,
        kind,
        contact,
        &mut world.shared.random,
        env,
    )
}
pub fn tick_explosion(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    entity: EntityId,
    slot: u32,
    dt: i32,
    limit: u32,
    simulation: bool,
    has_vehicles: bool,
    has_pool: bool,
) -> Result<(), i32> {
    world.objects.components.dt = dt;
    let env = world.registry.environment(limit);
    let mut view = world.objects.view();
    if simulation {
        view.execution = Execution::Simulation {
            random: &mut world.shared.random,
        };
    }
    if has_vehicles {
        view.vehicles = Some(&mut world.vehicles);
    }
    if has_pool {
        view.effect_pool = Some(&mut world.shared.pool);
    }
    world
        .identity
        .context()
        .dispatch(entity, slot, None, env, &mut view)
        .map(|_| ())
        .map_err(|e| match e {
            crate::entity_factory::Error::Dispatch(crate::dispatch::Error::Method(code)) => code,
            _ => -1,
        })
}
pub fn spawn_explosion(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    position: [i32; 3],
    limit: u32,
) -> Result<EntityId, i32> {
    let env = world.registry.environment(limit);
    world
        .objects
        .spawn_explosion(&mut world.identity, position, &mut world.shared.random, env)
}
pub fn dispatch_weapon_effects(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    entity: EntityId,
    slot: u32,
    limit: u32,
    has_pool: bool,
) -> Result<(), i32> {
    let env = world.registry.environment(limit);
    let mut view = world.objects.view();
    if has_pool {
        view.effect_pool = Some(&mut world.shared.pool);
    }
    world
        .identity
        .context()
        .dispatch(entity, slot, None, env, &mut view)
        .map(|_| ())
        .map_err(|e| match e {
            crate::entity_factory::Error::Dispatch(crate::dispatch::Error::Method(code)) => code,
            _ => -1,
        })
}
pub fn tick_mine(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    entity: EntityId,
    dt: i32,
    number: u32,
    limit: u32,
) -> Result<(), i32> {
    world.objects.components.dt = dt;
    world.objects.components.frame_group = number;
    let env = world.registry.environment(limit);
    world
        .identity
        .context()
        .dispatch(
            entity,
            crate::scene_loading::TICK,
            None,
            env,
            &mut world
                .objects
                .updates_with_vehicles(&mut world.shared.random, &mut world.vehicles),
        )
        .map(|_| ())
        .map_err(|e| match e {
            crate::entity_factory::Error::Dispatch(crate::dispatch::Error::Method(code)) => code,
            _ => -1,
        })
}
pub fn set_mine_owner(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    id: ObstacleId,
    owner: Option<EntityId>,
) {
    world.objects.components.mines.get_mut(id).unwrap().owner = owner;
}
pub fn set_mine_kind(world: &mut crate::vehicle_runtime::SinglePlayer, id: ObstacleId, kind: u32) {
    world.objects.components.mines.get_mut(id).unwrap().kind = kind;
}
pub fn spawn_mine(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    owner: crate::mine::Owner,
    repeat: bool,
    kind: u32,
    limit: u32,
) -> Result<EntityId, i32> {
    let env = world.registry.environment(limit);
    world
        .objects
        .view()
        .spawn_mine(&mut world.identity.context(), owner, repeat, kind, env)
}
pub fn spawn_projectile(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    owner: crate::projectile::launch::Owner,
    kind: crate::projectile::Kind,
    limit: u32,
) -> Result<EntityId, i32> {
    let env = world.registry.environment(limit);
    world.objects.view().spawn_projectile(
        &mut world.identity.context(),
        owner,
        kind,
        &mut world.shared.random,
        env,
    )
}
pub fn create_weapon(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    class: crate::dispatch::ClassId,
    limit: u32,
) -> Result<EntityId, i32> {
    let env = world.registry.environment(limit);
    world
        .identity
        .context()
        .create(class, env, &mut world.objects.view())
        .map_err(|e| match e {
            crate::entity_factory::Error::Dispatch(crate::dispatch::Error::Method(code)) => code,
            _ => -1,
        })
}
pub fn dispatch_weapon(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    entity: EntityId,
    slot: u32,
    limit: u32,
) -> Result<(), i32> {
    let env = world.registry.environment(limit);
    world
        .identity
        .context()
        .dispatch(entity, slot, None, env, &mut world.objects.view())
        .map(|_| ())
        .map_err(|e| match e {
            crate::entity_factory::Error::Dispatch(crate::dispatch::Error::Method(code)) => code,
            _ => -1,
        })
}
pub fn allocate_transient(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    entity: EntityId,
) -> Result<ObstacleId, i32> {
    let env = world.registry.environment(100000);
    let mut result = 0;
    let mut call = crate::entity_factory::Invocation {
        context: world.identity.context(),
        entity,
        payload: None,
        result: &mut result,
    };
    world
        .objects
        .view()
        .allocate_object(&mut call, allocation::Lifetime::Transient)
        .map(|(_, id)| id)
}
pub fn release_transient(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    entity: EntityId,
) -> Result<(), i32> {
    let env = world.registry.environment(100000);
    world
        .identity
        .context()
        .dispatch(
            entity,
            crate::scene_loading::RELEASE,
            None,
            env,
            &mut world.objects.view(),
        )
        .map(|_| ())
        .map_err(|e| match e {
            crate::entity_factory::Error::Dispatch(crate::dispatch::Error::Method(code)) => code,
            _ => -1,
        })
}
pub fn clear_terrain(components: &mut Components) {
    components.terrain = None;
}
pub fn lists(components: &Components) -> [Vec<i32>; 5] {
    [
        components.free_slots.iter().map(|id| id.0 as i32).collect(),
        components
            .lap_objects
            .iter()
            .map(|id| id.0 as i32)
            .collect(),
        components.signs.iter().map(|id| id.0 as i32).collect(),
        components
            .lap_events
            .enter
            .iter()
            .map(|id| id.0 as i32)
            .collect(),
        components
            .lap_events
            .leave
            .iter()
            .map(|id| id.0 as i32)
            .collect(),
    ]
}
