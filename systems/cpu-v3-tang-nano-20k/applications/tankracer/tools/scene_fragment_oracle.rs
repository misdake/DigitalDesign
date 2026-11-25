//! Original auxiliary-update class calls on actual ENGLAND resources and IDs.
#[allow(dead_code)]
mod fragment_snapshot;
mod identity_assets;
#[allow(dead_code)]
mod reference_world;
mod scene_export;
#[allow(dead_code)]
mod scene_lifecycle_assets;
use fragment_snapshot::Fixture;
use serde_json::{json, Value};
use std::{fs, path::Path};
use tankracer_assets::mesh::Geometry;
use tankracer_game::{
    dispatch, entity_factory, mesh_break,
    object_query::ObstacleId,
    obstacle_startup::Auxiliary,
    scene_loading,
    scene_meshes::{rcc_probe, MeshId},
    scene_objects::{Execution, View},
    session::Session,
    vehicle_runtime::SinglePlayer,
};
fn raw_words(words: &[u16]) -> Vec<i32> {
    words
        .chunks_exact(2)
        .map(|p| i32::from(p[0]) | (i32::from(p[1]) << 16))
        .collect()
}
fn fixture(
    world: &SinglePlayer,
    id: usize,
    mesh: MeshId,
    config: &Auxiliary,
) -> (Fixture, u64, bool) {
    let (geometry, centers, radius, version) =
        rcc_probe::inspect(&world.objects.components.surfaces.meshes, mesh);
    let present = geometry.is_some();
    let state = world.objects.components.objects[id]
        .as_ref()
        .and_then(|o| o.auxiliary_motion.clone())
        .unwrap_or_else(|| mesh_break::State::new(0));
    (
        Fixture {
            state,
            geometry: geometry.unwrap_or(Geometry {
                vertices: vec![],
                triangles: vec![],
            }),
            centers,
            radius,
            config: config.clone(),
            seed: world.shared.random,
            velocity: None,
            basis: None,
            height: 0,
            position: world.objects.world.colliders[id].pose.position,
            tail: None,
        },
        version,
        present,
    )
}
fn object_words(world: &SinglePlayer, id: usize) -> [i32; 2] {
    world.objects.components.objects[id]
        .as_ref()
        .map_or([0, -1], |o| {
            [
                i32::from(o.auxiliary_active),
                o.data.mesh.map_or(-1, |m| m.index() as i32),
            ]
        })
}
fn prepare(root: &Path, id: usize, variant: u16) -> (Session, MeshId, Auxiliary) {
    let (mut session, resources) = reference_world::constructed(root).into_session().unwrap();
    let world = session.world_mut_at_load().unwrap();
    let entity = world.objects.world.colliders[id].entity;
    let config = world.objects.components.objects[id]
        .as_ref()
        .unwrap()
        .data
        .auxiliary
        .as_ref()
        .unwrap()
        .clone();
    let model = resources.auxiliary_model(&config.shape_name).unwrap();
    let meshes = &mut world.objects.components.surfaces.meshes;
    let shared = meshes.bind(&config.shape_name, &model);
    let private = meshes.private_copy(shared).unwrap();
    let mut state = mesh_break::State::new(model.geometry.triangles.len());
    let position = world.objects.world.colliders[id].pose.position;
    let mut height = tankracer_game::terrain::MISS_HEIGHT;
    tankracer_game::terrain::height(world.terrain(), position[0], position[2], &mut height)
        .unwrap();
    mesh_break::activate(
        &mut state,
        world
            .objects
            .components
            .surfaces
            .meshes
            .fragments(private)
            .unwrap(),
        &config,
        mesh_break::Hit {
            velocity: Some([250000, -150000, 500000]),
            basis: world.objects.world.colliders[id].external_basis,
            height,
            paired_tail: None,
        },
        &mut world.shared.random,
        100000,
    )
    .unwrap();
    if variant == 1 || variant == 14 {
        for f in &mut state.faces {
            f.life = 1;
        }
    }
    let object = world.objects.components.objects[id].as_mut().unwrap();
    object.auxiliary_mesh = Some(private);
    object.auxiliary_motion = Some(state);
    object.auxiliary_active = 1;
    object.data.mesh = Some(private);
    world.objects.world.colliders[id].radius = 5120;
    tankracer_game::spatial::remove(
        &mut world.objects.world.grid,
        ObstacleId(id),
        object.data.cell,
        100000,
    )
    .unwrap();
    world.identity.current_slot = 17;
    match variant {
        2 => {
            world.objects.components.objects[id]
                .as_mut()
                .unwrap()
                .auxiliary_mesh = None
        }
        3 => {
            world.objects.components.objects[id]
                .as_mut()
                .unwrap()
                .auxiliary_motion = None
        }
        4 => {
            world.objects.components.objects[id]
                .as_mut()
                .unwrap()
                .data
                .auxiliary = None
        }
        5 => {
            world
                .objects
                .components
                .surfaces
                .meshes
                .release_private(private)
                .unwrap();
        }
        6 => world.objects.components.objects[id] = None,
        7 => world.identity.entities.record_mut(entity).unwrap().class = None,
        8 => world.identity.entities.record_mut(entity).unwrap().object = None,
        9 => {
            world.identity.entities.record_mut(entity).unwrap().class = Some(world.registry.generic)
        }
        10 => {
            world
                .objects
                .components
                .surfaces
                .meshes
                .fragments(private)
                .unwrap()
                .centers[1] = None
        }
        11 => rcc_probe::version(
            &mut world.objects.components.surfaces.meshes,
            private,
            u64::MAX,
        ),
        12 => {
            world.objects.components.objects[id]
                .as_mut()
                .unwrap()
                .auxiliary_active = -1
        }
        13 => rcc_probe::version(
            &mut world.objects.components.surfaces.meshes,
            private,
            u32::MAX as u64,
        ),
        _ => (),
    }
    (session, private, config)
}
fn tick(world: &mut SinglePlayer, id: usize, dt: i32, mode: i32, limit: u32) -> (i32, u32) {
    world.objects.components.dt = dt;
    world.objects.components.mode = mode;
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
    match world.identity.context().dispatch(
        entity,
        scene_loading::AUXILIARY_UPDATE,
        None,
        env,
        &mut view,
    ) {
        Ok(result) => (0, result),
        Err(entity_factory::Error::Dispatch(dispatch::Error::Method(code))) => (code, 0),
        Err(_) => (-1, 0),
    }
}
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let out = Path::new(&args[0]);
    let root = Path::new(&args[1]);
    let construction = reference_world::constructed(root);
    let world = construction.world();
    let (scene, _) = scene_export::encode(&world.objects.world);
    fs::write(
        out.join("scene-fragments.bin"),
        scene
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let ids = world
        .objects
        .components
        .objects
        .iter()
        .enumerate()
        .filter_map(|(id, o)| {
            let o = o.as_ref()?;
            o.data.auxiliary.as_ref()?;
            let class = &world.registry.classes[world.objects.world.objects[id].class.0];
            (class.methods[22] == Some(tankracer_game::scene_objects::AUXILIARY_TICK)
                && class.methods[8] == Some(tankracer_game::scene_objects::DEACTIVATE)
                && class.methods[27] == Some(tankracer_game::scene_objects::DESTROY_REQUEST))
            .then_some(id)
        })
        .collect::<Vec<_>>();
    assert!(!ids.is_empty());
    let mut cases = Vec::<Value>::new();
    for (order, id) in ids.iter().copied().enumerate() {
        let variants = if order == 0 { 15 } else { 2 };
        for variant in 0..variants {
            let limits: &[u32] = if order == 0 {
                &[0, 1, 2, 100000]
            } else {
                &[100000]
            };
            for &limit in limits {
                let (mut session, mesh, config) = prepare(root, id, variant);
                let world = session.world_mut_at_load().unwrap();
                let (initial, version, present) = fixture(world, id, mesh, &config);
                let object = object_words(world, id);
                let mut input = scene_lifecycle_assets::input(world, id, 5, limit, scene.len());
                input.extend([
                    0,
                    0,
                    object[0],
                    object[1],
                    version as u32 as i32,
                    (version >> 32) as u32 as i32,
                    i32::from(present),
                ]);
                input.extend(raw_words(
                    &scene_export::record(&world.objects.world, id)[..60],
                ));
                let payload = initial.input(1, limit, 33);
                input.push(payload.len() as i32);
                input.extend(payload);
                let mode = if variant == 14 { 2 } else { 0 };
                let steps = if order == 0 && variant == 0 && limit == 100000 {
                    vec![[128, mode]; 24]
                } else {
                    vec![[33, mode], [33, mode]]
                };
                input.push(steps.len() as i32);
                input.extend(steps.iter().flatten());
                let mut expected = Vec::new();
                for [dt, mode] in &steps {
                    let (status, result) = tick(world, id, *dt, *mode, limit);
                    let mut frame = vec![status];
                    frame.extend(scene_lifecycle_assets::state(world, id));
                    let (fragment, revision, _) = fixture(world, id, mesh, &config);
                    let object = object_words(world, id);
                    frame.extend([
                        result as i32,
                        object[0],
                        object[1],
                        revision as u32 as i32,
                        (revision >> 32) as u32 as i32,
                    ]);
                    frame.extend(raw_words(
                        &scene_export::record(&world.objects.world, id)[..60],
                    ));
                    frame.extend(fragment.output(status));
                    assert!(frame.len() < 2944, "scene fragment output too large");
                    expected.push(frame);
                }
                assert!(input.len() < 3072, "scene fragment input too large");
                cases.push(json!({"input":input,"frames":expected,"object":id,"variant":variant,"limit":limit,"steps":steps}));
            }
        }
    }
    fs::write(out.join("scene-fragments.json"),serde_json::to_vec(&json!({"cases":cases,"objects":ids,
        "scope":"Original auxiliary-update class calls on live ENGLAND IDs, private mesh versions, geometry, completion dispatch, shared lists/grid and committed failures; persistent per-case sequences"})).unwrap()).unwrap();
    println!(
        "{} scene-fragment sequences over {} actual owners",
        cases.len(),
        ids.len()
    );
}
