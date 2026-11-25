//! Actual auxiliary-contact method with local resource ownership and real terrain.
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
    actor_startup::Actor,
    mesh_break,
    object_query::{EntityId, ObstacleId},
    obstacle_collision as obstacle,
    scene_meshes::Meshes,
    scene_objects::{
        self,
        collision::{Collision, Services, Sound},
    },
    vehicle_runtime::SinglePlayer,
};
struct Devices {
    model: Arc<Model>,
    status: i32,
    tail: Option<u16>,
}
impl Services for Devices {
    fn paired_vertex_tail(&mut self) -> Option<u16> {
        self.tail
    }
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
        _: obstacle::Invocation<'_>,
    ) -> Result<(), i32> {
        panic!("unexpected audio")
    }
    fn feedback(
        &mut self,
        _: i32,
        _: &mut Collision<'_>,
        _: obstacle::Invocation<'_>,
    ) -> Result<(), i32> {
        panic!("unexpected feedback")
    }
}
fn wide(out: &mut Vec<u16>, value: i32) {
    out.extend([value as u16, (value >> 16) as u16]);
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
fn snapshot(
    world: &SinglePlayer,
    id: usize,
    status: i32,
    result: u32,
    tables: &mut Vec<Value>,
    name: &str,
) -> Vec<u16> {
    let mut out = Vec::new();
    for v in [
        status,
        result as i32,
        world.shared.random as i32,
        world.identity.current_slot as i32,
    ] {
        wide(&mut out, v);
    }
    let object = world.objects.components.objects[id].as_ref();
    wide(
        &mut out,
        object
            .and_then(|o| o.auxiliary_mesh)
            .map_or(-1, |m| m.index() as i32),
    );
    wide(
        &mut out,
        object
            .and_then(|o| o.data.mesh)
            .map_or(-1, |m| m.index() as i32),
    );
    out.push(object.map_or(0, |o| o.auxiliary_active as u16));
    out.extend(scene_lifecycle_assets::row(world, id));
    out.extend(&scene_export::record(&world.objects.world, id)[..60]);
    motion(&mut out, object.and_then(|o| o.auxiliary_motion.as_ref()));
    out.extend(mesh_store_snapshot::snapshot(
        &world.objects.components.surfaces.meshes,
        status,
        tables,
        [name, "other.sm"],
    ));
    for cell in &world.objects.world.grid.cells {
        out.push(cell.len() as u16);
    }
    for cell in &world.objects.world.grid.cells {
        for value in cell {
            wide(&mut out, value.0 as i32);
        }
    }
    out
}
fn invoke(
    world: &mut SinglePlayer,
    id: usize,
    model: &Arc<Model>,
    control: [i32; 9],
) -> (i32, u32) {
    let [limit, entity, vx, vy, vz, status, has_terrain, tail, clear] = control;
    if clear != 0 {
        if let Some(o) = world.objects.components.objects[id].as_mut() {
            o.auxiliary_active = 0;
        }
    }
    if has_terrain == 0 {
        scene_objects::rcc_lifecycle_probe::clear_terrain(&mut world.objects.components);
    }
    let tuning = world.objects.components.fall_tuning;
    let hit = world.registry.hit_environment(tuning, false, limit as u32);
    let (lifecycle, current_slot) = world.identity.split();
    let mut collision = Collision {
        components: &mut world.objects.components,
        lifecycle,
        environment: world.registry.environment(limit as u32),
        cars: None,
    };
    let mut vehicle = obstacle::State {
        entity: EntityId(entity as u32),
        ..Default::default()
    };
    vehicle.motion.velocity = [vx, vy, vz];
    let mut result = 0x7182;
    let code = collision
        .method(
            scene_objects::AUXILIARY_HIT,
            obstacle::Invocation {
                dt: 33,
                target: ObstacleId(id),
                vehicle: &mut vehicle,
                world: &mut world.objects.world,
                random: &mut world.shared.random,
                current_slot,
                result: &mut result,
            },
            hit,
            &mut Devices {
                model: Arc::clone(model),
                status,
                tail: (tail >= 0).then_some(tail as u16),
            },
            true,
        )
        .err()
        .unwrap_or(0);
    (code, result)
}
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let out = Path::new(&args[0]);
    let root = Path::new(&args[1]);
    let construction = reference_world::constructed(root);
    let world = construction.world();
    let mut ids = Vec::new();
    let mut names = Vec::new();
    for (id, o) in world.objects.components.objects.iter().enumerate() {
        if let Some(aux) = o.as_ref().and_then(|o| o.data.auxiliary.as_ref()) {
            let name = aux.shape_name.to_ascii_lowercase();
            if !names.contains(&name) {
                ids.push(id);
                names.push(name);
            }
        }
    }
    let mut cells = Vec::new();
    for i in 0..64 {
        cells.extend(terrain_assets::cell_bytes(world.terrain(), i));
    }
    fs::write(out.join("auxiliary-cells.bin"), cells).unwrap();
    let mut cases = Vec::new();
    let mut tables = vec![json!([])];
    for (order, &id) in ids.iter().enumerate() {
        let variants = if order == 0 { 16 } else { 1 };
        for variant in 0..variants {
            let limits: &[u32] = if variant == 0 {
                &[100000]
            } else {
                &[0, 1, 100000]
            };
            for &limit in limits {
                let (mut session, resources) =
                    reference_world::constructed(root).into_session().unwrap();
                let world = session.world_mut_at_load().unwrap();
                let config = world.objects.components.objects[id]
                    .as_ref()
                    .unwrap()
                    .data
                    .auxiliary
                    .as_ref()
                    .unwrap()
                    .clone();
                let models = [
                    resources.auxiliary_model(&config.shape_name).unwrap(),
                    resources
                        .auxiliary_model(&names[(order + 1) % names.len()])
                        .unwrap(),
                ];
                let packets = models
                    .each_ref()
                    .map(|m| mesh_store_snapshot::payload(m, &mut tables));
                let slot_words = models
                    .iter()
                    .map(|m| {
                        m.geometry.vertices.len() * 3
                            + m.geometry.triangles.len()
                            + m.face_centers
                                .iter()
                                .flatten()
                                .map(Vec::len)
                                .max()
                                .unwrap_or(0)
                                * 3
                    })
                    .max()
                    .unwrap();
                let capacity = models
                    .iter()
                    .map(|m| m.geometry.triangles.len())
                    .max()
                    .unwrap();
                world.objects.components.surfaces.meshes = Meshes::default();
                let object = world.objects.components.objects[id].as_mut().unwrap();
                object.data.mesh = None;
                object.auxiliary_mesh = None;
                object.auxiliary_motion = None;
                object.auxiliary_active = 0;
                let mut initial = Vec::<[u16; 4]>::new();
                if [6, 7, 8, 12, 13, 14].contains(&variant) {
                    let model = if variant == 14 { 1 } else { 0 };
                    let shared = world
                        .objects
                        .components
                        .surfaces
                        .meshes
                        .bind(&config.shape_name, &models[model]);
                    initial.push([0, model as u16, 0, 0]);
                    if variant != 14 {
                        let mut private = shared;
                        if variant != 8 {
                            private = world
                                .objects
                                .components
                                .surfaces
                                .meshes
                                .private_copy(shared)
                                .unwrap();
                            initial.push([1, 0, 0, 0]);
                        }
                        let object = world.objects.components.objects[id].as_mut().unwrap();
                        object.auxiliary_mesh = Some(private);
                        if variant != 8 {
                            let mut state =
                                mesh_break::State::new(models[0].geometry.triangles.len());
                            state.prepared = [6, 12, 13].contains(&variant);
                            object.auxiliary_motion = Some(state);
                        }
                        if variant == 7 {
                            world
                                .objects
                                .components
                                .surfaces
                                .meshes
                                .release_private(private)
                                .unwrap();
                            initial.push([2, 1, 0, 0]);
                        }
                    }
                }
                match variant {
                    4 | 13 => {
                        world.objects.components.objects[id]
                            .as_mut()
                            .unwrap()
                            .data
                            .auxiliary = None
                    }
                    5 => {
                        world.objects.components.objects[id]
                            .as_mut()
                            .unwrap()
                            .auxiliary_active = -1
                    }
                    9 => world.objects.components.objects[id] = None,
                    10 => world.objects.world.colliders[id].external_basis = None,
                    12 => {
                        world.objects.components.objects[id]
                            .as_mut()
                            .unwrap()
                            .auxiliary_mesh = None
                    }
                    15 => {
                        let cell = world.objects.components.objects[id]
                            .as_ref()
                            .unwrap()
                            .data
                            .cell
                            .unwrap();
                        let x = (cell.x - 1).max(0) as usize;
                        let z = cell.z.saturating_sub(1).min(7) as usize;
                        let list = &mut world.objects.world.grid.cells[z * 8 + x];
                        list.clear();
                        list.extend(std::iter::repeat_n(ObstacleId(65534), 80));
                        list.push(ObstacleId(id));
                    }
                    _ => (),
                }
                world.identity.current_slot = 25;
                let status = match variant {
                    1 | 5 => 1,
                    2 | 6 => -7,
                    _ => 0,
                };
                let has_terrain = i32::from(![3, 5].contains(&variant));
                let actual_limit = if variant == 15 && limit != 0 {
                    models[0].geometry.triangles.len() as u32 * 2
                } else {
                    limit
                };
                let entity = if variant == 11 { 0 } else { 99 };
                let mut controls = vec![
                    [
                        actual_limit as i32,
                        entity,
                        250000,
                        -150000,
                        500000,
                        status,
                        has_terrain,
                        -1,
                        0
                    ];
                    2
                ];
                if variant == 0 {
                    controls.push([
                        actual_limit as i32,
                        entity,
                        -250000,
                        50000,
                        -500000,
                        status,
                        has_terrain,
                        -1,
                        1,
                    ]);
                }
                let mut input = vec![
                    0,
                    controls.len() as u16,
                    0,
                    0,
                    slot_words as u16,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    initial.len() as u16,
                    0,
                    capacity as u16,
                    id as u16,
                    0,
                ];
                for (index, packet) in packets.iter().enumerate() {
                    input[2 + index] = input.len() as u16;
                    input.extend(packet);
                }
                input[5] = input.len() as u16;
                input.extend(scene_lifecycle_assets::row(world, id));
                input[6] = input.len() as u16;
                input.extend(&scene_export::record(&world.objects.world, id)[..60]);
                input[7] = input.len() as u16;
                let object = world.objects.components.objects[id].as_ref();
                input.push(object.map_or(0, |o| o.auxiliary_active as u16));
                for value in [
                    object
                        .and_then(|o| o.data.mesh)
                        .map_or(-1, |m| m.index() as i32),
                    object
                        .and_then(|o| o.auxiliary_mesh)
                        .map_or(-1, |m| m.index() as i32),
                    world.shared.random as i32,
                    25,
                ] {
                    wide(&mut input, value);
                }
                input.push(u16::from(matches!(
                    world.objects.components.actors.get(ObstacleId(id)),
                    Some(Actor::Powerup(_))
                )));
                wide(&mut input, config.gravity);
                for gain in [
                    config.bounce,
                    config.friction,
                    config.push,
                    config.velocity,
                    config.spin,
                ] {
                    input.extend([gain.multiplier, u16::from(gain.shift)]);
                }
                wide(&mut input, 0x7182);
                input[12] = input.len() as u16;
                motion(&mut input, object.and_then(|o| o.auxiliary_motion.as_ref()));
                input[8] = input.len() as u16;
                for cell in &world.objects.world.grid.cells {
                    input.push(cell.len() as u16);
                }
                for cell in &world.objects.world.grid.cells {
                    for value in cell {
                        wide(&mut input, value.0 as i32);
                    }
                }
                input[10] = input.len() as u16;
                input.extend(initial.iter().flatten());
                input[9] = input.len() as u16;
                for control in &controls {
                    for v in control {
                        wide(&mut input, *v);
                    }
                }
                let mut frames = Vec::new();
                for control in &controls {
                    let (code, result) = invoke(world, id, &models[0], *control);
                    frames.push(snapshot(
                        world,
                        id,
                        code,
                        result,
                        &mut tables,
                        &config.shape_name,
                    ));
                }
                if variant == 0 {
                    assert_eq!(
                        frames[0], frames[1],
                        "active guard changed persistent state"
                    );
                }
                if variant == 15 && actual_limit != 0 {
                    assert_eq!(
                        frames[0][0],
                        (-3i16) as u16,
                        "late membership limit was not exercised"
                    );
                    assert_eq!(
                        frames[0][12], 1,
                        "activation did not commit before membership failure"
                    );
                    assert_eq!(
                        frames[1][0], 0,
                        "active retry did not take the original guard"
                    );
                    assert_eq!(
                        &frames[0][4..6],
                        &frames[1][4..6],
                        "active retry consumed RNG"
                    );
                }
                cases.push(json!({"input":input,"frames":frames,"object":id,"model":order,"variant":variant,"limit":actual_limit}));
            }
        }
    }
    fs::write(out.join("auxiliary-hit.json"),serde_json::to_vec(&json!({"cases":cases,"names":names,"scope":"Actual AUXILIARY_HIT method on authored ENGLAND objects and real terrain, with a local persistent mesh arena; full geometry, state, resource errors, shared RNG and complete grid prefixes"})).unwrap()).unwrap();
    println!(
        "{} auxiliary-contact sequences over {} models",
        cases.len(),
        names.len()
    );
}
