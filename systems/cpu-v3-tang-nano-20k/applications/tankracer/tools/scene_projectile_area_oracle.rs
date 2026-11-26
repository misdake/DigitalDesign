//! Complete original Concussion/Debris ticks through marks, explosions and racers.
use super::{ints, scene_explosions_oracle, scene_export};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    explosion::blasts,
    math::Matrix,
    object_query::{EntityId, TankId},
    pose::Pose,
    projectile::{self, launch::Owner},
    scene_entity::Handle,
    scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
};
pub(super) fn snapshot(world: &SinglePlayer) -> Value {
    let mut value = scene_explosions_oracle::snapshot(world);
    let mut cars = Vec::new();
    for car in &world.vehicles.entries {
        let start = cars.len();
        if let Some(car) = car {
            let v = &car.physics;
            let b = &v.blast;
            ints(
                &mut cars,
                [
                    1,
                    v.entity.0 as i32,
                    i32::from(v.player),
                    i32::from(car.visible),
                ],
            );
            ints(&mut cars, v.motion.poses.base_position);
            ints(&mut cars, v.motion.poses.physical_position);
            ints(&mut cars, v.motion.velocity);
            ints(&mut cars, v.motion.angular);
            ints(
                &mut cars,
                [
                    b.health,
                    i32::from(b.health_timer),
                    i32::from(b.health_color),
                    b.protection_timer,
                    i32::from(b.angular_enabled),
                    i32::from(b.hit_axes[0]),
                    i32::from(b.hit_axes[1]),
                    v.collision.collision_size as i32,
                    car.prefix.spin.speed,
                ],
            );
            let body = car.model.parts[tankracer_assets::config::Part::Body.slot()].as_ref();
            ints(
                &mut cars,
                [
                    i32::from(body.is_some()),
                    body.map_or(0, |v| i32::from(v.source.bounds[1][2])),
                ],
            );
            assert_eq!(cars.len() - start, 54);
        }
        cars.resize(start + 64, 0);
    }
    assert!(cars.len() <= 512);
    cars.resize(512, 0);
    value["area_cars"] = json!(cars);
    let marks = &world.objects.components.projectiles.marks;
    let mut packed = Vec::new();
    ints(
        &mut packed,
        [marks.count as i32, marks.head as i32, marks.next as i32],
    );
    ints(&mut packed, marks.handles.map(|v| v.0 as i32));
    value["marks"] = json!(packed);
    let (duration, magnitude) = world.objects.components.camera_impulse.unwrap_or((0, 0));
    let mut impulse = Vec::new();
    ints(
        &mut impulse,
        [
            i32::from(world.objects.components.camera_impulse.is_some()),
            duration as i32,
            magnitude,
        ],
    );
    value["camera_impulse"] = json!(impulse);
    let mut overlay = vec![u16::from(world.objects.components.overlay.is_some())];
    overlay.extend(
        world
            .objects
            .components
            .overlay
            .unwrap_or([0; 4])
            .map(u16::from),
    );
    value["overlay"] = json!(overlay);
    value
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..28 {
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
        let initial_position = world.vehicles.entries[0]
            .as_ref()
            .unwrap()
            .physics
            .motion
            .poses
            .base_position;
        let mut origin = initial_position;
        let mut found = false;
        for dx in [0, -512, 512, -1024, 1024, -2048, 2048, -4096, 4096] {
            for dz in [0, -512, 512, -1024, 1024, -2048, 2048, -4096, 4096] {
                let p = [initial_position[0] + dx, 0, initial_position[2] + dz];
                if !found && super::projectile_marks_oracle::category(world.terrain(), p) == 4 {
                    origin = p;
                    found = true;
                }
            }
        }
        assert!(found);
        let mut height = 0;
        assert!(
            tankracer_game::terrain::height(world.terrain(), origin[0], origin[2], &mut height)
                .unwrap()
        );
        origin[1] = height + 1024;
        let kind = if !matches!(branch, 9 | 11 | 19) && (branch == 10 || branch % 2 == 1) {
            projectile::Kind::Debris
        } else {
            projectile::Kind::Concussion
        };
        let radius = if kind == projectile::Kind::Debris {
            20480
        } else {
            30720
        };
        for (i, car) in world.vehicles.entries.iter_mut().enumerate() {
            let car = car.as_mut().unwrap();
            assert!(car.ai.is_none());
            car.slot = i as u8;
            world.vehicles.racers[i] = Some(TankId(i));
            let v = &mut car.physics;
            v.player = i < 2;
            car.visible = !(branch == 23 && i == 1);
            v.motion.poses.base_position = [
                origin[0] + i as i32 * 2048,
                initial_position[1],
                origin[2] + i as i32 * 1024,
            ];
            v.motion.poses.physical_position = [
                origin[0] + i as i32 * 1200,
                initial_position[1] + 321,
                origin[2] - i as i32 * 900,
            ];
            v.motion.velocity = [11111, -22222, 33333];
            v.motion.angular = [123456, -654321, 234567];
            v.blast.health = if branch == 27 { -123 } else { 12345 };
            v.blast.health_timer = 17;
            v.blast.health_color = 33;
            v.blast.protection_timer = 0;
            v.blast.angular_enabled = false;
            v.blast.hit_axes = [1, 2];
            v.collision.collision_size = 13;
        }
        if branch == 4 {
            world.vehicles.entries[1]
                .as_mut()
                .unwrap()
                .physics
                .blast
                .protection_timer = 1;
        }
        if branch == 5 {
            world.vehicles.entries[1]
                .as_mut()
                .unwrap()
                .physics
                .blast
                .protection_timer = -1;
        }
        if branch == 6 {
            world.vehicles.entries[1]
                .as_mut()
                .unwrap()
                .physics
                .motion
                .poses
                .base_position[0] = origin[0] + radius;
        }
        if branch == 7 {
            world.vehicles.entries[1]
                .as_mut()
                .unwrap()
                .physics
                .motion
                .poses
                .physical_position = [origin[0] + 100000, origin[1], origin[2]];
        }
        if branch == 8 {
            world.vehicles.entries[1]
                .as_mut()
                .unwrap()
                .physics
                .motion
                .poses
                .base_position[0] = origin[0] + 10000;
        }
        if branch == 10 {
            world.vehicles.entries[0]
                .as_mut()
                .unwrap()
                .prefix
                .spin
                .speed = 1234;
        }
        if branch == 16 {
            world.vehicles.racers[0] = None;
        }
        if branch == 17 {
            world.vehicles.entries[2] = None;
        }
        if branch == 18 {
            world.shared.pool.quad_count = 160;
        }
        if branch == 22 {
            world.vehicles.racers[2] = Some(TankId(1));
            world.vehicles.racers[8] = Some(TankId(2));
        }
        let resources = world.objects.components.projectiles.resources.unwrap();
        let class = world.objects.components.projectiles.class.unwrap();
        let owner = Owner {
            entity: world.vehicles.entries[0].as_ref().unwrap().physics.entity,
            turret: Pose {
                position: origin,
                matrix: Matrix::IDENTITY,
            },
            gun: [origin[0], origin[1] + 50000, origin[2]],
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
        let state = rcc_lifecycle_probe::projectile_state(&mut world, id);
        state.velocity = [0; 3];
        if !matches!(branch, 2 | 3) {
            state.owner = None;
        }
        if matches!(branch, 9 | 10) {
            state.owner = None;
            state.velocity = [0, 0, 384000];
        }
        world.objects.world.colliders[id.0].pose.position = if matches!(branch, 9 | 10) {
            let p = world.vehicles.entries[0]
                .as_ref()
                .unwrap()
                .physics
                .motion
                .poses
                .base_position;
            [p[0], p[1], p[2] - 24000]
        } else {
            [origin[0], height - 8192, origin[2]]
        };
        if branch == 19 {
            world.objects.components.explosion_class = None;
        }
        if branch == 13 {
            world.objects.components.projectiles.resources = None;
        }
        if branch == 14 {
            rcc_lifecycle_probe::clear_terrain(&mut world.objects.components);
        }
        if branch == 21 {
            world.objects.components.objects[id.0]
                .as_mut()
                .unwrap()
                .emitter = Some(EntityId(0x76543210));
        }
        if branch == 24 || branch == 25 {
            let mut table = (*world.registry.classes[class.0]).clone();
            if branch == 24 {
                table.methods[12] = None;
            } else {
                table.maximum_slot = 11;
            }
            let table = Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        world.objects.components.camera_impulse = Some((17, -250));
        world.objects.components.frame_group = entity.0;
        world.objects.components.stamp_units = 100;
        let has_vehicles = branch != 15;
        let has_pool = branch != 12;
        let simulation = branch != 11;
        let fixed = world.objects.world.colliders.len();
        let initial = snapshot(&world);
        let mut poses = Vec::new();
        for car in &world.vehicles.entries {
            poses.push(u16::from(car.is_some()));
            ints(
                &mut poses,
                [car.as_ref().map_or(0, |v| v.physics.entity.0 as i32)],
            );
            ints(
                &mut poses,
                car.as_ref()
                    .map_or([0; 3], |v| v.physics.motion.poses.base_position),
            );
            ints(
                &mut poses,
                car.as_ref()
                    .map_or(Matrix([0; 9]), |v| v.physics.motion.poses.reference)
                    .0,
            );
        }
        poses.resize(216, 0);
        let mut cars = Vec::new();
        super::cars(&mut cars, &world);
        cars[213] = u16::from(has_vehicles);
        let mut spins = Vec::new();
        for car in &world.vehicles.entries {
            ints(
                &mut spins,
                [car.as_ref().map_or(0, |v| v.prefix.spin.speed)],
            );
        }
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        for step in 0..if branch == 20 { 6 } else { 1 } {
            world.objects.components.frame_group = entity.0.wrapping_add(step);
            let dt = if step > 0 {
                200
            } else if matches!(branch, 9 | 10) {
                32
            } else {
                33
            };
            let limit = if branch == 26 { 0 } else { 100000 };
            let result = rcc_lifecycle_probe::tick_explosion(
                &mut world,
                entity,
                12,
                dt,
                limit,
                simulation,
                has_vehicles,
                has_pool,
            );
            frames.push(json!({"entity":entity.0,"status":result.map_or_else(|e|e,|()|0),"object":id.0,"state":snapshot(&world)}));
            controls.push([
                13,
                dt,
                world.objects.components.frame_group as i32,
                entity.0 as i32,
                limit as i32,
                100,
            ]);
            if branch == 20 && step == 0 {
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
                frames.push(json!({"entity":child.0,"status":result.map_or_else(|e|e,|()|0),"object":object.0,"state":snapshot(&world)}));
                controls.push([7, 97, 12, child.0 as i32, 100000, 0]);
            }
        }
        if matches!(
            branch,
            0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 18 | 20 | 21 | 22 | 23 | 24 | 25 | 27
        ) {
            assert_eq!(frames[0]["status"], 0, "branch {branch}");
        }
        if branch == 10 {
            assert_eq!(frames[0]["state"]["explosion_world"][0], 0);
        }
        if branch == 11 {
            assert_eq!(frames[0]["state"]["marks"][0], 1);
            assert_eq!(frames[0]["status"], -5);
        }
        if branch == 7 {
            assert_eq!(frames[0]["state"]["area_cars"][64 + 46], 13);
            assert_eq!(frames[0]["state"]["camera_impulse"][2], 300);
        }
        if branch == 21 {
            assert_eq!(
                frames[0]["state"]["weapons"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()["meta"][7],
                1
            );
        }
        if branch == 20 {
            assert!(frames.iter().all(|v| v["status"] == 0));
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-projectile-area-{branch}.bin");
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
        ints(&mut pos, origin);
        cases.push(json!({"tag":format!("projectile-area-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":class.0,"resources_present":world.objects.components.projectiles.resources.is_some(),"homing_mesh":resources.homing_mesh.index(),"launch":super::launch_input(resources,owner),"scene":name,"initial":initial,"controls":controls,"frames":frames,"terrain_present":branch!=14,"flight_cars":cars,"carrier_pose":vec![0u16;24],"spin_inputs":spins,"explosion_class":world.objects.components.explosion_class.map_or(-1,|v|v.0 as i32),"explosion_resources":er,"explosion_position":pos,"effect_pool_present":has_pool,"simulation":simulation,"vehicle_count":if has_vehicles{world.vehicles.entries.len()}else{0},"vehicle_inputs":poses,"blast_materials":materials,"blast_camera":camera,"crater":resources.crater}));
    }
    fs::write(out.join("scene-projectile-area.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Complete original class-table Concussion/Debris ticks: flight/reflection, two-corner marks, shared explosion, emitter transfer, flash, ordered non-AI racer blast, camera and retirement; includes next parent and child updates. Direct Shell/Homing/Warp hits have a separate complete-Tick qualifier; UFO transfer and mixed whole-world native frames remain integration work."})).unwrap()).unwrap();
}
