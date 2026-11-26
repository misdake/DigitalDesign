//! Qualification-only snapshots of original component pool ownership.
use super::*;
use crate::object_query::{EntityId, ObstacleId};
pub fn lap_bindings(world: &crate::vehicle_runtime::SinglePlayer) -> Vec<u16> {
    let c = &world.objects.components;
    let mut out = Vec::new();
    for (id, object) in c.objects.iter().enumerate() {
        let mut row = [0; 6];
        if let Some(object) = object {
            let v = object.data.visibility;
            row[..4].copy_from_slice(&[
                v.add_lap as u16,
                v.remove_lap as u16,
                u16::from(v.entered),
                u16::from(
                    object
                        .data
                        .mesh
                        .and_then(|m| c.surfaces.meshes.get(m))
                        .is_some(),
                ),
            ]);
        }
        row[4] = u16::from(c.crush_initial(ObstacleId(id)).is_ok());
        row[5] = match c.actors.get(ObstacleId(id)) {
            Some(crate::actor_startup::Actor::Powerup(_)) => 1,
            Some(crate::actor_startup::Actor::Train(t)) => {
                if matches!(t.motion, crate::actor_startup::train::Motion::Lander) {
                    3
                } else {
                    2
                }
            }
            Some(crate::actor_startup::Actor::Flock(_)) => 4,
            Some(crate::actor_startup::Actor::Background { .. }) => 5,
            Some(_) => 6,
            None => 0,
        };
        out.extend(row);
    }
    out
}
pub fn lap_attachments(world: &crate::vehicle_runtime::SinglePlayer) -> Vec<u16> {
    let mut out = Vec::new();
    for (id, surface) in world.objects.components.surfaces.entries.iter().enumerate() {
        if let Some(surface) = surface {
            out.extend([
                id as u16,
                1,
                u16::from(surface.sampling.state.records_attached),
            ]);
        }
    }
    out
}
pub fn lap_surface_mode(world: &mut crate::vehicle_runtime::SinglePlayer, id: usize, mode: u16) {
    if mode < 2 {
        world.objects.components.surfaces.entries[id]
            .as_mut()
            .unwrap()
            .sampling
            .state
            .records_attached = mode != 0;
    } else {
        world.objects.components.surfaces.entries[id] = None;
        if mode == 2 {
            world
                .objects
                .components
                .pending_crush
                .push((ObstacleId(id), crate::crush::Initial::default()));
        }
        if mode == 3 {
            world
                .objects
                .components
                .finish_crush_initial(ObstacleId(id));
        }
    }
}
pub fn lap_backups(world: &crate::vehicle_runtime::SinglePlayer) -> Vec<u16> {
    let mut out = Vec::new();
    for (id, fall) in world.objects.components.falls.iter().enumerate() {
        if let Some(basis) = fall.as_ref().and_then(|s| s.basis_backup) {
            out.push(id as u16);
            for v in basis.0 {
                out.extend([v as u16, (v >> 16) as u16]);
            }
        }
    }
    out
}
pub fn lap_step(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    lap: i16,
    flush: bool,
    camera: [i32; 3],
    limit: u32,
) -> Result<(), i32> {
    world.objects.components.camera_position = camera;
    let env = world.registry.environment(limit);
    let mut view = world.objects.view();
    let mut context = world.identity.context();
    if flush {
        view.flush_lap_events(&mut context, env)
    } else {
        view.lap_events(lap, &mut context, env)
    }
}
pub fn tick_ufo(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    entity: EntityId,
    dt: i32,
    slot: u32,
    limit: u32,
    vehicles: bool,
) -> Result<(), i32> {
    world.objects.components.dt = dt;
    let env = world.registry.environment(limit);
    let mut view = world.objects.view();
    if vehicles {
        view.vehicles = Some(&mut world.vehicles);
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
pub fn transfer_shapes(world: &crate::vehicle_runtime::SinglePlayer) -> Vec<u16> {
    world
        .objects
        .components
        .objects
        .iter()
        .map(|v| {
            u16::from(
                v.as_ref()
                    .and_then(|v| v.data.mesh)
                    .and_then(|m| world.objects.components.surfaces.meshes.get(m))
                    .is_some(),
            )
        })
        .collect()
}
pub fn set_ufo_height(world: &mut crate::vehicle_runtime::SinglePlayer, height: i32) {
    world
        .objects
        .components
        .ufo_resources
        .as_mut()
        .unwrap()
        .height = height;
}
pub fn dispatch_vehicle_transfer(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    entity: EntityId,
    slot: u32,
    payload: u16,
    value: u32,
    limit: u32,
    has_vehicles: bool,
) -> Result<u32, i32> {
    let env = world.registry.environment(limit);
    let mut view = world.objects.view();
    if has_vehicles {
        view.vehicles = Some(&mut world.vehicles);
    }
    let argument = match payload {
        1 => Some(crate::entity_factory::Payload::Value(value)),
        2 => Some(crate::entity_factory::Payload::Position([
            12345, -23456, 34567,
        ])),
        _ => None,
    };
    world
        .identity
        .context()
        .dispatch(entity, slot, argument, env, &mut view)
        .map_err(|e| match e {
            crate::entity_factory::Error::Dispatch(crate::dispatch::Error::Method(code)) => code,
            _ => -1,
        })
}
pub fn transfer_object_setup(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    id: ObstacleId,
    mode: u16,
) -> bool {
    if mode == 1 {
        world.objects.components.objects[id.0] = None;
    }
    if let Some(object) = world.objects.components.objects[id.0].as_mut() {
        if mode == 2 {
            object.data.mesh = None;
        }
        if mode == 3 {
            object.data.cell = None;
        }
    }
    if mode == 4 {
        let cell = world
            .objects
            .components
            .object(id)
            .unwrap()
            .data
            .cell
            .unwrap();
        let other = ObstacleId((id.0 + 1) % world.objects.world.colliders.len());
        let mut first = true;
        for z in i32::from(cell.z) - 1..=i32::from(cell.z) + 1 {
            for x in i32::from(cell.x) - 1..=i32::from(cell.x) + 1 {
                if (0..8).contains(&x) && (0..8).contains(&z) {
                    world.objects.world.grid.cells[(z * 8 + x) as usize] =
                        if first { vec![id] } else { vec![other, id] };
                    first = false;
                }
            }
        }
    }
    world.objects.components.objects[id.0]
        .as_ref()
        .and_then(|v| v.data.mesh)
        .and_then(|mesh| world.objects.components.surfaces.meshes.get(mesh))
        .is_some()
}
pub fn dispatch_obstacle_transfer(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    entity: EntityId,
    slot: u32,
    payload: u16,
    value: u32,
    position: [i32; 3],
    limit: u32,
) -> Result<u32, i32> {
    let env = world.registry.environment(limit);
    let argument = match payload {
        1 => Some(crate::entity_factory::Payload::Value(value)),
        2 => Some(crate::entity_factory::Payload::Position(position)),
        _ => None,
    };
    world
        .identity
        .context()
        .dispatch(entity, slot, argument, env, &mut world.objects.view())
        .map_err(|e| match e {
            crate::entity_factory::Error::Dispatch(crate::dispatch::Error::Method(code)) => code,
            _ => -1,
        })
}
pub fn ufo_config(
    world: &crate::vehicle_runtime::SinglePlayer,
) -> (
    Option<crate::dispatch::ClassId>,
    Option<crate::ufo::Resources>,
) {
    (
        world.objects.components.ufo_class,
        world.objects.components.ufo_resources,
    )
}
pub fn clear_ufo_config(world: &mut crate::vehicle_runtime::SinglePlayer, resources: bool) {
    if resources {
        world.objects.components.ufo_resources = None;
    } else {
        world.objects.components.ufo_class = None;
    }
}
pub fn spawn_ufo(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    emitter: Option<EntityId>,
    target: EntityId,
    destination: [i32; 3],
    limit: u32,
    has_vehicles: bool,
) -> Result<EntityId, i32> {
    let env = world.registry.environment(limit);
    let mut view = world.objects.view();
    if has_vehicles {
        view.vehicles = Some(&mut world.vehicles);
    }
    view.spawn_ufo(
        &mut world.identity.context(),
        emitter,
        target,
        destination,
        env,
    )
}
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
