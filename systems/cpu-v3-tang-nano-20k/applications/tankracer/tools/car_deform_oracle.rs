//! Actual private deformable entry: real meshes, gates and committed failures.
use serde_json::{json, Value};
use std::{fs, path::Path};
use tankracer_game::{
    deformable_hit, effect_pool, impact_particles, math::Matrix, object_query::TankId,
    vehicle_runtime::rcc_probe,
};
mod impact_snapshot;
#[allow(dead_code)]
mod reference_world;
mod scene_export;
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let out = Path::new(&args[0]);
    let mut session = reference_world::complete(Path::new(&args[1]));
    let world = session.world_mut_at_load().unwrap();
    let template = world.vehicles.entries[0].as_ref().unwrap().clone();
    let (scene, _) = scene_export::encode(&world.objects.world);
    fs::write(
        out.join("car-deform-scene.bin"),
        scene
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut cases = Vec::<Value>::new();
    for object in [124, 173] {
        let collider = world.objects.world.colliders[object];
        let properties = world.objects.world.objects[object];
        let component = world.objects.components.surfaces.entries[object]
            .as_ref()
            .unwrap();
        let mesh = component.mesh;
        let config = component.config;
        let vertices = world
            .objects
            .components
            .surfaces
            .meshes
            .get(mesh)
            .unwrap()
            .vertices
            .clone();
        let flags = component.vertex_flags.clone();
        let point = collider
            .pose
            .matrix
            .local_to_world(vertices[0].map(|v| i32::from(v) * 8));
        let origin =
            std::array::from_fn::<_, 3, _>(|i| point[i].wrapping_add(collider.pose.position[i]));
        for variant in 0..12 {
            let c = world.objects.components.surfaces.entries[object]
                .as_mut()
                .unwrap();
            c.state = deformable_hit::State::default();
            c.config = config;
            c.config.has_sound = variant == 11;
            c.vertex_flags = if variant == 9 {
                Vec::new()
            } else {
                flags.clone()
            };
            world
                .objects
                .components
                .surfaces
                .meshes
                .get_mut(mesh)
                .unwrap()
                .vertices = vertices.clone();
            let mut p = template.clone();
            let v = &mut p.physics;
            v.player = variant != 1;
            v.impact_cooldown = if variant == 5 {
                0
            } else if variant == 6 {
                u32::MAX
            } else {
                500
            };
            let mut position = origin;
            if variant == 10 {
                position[0] += 100000;
            }
            v.motion.poses.physical_position = position;
            v.motion.poses.base_position = position;
            v.motion.previous_position = position;
            v.motion.poses.body.matrix = Matrix::IDENTITY;
            v.motion.poses.reference = Matrix::IDENTITY;
            v.motion.velocity = [0, 0, if variant == 4 { 200000 } else { 300001 }];
            v.powertrain.local_velocity = [0, 0, if object == 124 { 300001 } else { -300001 }];
            v.collision.material = 77;
            v.collision.deformable = Some(collider.entity);
            v.collision.impact_accumulator = 123;
            let contacts = [[-16, 0, 16], [-16, 0, -16], [16, 0, 16], [16, 0, -16]];
            for i in 0..4 {
                let point = std::array::from_fn(|j| position[j] + contacts[i][j]);
                v.motion.contacts[i].world = point;
                v.contact.sampling.primary.points[i].world = point;
                v.contact.response.points[i].corner = if variant == 8 { 9 } else { i as u32 };
            }
            world.shared.impacts = impact_particles::State::default();
            if variant == 7 {
                world.shared.impacts.next = 4;
            }
            world.shared.pool = effect_pool::Pool::default();
            let seed = (0u32..64)
                .find(|s| s.wrapping_mul(1664525).wrapping_add(1013904223) & 48 == 0)
                .unwrap();
            world.shared.random = seed;
            world.identity.current_slot = 5;
            world.frame = 17;
            let mode = if variant == 2 {
                2
            } else if variant == 3 {
                3
            } else {
                0
            };
            let mut input = vec![
                33,
                mode,
                i32::from(v.player),
                v.powertrain.local_velocity[2],
                v.impact_cooldown as i32,
                seed as i32,
                world.shared.impacts.next as i32,
                collider.entity.0 as i32,
                77,
                123,
                5,
                variant,
                vertices.len() as i32,
                if variant == 9 { 0 } else { flags.len() as i32 },
            ];
            input.push(collider.entity.0 as i32);
            input.extend(collider.pose.position);
            input.extend(collider.pose.matrix.0);
            input.extend(collider.external_basis.unwrap_or(Matrix::IDENTITY).0);
            input.extend(collider.bounds.min);
            input.extend(collider.bounds.max);
            input.extend([
                collider.radius as i32,
                i32::from(collider.external_basis.is_some()),
                properties.weight,
                config.floor,
                config.compression,
                config.drift_shift as i32,
                config.material as i32,
                i32::from(variant == 11),
                0,
                -1,
                0,
                v.entity.0 as i32,
            ]);
            input.extend(position);
            input.extend(position);
            input.extend(position);
            input.extend(v.motion.velocity);
            input.extend(v.motion.contacts.map(|c| c.world).into_iter().flatten());
            input.extend(Matrix::IDENTITY.0);
            input.push(1);
            input.extend(
                v.contact
                    .response
                    .points
                    .each_ref()
                    .map(|p| p.corner as i32),
            );
            assert_eq!(input.len(), 92);
            input.extend(vertices.iter().flatten().map(|v| i32::from(*v)));
            input.extend(flags.iter().map(|v| i32::from(*v)));
            world.vehicles.entries[0] = Some(p);
            let status = rcc_probe::deformables(world, TankId(0), 33, mode)
                .err()
                .unwrap_or(0);
            if variant == 7 || variant == 8 {
                assert_eq!(status, -4);
            }
            if variant == 9 {
                assert_eq!(status, -1);
            }
            let v = &world.vehicles.entries[0].as_ref().unwrap().physics;
            let c = world.objects.components.surfaces.entries[object]
                .as_ref()
                .unwrap();
            let mut expected = impact_snapshot::snapshot(
                [status, -99, -99, -99],
                &world.shared.impacts,
                v,
                &world.shared.pool,
                world.shared.random,
            );
            expected.extend([
                v.collision.material,
                v.collision.deformable.map_or(-1, |e| e.0 as i32),
                v.collision.impact_accumulator,
                world.identity.current_slot as i32,
            ]);
            expected.extend(v.motion.velocity);
            expected.extend([
                i32::from(c.state.dirty),
                c.state.last_vehicle.map_or(-1, |e| e.0 as i32),
                c.state.sound_timer,
            ]);
            expected.extend(
                world
                    .objects
                    .components
                    .surfaces
                    .meshes
                    .get(mesh)
                    .unwrap()
                    .vertices
                    .iter()
                    .flatten()
                    .map(|v| i32::from(*v)),
            );
            // Inside uses original mesh bounds, independently of collision bounds.
            input.extend(properties.mesh_bounds.into_iter().flatten().map(i32::from));
            input.extend([
                properties.class.0 as i32,
                (scene.len() * 2) as i32,
                0,
                (scene.len() * 2).div_ceil(128) as i32,
            ]);
            cases.push(json!({"kind":30,"input":input,"status":0,"expected":expected,"variant":variant,"object":object}));
        }
    }
    fs::write(
        out.join("car-deforms.json"),
        serde_json::to_vec(&cases).unwrap(),
    )
    .unwrap();
}
