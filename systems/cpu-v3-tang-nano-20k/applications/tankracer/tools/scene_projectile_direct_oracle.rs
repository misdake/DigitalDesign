//! Complete class-table direct hits with live damage geometry and temporary effects.
use super::{ints, scene_export};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_assets::config::Part;
use tankracer_game::{
    explosion::blasts,
    math::Matrix,
    object_query::TankId,
    pose::{ExternalBasis, Pose},
    projectile::{self, launch::Owner},
    scene_entity::Handle,
    scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
    vehicle_startup::Player,
};

fn snapshot(world: &SinglePlayer, original: &[Player]) -> Value {
    let mut value = super::scene_projectile_area_oracle::snapshot(world);
    let mut profiles = Vec::new();
    let mut geometry = Vec::new();
    for (i, source) in original.iter().enumerate() {
        let mut row = vec![0u16; 288];
        if let Some(car) = &world.vehicles.entries[i] {
            let sizes = [Part::Body, Part::Turret].map(|p| {
                let g = source.model.parts[p.slot()].as_ref().unwrap().geometry();
                [g.vertices.len(), g.triangles.len()]
            });
            let input = super::tank_hit_oracle::input(
                car,
                world.objects.components.projectiles.shell_hit,
                world.objects.components.projectiles.homer_hit,
                sizes,
                source,
                &world.shared.temporary,
                &world.shared.pool,
                world.shared.random,
                false,
            );
            row[..63].copy_from_slice(&input[16..79]);
            let prefix = usize::from(input[12]);
            row[64..76].copy_from_slice(&input[prefix..prefix + 12]);
            let ro = usize::from(input[5]);
            row[96..208].copy_from_slice(&input[ro..ro + 112]);
            for upper in 0..2 {
                for (field, count) in [
                    (1 + upper * 2, sizes[upper][0] * 3),
                    (2 + upper * 2, sizes[upper][1] * 3),
                    (10 + upper, sizes[upper][1] * 3),
                ] {
                    row[80 + field] = geometry.len() as u16;
                    let start = usize::from(input[field]);
                    geometry.extend_from_slice(&input[start..start + count]);
                }
                row[86 + upper * 2] = sizes[upper][0] as u16;
                row[87 + upper * 2] = sizes[upper][1] as u16;
            }
        }
        profiles.extend(row);
    }
    assert_eq!(profiles.len(), 864);
    assert!(geometry.len() < 16384);
    value["damage_profiles"] = json!(profiles);
    value["damage_geometry"] = json!(geometry);
    let temporary = super::temporary_oracle::snapshot(
        &world.shared.temporary,
        &world.shared.pool,
        0,
        world.shared.random,
    );
    value["temporary"] = json!(&temporary[..274]);
    value
}

pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..31 {
        let mut world = super::reference_world::race(root);
        for _ in 0..2 {
            let car = world.vehicles.entries[0].as_ref().unwrap();
            world
                .add_vehicle(tankracer_game::vehicle_loading::Request {
                    model: car.model.clone(),
                    spawn: car.physics.motion.poses.body,
                    static_allocation: true,
                    environment: tankracer_game::vehicle_startup::Environment {
                        ambient_wobble: false,
                        session_active: false,
                        ambient_phase: 0,
                        gun_pivot: [0; 3],
                    },
                })
                .unwrap();
        }
        let original: Vec<_> = world
            .vehicles
            .entries
            .iter()
            .map(|c| c.as_ref().unwrap().clone())
            .collect();
        let position = original[0].physics.motion.poses.base_position;
        for (i, entry) in world.vehicles.entries.iter_mut().enumerate() {
            let car = entry.as_mut().unwrap();
            assert!(car.ai.is_none());
            car.slot = i as u8;
            world.vehicles.racers[i] = Some(TankId(i));
            car.physics.player =
                i == 0 && branch != 3 && branch != 4 && branch != 5 && branch != 30;
            car.visible = branch != 8;
            let p = [position[0] + i as i32 * 200000, position[1], position[2]];
            car.physics.motion.poses.base_position = p;
            car.physics.motion.poses.physical_position = p;
            car.physics.motion.poses.reference = Matrix::IDENTITY;
            car.physics.motion.poses.body = Pose {
                position: p,
                matrix: Matrix::IDENTITY,
            };
            car.physics.motion.poses.turret = Pose {
                position: p,
                matrix: Matrix::IDENTITY,
            };
            car.physics.pose_input.external = ExternalBasis::Custom(Matrix::IDENTITY);
            car.physics.motion.velocity = [11111, -22222, 33333];
            car.physics.motion.angular = [123456, -654321, 234567];
            car.physics.contact.response.timer = 17;
            car.physics.collision.collision_size = 13;
            car.hit_counts = [[1; 6]; 2];
            car.inventory.weapon = 0;
            car.inventory.ammo = 3;
        }
        let kind = match branch {
            1 | 4 | 7 | 14 | 19 => projectile::Kind::Homing,
            2 | 5 | 10 | 11 | 12 | 13 | 15 | 16 | 20 | 21 | 26 | 30 => projectile::Kind::Warp,
            _ => projectile::Kind::Shell,
        };
        {
            let car = world.vehicles.entries[0].as_mut().unwrap();
            match branch {
                6 | 7 => car.prefix.spin.speed = 1234,
                9 => car.physics.pose_input.external = ExternalBasis::None,
                10 => {
                    car.prefix.temporary_timer = 1;
                    car.prefix.temporary_slot = 2;
                }
                11 => {
                    car.prefix.temporary_timer = -1;
                    car.prefix.temporary_slot = -1;
                }
                28 => car.model.parts[Part::Turret.slot()] = None,
                29 => tankracer_game::vehicle_runtime::rcc_car_probe::tank_hit_unown(
                    car,
                    Part::Body.slot(),
                ),
                17 => {
                    car.inventory.weapon = 1;
                    car.physics.contact.response.timer = 950;
                }
                18 => car.hit_counts = [[6; 6]; 2],
                21 => car.prefix.sidegrip.shift = 31,
                _ => {}
            }
        }
        if branch == 12 {
            world.shared.pool.line_count = 100;
        }
        if branch == 13 {
            world.shared.temporary.available = u32::MAX;
            world.shared.temporary.timestamps = [1; 4];
        }
        let resources = world.objects.components.projectiles.resources.unwrap();
        let class = world.objects.components.projectiles.class.unwrap();
        let owner = Owner {
            entity: original[0].physics.entity,
            turret: Pose {
                position,
                matrix: Matrix::IDENTITY,
            },
            gun: [position[0], position[1] + 50000, position[2]],
            velocity: [0; 3],
        };
        let entity =
            rcc_lifecycle_probe::spawn_projectile(&mut world, owner, kind, 100000).unwrap();
        let id = world
            .identity
            .entities
            .record(entity)
            .unwrap()
            .object
            .and_then(Handle::obstacle)
            .unwrap();
        let start = [
            position[0],
            position[1]
                + if branch == 27 || branch == 28 {
                    2048
                } else {
                    0
                },
            position[2] - 24000,
        ];
        let state = rcc_lifecycle_probe::projectile_state(&mut world, id);
        state.owner = None;
        state.target = None;
        if branch == 30 {
            state.time = 100000;
        }
        state.velocity = [0, 0, 384000];
        if kind == projectile::Kind::Homing {
            state.reference = Some(Matrix::IDENTITY);
            state.time = 1;
            state.target_height = position[1];
            state.previous = start;
        }
        world.objects.world.colliders[id.0].pose = Pose {
            position: start,
            matrix: Matrix::IDENTITY,
        };
        if branch == 22 {
            world.objects.components.explosion_class = None;
        }
        if branch == 23 || branch == 24 {
            let mut table = (*world.registry.classes[class.0]).clone();
            if branch == 23 {
                table.methods[12] = None;
            } else {
                table.maximum_slot = 11;
            }
            let table = Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        if branch == 25 {
            world.objects.components.projectiles.resources = None;
        }
        world.objects.components.camera_impulse = Some((17, -250));
        world.objects.components.frame_group = entity.0 ^ 1;
        let stamp = if branch == 26 { 0x80000001u32 } else { 100 };
        world.objects.components.stamp_units = stamp as i32;
        let has_pool = branch != 15 && branch != 19;
        let has_temporary = branch != 14 && branch != 16;
        let simulation = branch != 20;
        let initial = snapshot(&world, &original);
        let profile = initial["damage_profiles"].as_array().unwrap();
        let shell = profile[177..194].to_vec();
        let homer = profile[194..202].to_vec();
        let mut cars = Vec::new();
        super::cars(&mut cars, &world);
        cars[213] = 1;
        let mut poses = Vec::new();
        let mut spins = Vec::new();
        for car in &world.vehicles.entries {
            let car = car.as_ref().unwrap();
            poses.push(1);
            ints(&mut poses, [car.physics.entity.0 as i32]);
            ints(&mut poses, car.physics.motion.poses.base_position);
            ints(&mut poses, car.physics.motion.poses.reference.0);
            ints(&mut spins, [car.prefix.spin.speed]);
        }
        poses.resize(216, 0);
        let fixed = world.objects.world.colliders.len();
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        for step in 0..if branch == 0 || branch == 2 { 3 } else { 1 } {
            let frame = (entity.0 ^ 1).wrapping_add(step);
            world.objects.components.frame_group = frame;
            let dt = if step == 0 { 32 } else { 97 };
            let result = rcc_lifecycle_probe::tick_projectile(
                &mut world,
                entity,
                dt,
                100000,
                simulation,
                true,
                has_pool,
                has_temporary,
            );
            let object = world
                .identity
                .entities
                .record(entity)
                .and_then(|r| r.object)
                .and_then(Handle::obstacle)
                .map(|id| id.0);
            frames.push(json!({"entity":entity.0,"status":result.map_or_else(|e|e,|()|0),"object":object,"state":snapshot(&world,&original)}));
            controls.push([13, dt, frame as i32, entity.0 as i32, 100000, stamp as i32]);
            if branch == 2 && step == 0 {
                let child = world
                    .objects
                    .components
                    .explosion_world
                    .slots
                    .iter()
                    .find_map(|s| s.entity)
                    .unwrap();
                let object = world
                    .identity
                    .entities
                    .record(child)
                    .unwrap()
                    .object
                    .and_then(Handle::obstacle)
                    .unwrap();
                let result = rcc_lifecycle_probe::tick_explosion(
                    &mut world, child, 12, 97, 100000, true, true, true,
                );
                frames.push(json!({"entity":child.0,"status":result.map_or_else(|e|e,|()|0),"object":object.0,"state":snapshot(&world,&original)}));
                controls.push([7, 97, 12, child.0 as i32, 100000, 0]);
            }
        }
        if !matches!(branch, 9 | 11 | 14 | 15 | 16 | 19 | 20 | 22 | 25 | 28 | 29) {
            assert_eq!(frames[0]["status"], 0, "direct branch {branch}");
        }
        if matches!(branch, 0 | 1 | 2 | 8 | 10 | 12 | 13 | 17 | 18 | 21 | 26) {
            assert_ne!(
                frames[0]["state"]["camera_impulse"], initial["camera_impulse"],
                "direct camera branch {branch}"
            );
        }
        if matches!(branch, 9 | 11 | 14 | 15 | 16 | 19 | 20 | 22 | 25 | 28 | 29) {
            assert!(
                frames[0]["status"].as_i64().unwrap() < 0,
                "direct error branch {branch}"
            );
        }

        if branch == 0 || branch == 1 || branch == 2 || branch == 27 {
            assert_ne!(
                frames[0]["state"]["damage_geometry"], initial["damage_geometry"],
                "damage geometry branch {branch}"
            );
            let word = if branch == 27 { 40 } else { 34 };
            assert_eq!(
                frames[0]["state"]["damage_profiles"][word], 2,
                "hit count branch {branch}"
            );
        }
        if branch == 2 {
            assert!(frames.iter().all(|f| f["status"] == 0));
            assert_ne!(frames[0]["state"]["temporary"], initial["temporary"]);
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-projectile-direct-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let r = world.objects.components.explosion_resources;
        let mut er = Vec::new();
        ints(
            &mut er,
            [
                r.main as i32,
                r.smoke as i32,
                i32::from(r.small),
                r.damping as i32,
                i32::from(r.map_fog),
            ],
        );
        let m = world
            .objects
            .components
            .blasts
            .materials
            .unwrap_or(blasts::Materials {
                band: 0,
                ring: [0; 3],
            });
        let mut materials = Vec::new();
        ints(
            &mut materials,
            [
                i32::from(world.objects.components.blasts.materials.is_some()),
                m.band as i32,
            ],
        );
        ints(&mut materials, m.ring.map(|v| v as i32));
        let mut camera = Vec::new();
        ints(&mut camera, world.objects.components.camera_basis.0);
        let mut pos = Vec::new();
        ints(&mut pos, position);
        cases.push(json!({"tag":format!("projectile-direct-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":class.0,"resources_present":world.objects.components.projectiles.resources.is_some(),"homing_mesh":resources.homing_mesh.index(),"launch":super::launch_input(resources,owner),"scene":name,"initial":initial,"controls":controls,"frames":frames,"terrain_present":true,"flight_cars":cars,"carrier_pose":vec![0u16;24],"spin_inputs":spins,"explosion_class":world.objects.components.explosion_class.map_or(-1,|v|v.0 as i32),"explosion_resources":er,"explosion_position":pos,"effect_pool_present":has_pool,"temporary_present":has_temporary,"simulation":simulation,"vehicle_count":3,"vehicle_inputs":poses,"blast_materials":materials,"blast_camera":camera,"crater":resources.crater,"shell_hit":shell,"homer_hit":homer}));
    }
    fs::write(out.join("scene-projectile-direct.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Complete original Shell/Homing/Warp class ticks with collision, reflection, explosion attachment, live vehicle damage and geometry revisions, temporary effects, camera, retirement and failure prefixes."})).unwrap()).unwrap();
}
