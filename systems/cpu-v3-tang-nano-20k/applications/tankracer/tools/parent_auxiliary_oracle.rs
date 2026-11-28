//! Actual Fall/Closer contact, persistent update and resource destruction chains.
mod identity_assets;
mod mesh_store_snapshot;
#[allow(dead_code)]
mod reference_world;
#[allow(dead_code)]
mod scene_export;
#[allow(dead_code)]
mod scene_lifecycle_assets;
#[allow(dead_code)]
mod terrain_assets;
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Arc};
use tankracer_assets::model::Model;
use tankracer_game::{
    closer::Phase,
    dispatch, entity_factory, mesh_break,
    object_query::ObstacleId,
    obstacle_collision, scene_loading,
    scene_meshes::Meshes,
    scene_objects::{
        self,
        collision::{Collision, Services, Sound},
        Execution, View,
    },
    vehicle_runtime::SinglePlayer,
};
fn wide(out: &mut Vec<u16>, v: i32) {
    out.extend([v as u16, (v >> 16) as u16]);
}
fn packed(values: &[i32]) -> Vec<u16> {
    values
        .iter()
        .flat_map(|v| [*v as u16, (*v >> 16) as u16])
        .collect()
}
fn motion(out: &mut Vec<u16>, state: Option<&mesh_break::State>) {
    out.push(state.map_or(0, |s| s.faces.len() as u16));
    if let Some(s) = state {
        for v in [
            s.retired as i32,
            s.ground_height,
            i32::from(s.prepared),
            s.lifetime,
        ] {
            wide(out, v);
        }
        for f in &s.faces {
            for v in f.velocity.into_iter().chain(f.angular).chain([f.life]) {
                wide(out, v);
            }
            out.extend([f.extra_vertex as u16, f.mode]);
        }
    }
}
fn parent(world: &SinglePlayer, id: usize, kind: u16) -> Vec<i32> {
    let mut values = vec![0; 5];
    if kind == 1 {
        if let Some(s) = world.objects.components.falls[id].as_ref() {
            values = vec![
                s.angular_velocity,
                s.rotation,
                s.turn_rate,
                s.slope_limit,
                s.last_vehicle.map_or(-1, |e| e.0 as i32),
            ];
        }
    } else if let Some(s) = world.objects.components.closers[id].as_ref() {
        values[..3].copy_from_slice(&[
            s.state.speed,
            s.state.flags as i32,
            match s.state.phase {
                Phase::Uninitialized => 0,
                Phase::Waiting => 1,
                Phase::Closing => 2,
                Phase::Fragments => 3,
            },
        ]);
    }
    values
}
struct Devices {
    model: Arc<Model>,
    status: i32,
}
impl Services for Devices {
    fn auxiliary_model(&mut self, _: &str) -> Result<Option<Arc<Model>>, i32> {
        if self.status < 0 {
            Err(self.status)
        } else if self.status > 0 {
            Ok(None)
        } else {
            Ok(Some(Arc::clone(&self.model)))
        }
    }
    fn sound(
        &mut self,
        _: Sound,
        _: &mut Collision<'_>,
        _: obstacle_collision::Invocation<'_>,
    ) -> Result<(), i32> {
        Ok(())
    }
    fn feedback(
        &mut self,
        _: i32,
        _: &mut Collision<'_>,
        _: obstacle_collision::Invocation<'_>,
    ) -> Result<(), i32> {
        Ok(())
    }
}
fn invoke(
    world: &mut SinglePlayer,
    id: usize,
    kind: u16,
    model: &Arc<Model>,
    control: [i32; 5],
) -> (i32, u32) {
    let [action, dt, mode, limit, model_status] = control;
    let entity = world.objects.world.colliders[id].entity;
    if action == 0 {
        let incoming = world.vehicles.primary().unwrap().physics.entity;
        let tuning = world.objects.components.fall_tuning;
        let hit = world.registry.hit_environment(tuning, false, limit as u32);
        let (lifecycle, slot) = world.identity.split();
        let mut collision = Collision {
            components: &mut world.objects.components,
            lifecycle,
            environment: world.registry.environment(limit as u32),
            cars: None,
        };
        let mut car = obstacle_collision::State {
            entity: incoming,
            ..Default::default()
        };
        car.motion.velocity = [250000, -150000, 500000];
        car.powertrain.local_velocity[2] = 300000;
        let mut result = 0x7182;
        let code = collision
            .method(
                if kind == 1 {
                    tankracer_game::breakable::BREAKABLE_HIT
                } else {
                    scene_objects::CLOSER_HIT
                },
                obstacle_collision::Invocation {
                    dt,
                    target: ObstacleId(id),
                    vehicle: &mut car,
                    world: &mut world.objects.world,
                    random: &mut world.shared.random,
                    current_slot: slot,
                    result: &mut result,
                },
                hit,
                &mut Devices {
                    model: Arc::clone(model),
                    status: model_status,
                },
                true,
            )
            .err()
            .unwrap_or(0);
        return (code, result);
    }
    if action == 2 {
        if let Some(s) = world.objects.components.objects[id]
            .as_mut()
            .and_then(|o| o.auxiliary_motion.as_mut())
        {
            for face in &mut s.faces {
                face.life = 1;
            }
        }
        return (0, 0);
    }
    if action == 4 {
        let mesh = world.objects.components.objects[id]
            .as_ref()
            .unwrap()
            .auxiliary_mesh
            .unwrap();
        world
            .objects
            .components
            .surfaces
            .meshes
            .release_private(mesh)
            .unwrap();
        return (0, 0);
    }
    if action == 5 {
        world.objects.components.objects[id]
            .as_mut()
            .unwrap()
            .auxiliary_mesh = Some(tankracer_game::scene_meshes::rcc_probe::id(65534));
        return (0, 0);
    }
    world.objects.components.dt = dt;
    world.objects.components.mode = mode;
    world.objects.components.frame_group = 0;
    let env = world.registry.environment(limit as u32);
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
    if action == 3 {
        let code = match context.destroy(entity, env, &mut view) {
            Ok(()) => 0,
            Err(entity_factory::Error::Dispatch(dispatch::Error::Method(code))) => code,
            Err(_) => -1,
        };
        return (code, 0);
    }
    match context.dispatch(entity, scene_loading::TICK, None, env, &mut view) {
        Ok(result) => (0, result),
        Err(entity_factory::Error::Dispatch(dispatch::Error::Method(code))) => (code, 0),
        Err(_) => (-1, 0),
    }
}
fn snapshot(
    world: &SinglePlayer,
    id: usize,
    kind: u16,
    status: i32,
    result: u32,
    tables: &mut Vec<Value>,
    name: &str,
) -> Vec<u16> {
    let mut out = vec![];
    wide(&mut out, status);
    wide(&mut out, result as i32);
    out.extend(packed(&scene_lifecycle_assets::state(world, id)));
    out.extend(packed(&parent(world, id, kind)));
    out.extend(scene_export::record(&world.objects.world, id));
    let object = world.objects.components.objects[id].as_ref();
    out.push(object.map_or(0, |o| o.auxiliary_active as u16));
    for id in [
        object.and_then(|o| o.data.mesh),
        object.and_then(|o| o.auxiliary_mesh),
    ] {
        wide(&mut out, id.map_or(-1, |m| m.index() as i32));
    }
    motion(&mut out, object.and_then(|o| o.auxiliary_motion.as_ref()));
    out.extend(mesh_store_snapshot::snapshot(
        &world.objects.components.surfaces.meshes,
        status,
        tables,
        [name, "other.sm"],
    ));
    out
}
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let out = Path::new(&args[0]);
    let root = Path::new(&args[1]);
    let construction = reference_world::constructed(root);
    let world = construction.world();
    let (scene, _) = scene_export::encode(&world.objects.world);
    fs::write(
        out.join("parent-scene.bin"),
        scene
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut cells = Vec::new();
    for i in 0..64 {
        cells.extend(terrain_assets::cell_bytes(world.terrain(), i));
    }
    fs::write(out.join("parent-cells.bin"), cells).unwrap();
    let mut cases = Vec::new();
    let mut tables = vec![json!([])];
    for id in [69, 70, 141, 142, 172, 237, 238] {
        for variant in 0..10 {
            if id != 69 && id != 172 && variant != 0 {
                continue;
            }
            if id == 69 && (variant == 6 || variant == 7) {
                continue;
            }
            let (mut session, resources) =
                reference_world::constructed(root).into_session().unwrap();
            let world = session.world_mut_at_load().unwrap();
            let kind = if world.objects.components.falls[id].is_some() {
                1
            } else {
                2
            };
            let config = world.objects.components.objects[id]
                .as_ref()
                .unwrap()
                .data
                .auxiliary
                .as_ref()
                .unwrap()
                .clone();
            let model = resources.auxiliary_model(&config.shape_name).unwrap();
            world.objects.components.surfaces.meshes = Meshes::default();
            world.objects.components.objects[id]
                .as_mut()
                .unwrap()
                .data
                .mesh = None;
            world.identity.current_slot = 24;
            if variant == 9 {
                let entity = world.objects.world.colliders[id].entity;
                world.identity.entities.record_mut(entity).unwrap().class = None;
            }
            if kind == 2 && variant == 6 {
                world.objects.components.closers[id]
                    .as_mut()
                    .unwrap()
                    .state
                    .phase = Phase::Waiting;
            }
            if kind == 2 && variant == 7 {
                world.objects.components.closers[id]
                    .as_mut()
                    .unwrap()
                    .state
                    .phase = Phase::Closing;
            }
            let limit = if variant == 4 {
                0
            } else if variant == 5 {
                1
            } else {
                100000
            };
            let model_status = if variant == 1 {
                1
            } else if variant == 2 {
                -7
            } else {
                0
            };
            let mut controls = vec![
                [0, 33, 0, limit, model_status],
                [0, 33, 0, limit, model_status],
            ];
            if variant == 0 {
                controls.extend([
                    [1, 33, 0, limit, 0],
                    [1, 128, 0, limit, 0],
                    [2, 0, 0, limit, 0],
                    [1, 33, 2, limit, 0],
                ]);
            } else if variant == 3 || variant == 8 {
                controls = vec![
                    [0, 33, 0, limit, 0],
                    [if variant == 3 { 4 } else { 5 }, 0, 0, limit, 0],
                    [1, 33, 0, limit, 0],
                ];
            } else if kind == 2 && (variant == 6 || variant == 7) {
                controls = vec![[1, 33, 0, limit, 0]; 10];
            }
            if variant != 9 {
                controls.extend([[3, 33, 0, limit, 0], [3, 33, 0, limit, 0]]);
            }
            let env = world.registry.hit_environment(
                world.objects.components.fall_tuning,
                false,
                limit as u32,
            );
            let life = packed(&scene_lifecycle_assets::input(
                world,
                id,
                5,
                limit as u32,
                scene.len(),
            ));
            let packet = mesh_store_snapshot::payload(&model, &mut tables);
            let mut auxiliary = vec![0u16; 16];
            auxiliary[1] = 1;
            auxiliary[2] = 16;
            auxiliary[3] = 16;
            auxiliary[4] = (model.geometry.vertices.len() * 3
                + model.geometry.triangles.len()
                + model
                    .face_centers
                    .iter()
                    .flatten()
                    .map(Vec::len)
                    .max()
                    .unwrap_or(0)
                    * 3) as u16;
            auxiliary.extend(packet);
            auxiliary[5] = auxiliary.len() as u16;
            auxiliary.extend(scene_lifecycle_assets::row(world, id));
            auxiliary[6] = auxiliary.len() as u16;
            auxiliary.extend(&scene_export::record(&world.objects.world, id)[..60]);
            auxiliary[7] = auxiliary.len() as u16;
            auxiliary.push(0);
            for value in [-1, -1, world.shared.random as i32, 24] {
                wide(&mut auxiliary, value);
            }
            auxiliary.push(0);
            wide(&mut auxiliary, config.gravity);
            for gain in [
                config.bounce,
                config.friction,
                config.push,
                config.velocity,
                config.spin,
            ] {
                auxiliary.extend([gain.multiplier, u16::from(gain.shift)]);
            }
            wide(&mut auxiliary, 0x7182);
            auxiliary[12] = auxiliary.len() as u16;
            auxiliary.push(0);
            auxiliary[8] = auxiliary.len() as u16;
            for cell in &world.objects.world.grid.cells {
                auxiliary.push(cell.len() as u16);
            }
            for cell in &world.objects.world.grid.cells {
                for id in cell {
                    wide(&mut auxiliary, id.0 as i32);
                }
            }
            auxiliary[10] = auxiliary.len() as u16;
            auxiliary[9] = auxiliary.len() as u16;
            auxiliary[13] = model.geometry.triangles.len() as u16;
            auxiliary[14] = id as u16;
            let alias = kind == 2
                && world.objects.components.closers[id]
                    .as_ref()
                    .unwrap()
                    .reference_alias;
            let mut extra = vec![
                kind,
                u16::from(if kind == 1 {
                    world.objects.components.falls[id].is_some()
                } else {
                    world.objects.components.closers[id].is_some()
                }),
                u16::from(alias),
            ];
            for value in [300000, env.minimum_turn_rate, env.maximum_turn_rate] {
                wide(&mut extra, value);
            }
            extra.extend(packed(&parent(world, id, kind)));
            extra.extend(&scene_export::record(&world.objects.world, id)[60..]);
            let t = world.objects.components.fall_tuning;
            for value in [
                t.squash_angle,
                t.gravity,
                t.elasticity_multiplier,
                t.elasticity_shift as i32,
            ] {
                wide(&mut extra, value);
            }
            extra.extend(packed(
                &world
                    .vehicles
                    .primary()
                    .unwrap()
                    .physics
                    .motion
                    .poses
                    .reference
                    .0,
            ));
            extra.extend(packed(
                &world
                    .vehicles
                    .primary()
                    .unwrap()
                    .physics
                    .motion
                    .poses
                    .base_position,
            ));
            wide(
                &mut extra,
                world.vehicles.primary().unwrap().physics.entity.0 as i32,
            );
            let mut input = vec![
                life.len() as u16,
                auxiliary.len() as u16,
                extra.len() as u16,
                controls.len() as u16,
            ];
            input.extend(life);
            input.extend(auxiliary);
            input.extend(extra);
            for control in &controls {
                input.extend(packed(control));
            }
            let mut frames = Vec::new();
            for control in &controls {
                let (status, result) = invoke(world, id, kind, &model, *control);
                frames.push(snapshot(
                    world,
                    id,
                    kind,
                    status,
                    result,
                    &mut tables,
                    &config.shape_name,
                ));
            }
            cases.push(json!({"input":input,"frames":frames,"object":id,"kind":kind,"variant":variant,"controls":controls}));
        }
    }
    fs::write(out.join("parent-auxiliary.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original Fall/Closer method chains on authored ENGLAND objects; full identity, memberships, geometry, RNG, persistent auxiliary state, completion and resource destruction; other world actors stay fixed"})).unwrap()).unwrap();
    println!("{} parent auxiliary chains", cases.len());
}
