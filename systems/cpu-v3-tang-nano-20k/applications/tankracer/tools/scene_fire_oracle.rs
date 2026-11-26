//! Actual player fire, real factories and post-fire target selection.
use super::{ints, scene_export, scene_mines_oracle};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    dispatch::MethodId,
    math::Matrix,
    object_query::{EntityId, TankId},
    projectile::launch::Owner,
    scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::{SinglePlayer, rcc_car_probe},
};
fn snapshot(world: &SinglePlayer, shot: Option<EntityId>) -> Value {
    let mut out = scene_mines_oracle::snapshot(world);
    let car = world.vehicles.entries[0].as_ref().unwrap();
    let v = &car.physics;
    let mut fields = Vec::new();
    ints(&mut fields, [car.inventory.weapon as i32]);
    fields.push(u16::from(car.inventory.ammo));
    ints(
        &mut fields,
        [
            v.contact.response.timer,
            v.collision.impact_accumulator,
            v.collision.collision_size as i32,
            v.motion.articulation.recoil_rate,
            v.motion.articulation.pitch_rate,
            v.pose_input.recoil_mode,
        ],
    );
    let b = car.prefix.boost;
    ints(
        &mut fields,
        [
            b.camera_velocity,
            b.camera_timer,
            b.target.0 as i32,
            i32::from(b.active),
            i32::from(b.applied),
            i32::from(b.phase),
        ],
    );
    let s = car.prefix.spin;
    ints(
        &mut fields,
        [
            i32::from(s.flags),
            i32::from(s.visible),
            s.hold,
            s.color_timer,
            s.color as i32,
            s.target_color as i32,
            s.angle,
            s.speed,
            i32::from(s.disabled),
        ],
    );
    let b = world.shared.boost;
    ints(
        &mut fields,
        [b.texture_frame as i32, i32::from(b.texture_delta)],
    );
    ints(&mut fields, b.source_uv.map(i32::from));
    ints(&mut fields, b.current_uv.map(i32::from));
    ints(&mut fields, b.feedback);
    fields.extend([
        u16::from(world.shared.spin.count),
        u16::from(world.shared.spin.pulse),
    ]);
    let b = car.prefix.boost;
    ints(
        &mut fields,
        [b.extra_speed, b.power, b.power_loss, i32::from(b.timer)],
    );
    assert_eq!(fields.len(), 73);
    out["fire_state"] = json!(fields);
    let mut result = vec![u16::from(shot.is_some())];
    ints(&mut result, [shot.map_or(0, |v| v.0 as i32)]);
    out["fire_shot"] = json!(result);
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..77 {
        let mut world = super::reference_world::full_race(root);
        let fixed = world.objects.world.colliders.len();
        let pr = world.objects.components.projectiles.resources.unwrap();
        let mr = world.objects.components.mines.resources.unwrap();
        let pc = world.objects.components.projectiles.class.unwrap();
        let position = world.objects.world.colliders
            [world.objects.components.waypoints.order[10].0]
            .pose
            .position;
        let (mut weapon, mut ammo, mut cooldown) = if branch < 44 {
            (
                branch / 4,
                [0u8, 1, 3, 1][branch % 4],
                i32::from(branch % 4 == 3),
            )
        } else {
            (0, 3, 0)
        };
        let mut mode = 0;
        let mut limit = 100000;
        match branch {
            44 | 46 | 47 | 48 | 49 | 58 | 59 | 63 | 64 | 65 => weapon = 3,
            45 | 70 => weapon = 10,
            50 | 67 => weapon = 7,
            51 | 52 | 53 | 54 | 68 | 76 => weapon = 8,
            60 | 61 | 62 | 74 => weapon = 1,
            57 => weapon = u32::MAX as usize,
            69 | 73 => weapon = 6,
            _ => {}
        }
        if branch == 52 {
            mode = 4;
        }
        if branch == 53 {
            mode = 5;
        }
        if branch == 55 {
            cooldown = -1;
        }
        if branch == 56 {
            cooldown = i32::MAX;
        }
        if branch == 70 {
            ammo = 255;
        }
        if branch == 73 || branch == 74 {
            ammo = 0;
        }
        if matches!(branch, 66 | 67 | 68 | 69) {
            limit = 0;
        }
        let car = world.vehicles.entries[0].as_mut().unwrap();
        car.inventory.weapon = weapon as u32;
        car.inventory.ammo = ammo;
        car.physics.contact.response.timer = cooldown;
        car.physics.motion.poses.physical_position = position;
        car.physics.motion.poses.base_position = position;
        car.physics.motion.poses.turret.matrix = Matrix::IDENTITY;
        car.physics.motion.poses.turret.position = position;
        car.physics.motion.poses.gun.position =
            [position[0], position[1].wrapping_add(3000), position[2]];
        car.physics.motion.velocity = [10000, -2000, 30000];
        car.physics.collision.impact_accumulator = 123;
        car.physics.collision.collision_size = 456;
        car.physics.motion.articulation.recoil_rate = 789;
        car.physics.motion.articulation.pitch_rate = -987;
        car.physics.pose_input.recoil_mode = 17;
        car.prefix.boost.extra_speed = 321;
        car.prefix.boost.power = 654;
        car.prefix.boost.power_loss = 3;
        car.prefix.boost.timer = 1234;
        car.prefix.boost.camera_velocity = 111;
        car.prefix.boost.camera_timer = 222;
        car.prefix.boost.target = EntityId(333);
        car.prefix.boost.active = 3;
        car.prefix.boost.applied = 4;
        car.prefix.boost.phase = 5;
        car.prefix.spin.color = 0xdeadbeef;
        car.prefix.spin.target_color = 0x12345678;
        car.prefix.spin.hold = 123;
        car.prefix.spin.color_timer = 234;
        car.prefix.spin.angle = 345;
        car.prefix.spin.disabled = 67;
        if branch == 49 || branch == 54 {
            car.physics.player = false;
        }
        if branch == 51 {
            car.prefix.spin.speed = 1;
        }
        if branch == 71 {
            car.physics.collision.impact_accumulator = i32::MAX;
            car.physics.collision.collision_size = u32::MAX;
        }
        if branch == 72 {
            car.physics.motion.articulation.recoil_rate = i32::MIN;
            car.physics.motion.articulation.pitch_rate = i32::MIN;
        }
        if branch == 48 {
            car.physics.motion.poses.turret.matrix =
                Matrix([4096, 0, 0, 0, 4096, 0, 0, i32::MIN, 4096]);
        }
        let owner = Owner::from(&*car);
        let player = car.physics.player;
        world.shared.random = 0x19876543;
        world.shared.spin.count = 255;
        world.shared.spin.pulse = 1;
        world.shared.boost.texture_frame = 0x12345678;
        world.shared.boost.source_uv = [555, 666];
        world.shared.boost.current_uv = [777, 888];
        world.shared.boost.feedback = [999, 1111, 2222];
        world.vehicles.entries.resize_with(3, || None);
        if matches!(branch, 44 | 45 | 46 | 47 | 65) {
            for (i, z) in [(1, if branch == 65 { 30000 } else { 70000 }), (2, 30000)] {
                let mut peer = world.vehicles.entries[0].as_ref().unwrap().clone();
                let peer_entity = world.identity.entities.create_record(100000).unwrap();
                let class = world.identity.entities.record(owner.entity).unwrap().class;
                let record = world.identity.entities.record_mut(peer_entity).unwrap();
                record.class = class;
                record.object = Some(tankracer_game::scene_entity::Handle::Vehicle(TankId(i)));
                peer.physics.entity = peer_entity;
                peer.physics.player = false;
                peer.physics.motion.poses.base_position = [
                    position[0].wrapping_add(if branch == 46 { 90000 } else { 0 }),
                    position[1],
                    position[2].wrapping_add(z),
                ];
                world.vehicles.entries[i] = Some(peer);
                world.vehicles.racers[i] = Some(TankId(i));
            }
            if branch == 65 {
                world.vehicles.racers[1] = Some(TankId(2));
                world.vehicles.racers[2] = Some(TankId(1));
            }
            if branch == 47 {
                world.vehicles.entries[1] = None;
            }
        }
        let mine_owner = tankracer_game::mine::Owner {
            entity: owner.entity,
            position,
        };
        let mut value = scene_mines_oracle::bindings(&world, mine_owner, mr, json!({}));
        if branch == 58 {
            world.objects.components.projectiles.resources = None;
        }
        if branch == 59 {
            world.objects.components.projectiles.class = None;
        }
        if branch == 60 {
            world.objects.components.mines.resources = None;
        }
        if branch == 61 {
            rcc_lifecycle_probe::clear_terrain(&mut world.objects.components);
        }
        if branch == 62 {
            world.objects.components.mines.class = None;
        }
        if branch == 63 || branch == 64 {
            let mut table = (*world.registry.classes[pc.0]).clone();
            if branch == 63 {
                table.methods[4] = Some(MethodId(74));
            } else {
                table.methods[0] = None;
            }
            let table = Arc::new(table);
            world.registry.classes[pc.0] = table.clone();
            world.objects.world.classes[pc.0] = table;
        }
        let mut cars = Vec::new();
        super::cars(&mut cars, &world);
        let mut shot = Some(EntityId(0x76543210));
        world.identity.current_slot = 17;
        let initial = snapshot(&world, shot);
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        for step in 0..if branch == 75 || branch == 76 { 2 } else { 1 } {
            world.objects.components.frame_group = 100 + step as u32;
            let result = rcc_car_probe::fire(&mut world, TankId(0), mode, limit);
            let status = match result {
                Ok(value) => {
                    shot = value;
                    0
                }
                Err(e) => e,
            };
            let entity = if status == 0 {
                shot.map_or(0, |v| v.0)
            } else {
                0
            };
            let object = world
                .identity
                .entities
                .record(EntityId(entity))
                .and_then(|r| r.object)
                .and_then(|h| h.obstacle())
                .map(|v| v.0);
            frames.push(json!({"entity":entity,"status":status,"object":object,"state":snapshot(&world,shot)}));
            controls.push([19, mode, 100 + step as i32, 0, limit as i32, 0]);
        }
        if branch < 44
            || matches!(
                branch,
                44 | 45
                    | 46
                    | 49
                    | 50
                    | 51
                    | 52
                    | 53
                    | 54
                    | 55
                    | 56
                    | 57
                    | 65
                    | 67
                    | 68
                    | 69
                    | 70
                    | 71
                    | 72
                    | 73
                    | 74
                    | 75
                    | 76
            )
        {
            assert!(
                frames.iter().all(|f| f["status"] == 0),
                "fire branch {branch}"
            );
        }
        if branch == 47 {
            assert_eq!(frames[0]["status"], -4);
            assert_ne!(frames[0]["state"]["fire_state"], initial["fire_state"]);
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-fire-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        value["tag"] = json!(format!("fire-{branch}"));
        value["fixed_count"] = json!(fixed);
        value["scene"] = json!(name);
        value["initial"] = initial;
        value["frames"] = json!(frames);
        value["controls"] = json!(controls);
        value["projectile_class"] = json!(
            world
                .objects
                .components
                .projectiles
                .class
                .map_or(-1, |v| v.0 as i32)
        );
        value["resources_present"] =
            json!(world.objects.components.projectiles.resources.is_some());
        value["launch"] = json!(super::launch_input(pr, owner));
        value["mine_class"] = json!(
            world
                .objects
                .components
                .mines
                .class
                .map_or(-1, |v| v.0 as i32)
        );
        value["mine_resources_present"] = json!(world.objects.components.mines.resources.is_some());
        value["terrain_present"] = json!(branch != 61);
        value["fire_player"] = json!(player);
        value["flight_cars"] = json!(cars);
        cases.push(value);
    }
    fs::write(out.join("scene-fire.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original player Fire.apply and immediate non-AI target selection with actual projectile/mine factories, inventory byte wrap, cooldown/impact/recoil commit order, boost control, spin/shared RNG, live racer order and partial factory/selection errors. Audio is external; frame input/prepare and mixed scheduler are separate boundaries."})).unwrap()).unwrap();
}
