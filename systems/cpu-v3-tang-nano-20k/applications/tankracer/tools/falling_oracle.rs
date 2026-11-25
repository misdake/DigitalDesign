//! Real class dispatch on constructed ENGLAND, including immediate retirement.
use super::*;
use tankracer_game::{
    fall, lifecycle, scene_loading,
    session::loading::{Files, Selection},
    spatial, terrain,
    world_startup::Constructed,
};
fn snapshot(
    world: &tankracer_game::vehicle_runtime::SinglePlayer,
    id: usize,
    status: i32,
    out: &mut Vec<i32>,
) {
    let scene = &world.objects;
    let c = scene.world.colliders[id];
    let p = scene.world.objects[id];
    let s = scene.components.falls[id].as_ref().unwrap();
    out.extend([
        s.angular_velocity,
        s.rotation,
        s.turn_rate,
        s.slope_limit,
        p.flags as i32,
        p.activity,
    ]);
    out.extend(c.pose.matrix.0);
    out.extend(c.external_basis.unwrap_or(Matrix::IDENTITY).0);
    out.extend([
        i32::from(c.external_basis.is_some()),
        status,
        world.identity.current_slot as i32,
        world.identity.ticks.flags[c.entity.0 as usize] as i32,
        world.identity.queues.notifications.len() as i32,
        world
            .identity
            .queues
            .notifications
            .front()
            .map_or(-1, |id| id.0 as i32),
    ]);
    for row in &scene.world.grid.cells {
        assert!(row.len() <= 3);
        out.push(row.len() as i32);
        out.extend(row.iter().map(|id| id.0 as i32));
        out.extend(std::iter::repeat_n(-1, 3 - row.len()));
    }
}
pub(super) fn append(cases: &mut Vec<Value>, root: &std::path::Path) {
    let names = [
        "DATA",
        "TANKS/BRONZE",
        "TANKS/SILVER",
        "TANKS/GOLD",
        "INGAME",
        "LEVELS/ENGLAND",
    ];
    let archives =
        names.map(|n| fs::read(root.join("assets/game").join(n).join("FASTFILE.DAT")).unwrap());
    let vehicles = [
        archives[1].as_slice(),
        archives[2].as_slice(),
        archives[3].as_slice(),
    ];
    let constructed = Constructed::load(
        Files {
            data: &archives[0],
            vehicles: &vehicles,
            shared: &archives[4],
            level: &archives[5],
        },
        Selection::default_cup(0, 1).unwrap(),
    )
    .unwrap();
    let (mut session, _) = constructed.into_session().unwrap();
    let world = session.world_mut_at_load().unwrap();
    let terrain = std::sync::Arc::clone(world.terrain());
    let id = world
        .objects
        .components
        .falls
        .iter()
        .position(Option::is_some)
        .unwrap();
    let peer = (id + 1) % world.objects.world.colliders.len();
    let entity = world.objects.world.colliders[id].entity;
    let peer_entity = world.objects.world.colliders[peer].entity;
    let old_collider = world.objects.world.colliders[id];
    let old_properties = world.objects.world.objects[id];
    let original = world.objects.components.fall_tuning;
    let object = world.objects.components.objects[id].as_mut().unwrap();
    object.data.auxiliary = None;
    object.data.debris = None;
    object.data.cell = Some(spatial::Cell { x: 0, z: 0 });
    object.auxiliary_active = 0;
    assert!(entity.0 < 512);
    for variant in 0..14 {
        for limit in [0, 1, 10000] {
            let mut initial = old_collider;
            initial.pose.matrix = Matrix::IDENTITY;
            initial.external_basis = if variant == 0 || variant == 1 {
                None
            } else {
                Some(Matrix::IDENTITY)
            };
            if variant == 11 {
                initial
                    .pose
                    .matrix
                    .rotate(tankracer_game::math::Axis::Z, 4096);
            }
            if variant == 13 {
                let width = (i32::from(old_properties.mesh_bounds[1][1])
                    - i32::from(old_properties.mesh_bounds[0][1]))
                .wrapping_mul(8);
                let mut height = terrain::MISS_HEIGHT;
                terrain::height(
                    &terrain,
                    initial.pose.position[0],
                    initial.pose.position[2].wrapping_add(width),
                    &mut height,
                )
                .unwrap();
                assert_ne!(height, terrain::MISS_HEIGHT);
                initial.pose.position[1] = height.wrapping_add(i32::MIN);
            }
            world.objects.world.colliders[id] = initial;
            world.objects.world.objects[id] = old_properties;
            let p = &mut world.objects.world.objects[id];
            p.activity = i32::from(variant != 0);
            p.flags = 0x12340000
                | match variant {
                    2..=5 | 12 | 13 => 2,
                    7 => 8,
                    9 => 4,
                    _ => 0,
                };
            let s = fall::State {
                angular_velocity: match variant {
                    7 => 24000,
                    8 | 9 => -100,
                    _ => -24000,
                },
                rotation: if variant == 8 || variant == 9 {
                    -16380
                } else {
                    -12000
                },
                turn_rate: 1536,
                last_vehicle: if variant == 12 {
                    None
                } else {
                    Some(peer_entity)
                },
                ..Default::default()
            };
            world.objects.components.falls[id] = Some(s.clone());
            let yaw = match variant {
                3 => 32768,
                4 => 8192,
                5 => 57344,
                _ => 0,
            };
            let basis = Matrix::from_angles([0, yaw, 0]);
            world.objects.world.colliders[peer].pose.matrix = basis;
            let mut tuning = original;
            if variant == 8 || variant == 9 {
                tuning.elasticity_multiplier = 1;
                tuning.elasticity_shift = 3;
                tuning.gravity = 1;
            }
            if variant == 10 {
                tuning.gravity = i32::MAX;
                tuning.elasticity_multiplier = i32::MIN;
                tuning.elasticity_shift = 31;
            }
            world.objects.components.fall_tuning = tuning;
            world.objects.world.grid.origin = [0; 2];
            world.objects.world.grid.shifts = [12; 2];
            world.objects.world.grid.cells = std::array::from_fn(|_| Vec::new());
            for (cell, ids) in [
                (0, vec![query::ObstacleId(id), query::ObstacleId(id)]),
                (1, vec![query::ObstacleId(peer), query::ObstacleId(id)]),
                (63, vec![query::ObstacleId(id)]),
            ] {
                world.objects.world.grid.cells[cell] = ids;
            }
            world.identity.ticks = lifecycle::Ticks {
                flags: vec![0; 512],
                order: vec![entity],
                next: None,
            };
            world.identity.ticks.flags[entity.0 as usize] = 16;
            world.identity.queues = lifecycle::Queues::default();
            world.identity.current_slot = 777;
            let width = (i32::from(old_properties.mesh_bounds[1][1])
                - i32::from(old_properties.mesh_bounds[0][1]))
            .wrapping_mul(8);
            let mut input = vec![
                s.angular_velocity,
                s.rotation,
                s.turn_rate,
                s.slope_limit,
                world.objects.world.objects[id].flags as i32,
                world.objects.world.objects[id].activity,
            ];
            input.extend(initial.pose.matrix.0);
            input.extend(initial.external_basis.unwrap_or(Matrix::IDENTITY).0);
            input.push(i32::from(initial.external_basis.is_some()));
            input.extend(basis.0);
            input.extend(initial.pose.position);
            input.extend([
                width,
                tuning.squash_angle,
                tuning.gravity,
                tuning.elasticity_multiplier,
                tuning.elasticity_shift as i32,
                limit,
                id as i32,
                entity.0 as i32,
                i32::from(variant != 12),
            ]);
            assert_eq!(input.len(), 46);
            input.extend([16, 33, 1000, -16]);
            let mut expected = Vec::new();
            for dt in [16, 33, 1000, -16] {
                world.objects.components.dt = dt;
                let old_flags = world.objects.world.objects[id].flags;
                let result = world.identity.context().dispatch(
                    entity,
                    scene_loading::TICK,
                    None,
                    world.registry.environment(limit as u32),
                    &mut world.objects.view(),
                );
                let status = match result {
                    Ok(0) => 0,
                    Err(tankracer_game::entity_factory::Error::Dispatch(
                        tankracer_game::dispatch::Error::Method(code),
                    )) => code,
                    other => panic!("fall dispatch {other:?}"),
                };
                // Independent terrain answer for the caller's synchronous query.
                // The final class-dispatch state remains the only expected result.
                let mut height = terrain::MISS_HEIGHT;
                let mut height_status = 0;
                let mut query_position = [0; 2];
                if old_flags & 2 != 0 && world.objects.world.objects[id].flags & 2 == 0 {
                    let c = world.objects.world.colliders[id];
                    let m = c.external_basis.unwrap_or(c.pose.matrix);
                    let mut x = m.0[6].wrapping_mul(width) / 4096;
                    let mut z = m.0[8].wrapping_mul(width) / 4096;
                    let flags = world.objects.world.objects[id].flags;
                    if (flags & 8 != 0) != (flags & 4 != 0) {
                        x = x.wrapping_neg();
                        z = z.wrapping_neg();
                    }
                    query_position = [
                        c.pose.position[0].wrapping_add(x),
                        c.pose.position[2].wrapping_add(z),
                    ];
                    if let Err(e) = terrain::height(
                        &terrain,
                        c.pose.position[0].wrapping_add(x),
                        c.pose.position[2].wrapping_add(z),
                        &mut height,
                    ) {
                        height_status = if e == terrain::Error::Arithmetic {
                            -2
                        } else {
                            -1
                        };
                    }
                }
                input.extend([height, height_status]);
                snapshot(world, id, status, &mut expected);
                expected.extend(query_position);
            }
            assert_eq!(expected.len(), 1152);
            cases.push(json!({"kind":21,"input":input,"status":0,"expected":expected}));
        }
    }
}
