//! Original class ticks against actual ENGLAND heights, normals and fallback.
use super::*;
use tankracer_game::{bounce, geometry, lifecycle, scene_loading, spatial, terrain};
#[allow(dead_code)]
#[path = "terrain_assets.rs"]
mod pages;
fn snapshot(
    world: &tankracer_game::vehicle_runtime::SinglePlayer,
    id: usize,
    code: i32,
    out: &mut Vec<i32>,
) {
    let scene = &world.objects;
    let entity = scene.world.colliders[id].entity;
    out.extend(bounce_hit_cases::row(
        &scene.components.bounces[id].unwrap().state,
        scene.world.objects[id].activity,
    ));
    out.extend(collider_row(&scene.world.colliders[id]));
    let cell = scene.components.objects[id].as_ref().unwrap().data.cell;
    out.extend([
        cell.map_or(0, |c| i32::from(c.x)),
        cell.map_or(0, |c| i32::from(c.z)),
        i32::from(cell.is_some()),
        code,
        world.identity.current_slot as i32,
        world.shared.random as i32,
        world.identity.ticks.flags[entity.0 as usize] as i32,
    ]);
    for ids in [
        world
            .identity
            .ticks
            .order
            .iter()
            .map(|id| id.0 as i32)
            .collect::<Vec<_>>(),
        scene
            .components
            .ordinary
            .iter()
            .map(|id| id.0 as i32)
            .collect(),
        scene
            .components
            .special
            .iter()
            .map(|id| id.0 as i32)
            .collect(),
        world
            .identity
            .queues
            .notifications
            .iter()
            .map(|id| id.0 as i32)
            .collect(),
        world
            .identity
            .queues
            .destruction
            .iter()
            .map(|id| id.0 as i32)
            .collect(),
    ] {
        assert!(ids.len() <= 4);
        out.push(ids.len() as i32);
        out.extend(&ids);
        out.extend(std::iter::repeat_n(-1, 4 - ids.len()));
    }
    for ids in &scene.world.grid.cells {
        assert!(ids.len() <= 3);
        out.push(ids.len() as i32);
        out.extend(ids.iter().map(|id| id.0 as i32));
        out.extend(std::iter::repeat_n(-1, 3 - ids.len()));
    }
}
pub(super) fn append(cases: &mut Vec<Value>, root: &std::path::Path, output: &std::path::Path) {
    let constructed = constructed_world(root);
    let (mut session, _) = constructed.into_session().unwrap();
    let world = session.world_mut_at_load().unwrap();
    let data = std::sync::Arc::clone(world.terrain());
    let mut bytes = Vec::new();
    for cell in 0..64 {
        bytes.extend(pages::cell_bytes(&data, cell));
    }
    fs::write(output.join("collision-cells.bin"), bytes).unwrap();
    let mut fallback = None;
    'found: for mesh in &data.meshes {
        for center in &mesh.face_centers {
            let p = std::array::from_fn(|i| {
                mesh.position[i]
                    .wrapping_add(i32::from(center[i]))
                    .wrapping_mul(8)
            });
            let mut normal = [0; 3];
            if geometry::fallback(&data, p, 0, &mut normal) && (normal[0] != 0 || normal[2] != 0) {
                fallback = Some((p, normal));
                break 'found;
            }
        }
    }
    let (fallback_position, fallback_normal) =
        fallback.expect("ENGLAND fallback with horizontal component");
    let mut seen = std::collections::BTreeSet::new();
    let mut profiles = Vec::new();
    for (id, c) in world.objects.components.bounces.iter().enumerate() {
        let Some(c) = c else { continue };
        if seen.insert(format!("{:?}", c.config)) {
            profiles.push((id, *c, world.objects.world.colliders[id]));
        }
    }
    for (id, component, original) in profiles {
        let entity = original.entity;
        let other = (id + 1) % world.objects.world.colliders.len();
        assert!(entity.0 < 512);
        let class = world.objects.world.objects[id].class;
        assert_eq!(
            world.objects.world.classes[class.0].methods[8],
            Some(tankracer_game::scene_objects::DEACTIVATE)
        );
        assert_eq!(
            world.objects.world.classes[world.objects.components.parents.generic.0].methods[8],
            Some(tankracer_game::scene_objects::QUEUE_DESTROY)
        );
        for variant in 0..14 {
            for limit in [0, 1, 10000] {
                let mut collider = original;
                collider.pose.matrix = Matrix::IDENTITY;
                collider.external_basis = Some(Matrix::IDENTITY);
                let mut height = terrain::MISS_HEIGHT;
                terrain::height(
                    &data,
                    collider.pose.position[0],
                    collider.pose.position[2],
                    &mut height,
                )
                .unwrap();
                assert_ne!(height, terrain::MISS_HEIGHT);
                collider.pose.position[1] = height.wrapping_add(5000);
                let mut state = bounce::State {
                    velocity: [10000, -9000, 10000],
                    spin: -123,
                    ground: height,
                    timer: u16::MAX,
                    age: 1234,
                    airborne: 1,
                    ..Default::default()
                };
                let mut config = component.config;
                let mut alias = true;
                let mut dt = [16, 33, 1000];
                if variant == 2 {
                    collider.pose.position[1] = height - 5;
                    state.velocity = [10, -100, 10];
                    config.height_period = 1;
                }
                if variant == 3 {
                    state.airborne = 0;
                    state.velocity = [-100000, -99, -100000];
                    dt = [0, 16, 33];
                }
                if variant == 4 {
                    state.airborne = 0;
                    state.age = 19990;
                    alias = false;
                    collider.external_basis = Some(Matrix::from_angles([4000, 7000, 3000]));
                }
                if variant == 5 {
                    state.airborne = 0;
                    state.velocity = [0; 3];
                    collider.external_basis = None;
                }
                if variant == 6 {
                    config.height_period = 0;
                }
                if variant == 7 || variant == 8 {
                    state.age = 20001;
                }
                if variant == 9 || variant == 11 {
                    collider.pose.position[0] = i32::MAX / 4;
                    collider.pose.position[2] = i32::MAX / 4;
                    state.ground = terrain::MISS_HEIGHT;
                    config.height_period = 1;
                    if variant == 11 {
                        state.airborne = 0;
                        state.velocity = [0; 3];
                    }
                }
                if variant == 10 {
                    state.raised = 1;
                    state.airborne = 0;
                    state.velocity = [0; 3];
                    collider.external_basis = None;
                }
                if variant == 12 || variant == 13 {
                    collider.pose.position = fallback_position;
                    state.ground = terrain::MISS_HEIGHT;
                    state.timer = 0;
                    state.spin = 0;
                    state.velocity = [
                        fallback_normal[0].wrapping_mul(-100),
                        0,
                        fallback_normal[2].wrapping_mul(-100),
                    ];
                    config.height_period = 4;
                    dt = [0, 16, 33];
                    if variant == 13 {
                        collider.external_basis = None;
                    }
                }
                let activity = i32::from(variant != 0);
                world.objects.world.objects[id].activity = activity;
                world.objects.world.colliders[id] = collider;
                world.objects.components.bounces[id] = Some(bounce::Component {
                    config,
                    state,
                    reference_alias: alias,
                });
                let grid = &mut world.objects.world.grid;
                grid.cells =
                    std::array::from_fn(|_| vec![query::ObstacleId(other), query::ObstacleId(id)]);
                let inserted = spatial::insert(
                    grid,
                    query::ObstacleId(id),
                    &collider,
                    Some(collider.radius),
                );
                let cell = (variant != 8).then_some(inserted);
                let object = world.objects.components.objects[id].as_mut().unwrap();
                object.data.cell = cell;
                object.data.impact_threshold = 50;
                let has_mesh = object.data.mesh.is_some();
                assert!(has_mesh);
                world.objects.components.ordinary =
                    vec![query::ObstacleId(other), query::ObstacleId(id)];
                world.objects.components.special =
                    vec![query::ObstacleId(id), query::ObstacleId(other)];
                world.identity.ticks = lifecycle::Ticks {
                    flags: vec![0; 512],
                    order: vec![entity],
                    next: None,
                };
                world.identity.ticks.flags[entity.0 as usize] = 16;
                world.identity.queues = lifecycle::Queues::default();
                world.identity.current_slot = 24;
                world.shared.random = 7;
                let mut input = bounce_hit_cases::row(&state, activity);
                input.extend(collider_row(&collider));
                input.extend(bounce_hit_cases::config(config));
                input.extend([
                    cell.map_or(0, |c| i32::from(c.x)),
                    cell.map_or(0, |c| i32::from(c.z)),
                    i32::from(cell.is_some()),
                    50,
                    i32::from(alias),
                    i32::from(has_mesh),
                    entity.0 as i32,
                    id as i32,
                    other as i32,
                    limit as i32,
                ]);
                input.extend(world.objects.world.grid.origin);
                input.extend(world.objects.world.grid.shifts.map(|x| x as i32));
                input.extend(dt);
                input.push(7);
                assert_eq!(input.len(), 70);
                let mut expected = Vec::new();
                for delta in dt {
                    world.objects.components.dt = delta;
                    let result = world.identity.context().dispatch(
                        entity,
                        scene_loading::TICK,
                        None,
                        world.registry.environment(limit),
                        &mut world.objects.view(),
                    );
                    let code = match result {
                        Ok(0) => 0,
                        Err(tankracer_game::entity_factory::Error::Dispatch(
                            tankracer_game::dispatch::Error::Method(code),
                        )) => code,
                        other => panic!("Bounce tick {other:?}"),
                    };
                    snapshot(world, id, code, &mut expected);
                }
                assert_eq!(expected.len(), 990);
                if limit == 10000 {
                    if variant == 2 {
                        assert_eq!(expected[330 + 11], 0);
                        assert_eq!(expected[330 + 64], 1);
                    }
                    if variant == 3 {
                        assert_eq!(expected[11], 0);
                        assert_eq!(expected[64], 1);
                    }
                    if variant == 7 {
                        assert_eq!(expected[69], 1);
                    }
                    if variant == 12 {
                        let friction = i32::from(config.friction);
                        let vx = state.velocity[0]
                            .wrapping_sub(friction.wrapping_mul(state.velocity[0]) >> 7);
                        let vz = state.velocity[2]
                            .wrapping_sub(friction.wrapping_mul(state.velocity[2]) >> 7);
                        assert!(
                            expected[0] != vx || expected[2] != vz,
                            "fallback reflected the incoming velocity"
                        );
                    }
                    if variant == 13 {
                        assert_eq!(expected[45], -4);
                    }
                }
                cases.push(json!({"kind":27,"input":input,"status":0,"expected":expected}));
            }
        }
    }
}
