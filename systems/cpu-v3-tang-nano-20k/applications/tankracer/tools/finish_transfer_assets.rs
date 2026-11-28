//! Original finish takeover and whole-world continuation snapshots.
use super::*;
use tankracer_game::{effect_pool::QuadHandle, object_query::TankId};

fn ints(out: &mut Vec<u16>, values: impl IntoIterator<Item = i32>) {
    for value in values {
        out.extend([value as u16, (value >> 16) as u16]);
    }
}
fn geometry(car: &Player, part: Part) -> Vec<u16> {
    let mesh = car.model.parts[part.slot()].as_ref().unwrap().geometry();
    let mut out: Vec<_> = car.model.damage_vertices[usize::from(part == Part::Turret)]
        .map(u16::from)
        .to_vec();
    out.extend(mesh.vertices.iter().flatten().map(|v| *v as u16));
    out.extend(mesh.triangles.iter().flat_map(|t| t.indices.map(u16::from)));
    out.extend(
        mesh.triangles
            .iter()
            .flat_map(|t| t.normal.map(|v| v as u16)),
    );
    out
}
pub fn write(world: &mut SinglePlayer, stages: &[Vec<i32>; 2], baseline: &[Vec<u16>], out: &Path) {
    let scope = std::env::var("TANKRACER_DRIVING_SCOPE").unwrap_or_default();
    let ufo = scope == "finish-ufo";
    let world_pass = scope == "finish-world" || ufo;
    let sequence = scope == "finish-run" || world_pass;
    let success = scope == "finish-tick" || sequence;
    let own = world.player.0;
    assert_eq!(own, 0);
    let old = world.vehicles.entries[own].as_ref().unwrap().physics.entity;
    let car = world.vehicles.entries[own].as_mut().unwrap();
    car.race.laps = 3;
    if sequence {
        car.physics.motion.articulation.recoil = 0;
        car.physics.motion.articulation.recoil_rate = 0;
    }
    car.input.drive.alternate = true;
    car.model.config.tank.drive.max_turn = 12345;
    car.brake.saved_turn = 321;
    car.lights.active = [1, 1];
    car.lights.handles = [QuadHandle(0), QuadHandle(1)];
    for quad in &mut world.shared.pool.quads[..2] {
        assert_eq!(quad.points[0][1], 0xe0000000u32 as i32);
        quad.points[0][1] = 0;
    }
    world.shared.pool.quad_count += 2;
    for (i, bank) in world.shared.pickup.banks.iter_mut().enumerate() {
        bank.target = Some(old);
        bank.active = (i == 0) as _;
    }
    for bank in &mut world.shared.debris.banks {
        bank.owner = Some(old);
    }
    let mut launch = false;
    let mut devices = driving_devices::Devices { models: &[] };
    let number = world.frame;
    let status = if world_pass {
        tankracer_game::vehicle_runtime::rcc_player_probe::advance(world, 33, &mut devices);
        Ok(())
    } else {
        tankracer_game::vehicle_runtime::rcc_player_probe::update(
            world,
            None,
            33,
            number,
            100000,
            &mut launch,
            &mut devices,
            if success {
                tankracer_game::vehicle_runtime::rcc_player_probe::Stage::Complete
            } else {
                tankracer_game::vehicle_runtime::rcc_player_probe::Stage::FinishDispatchFault
            },
            None,
        )
    };
    assert_eq!(status, if success { Ok(()) } else { Err(-4) });
    assert_eq!(world.player, TankId(2));
    let car = world.vehicles.entries[2].as_ref().unwrap();
    assert_eq!(car.lights.active, [0, 0]);
    assert_eq!(
        world.vehicles.entries[own].as_ref().unwrap().lights.active,
        [1, 1]
    );
    let current_stages = std::array::from_fn(|i| {
        let car = world.vehicles.entries[i].as_ref().unwrap();
        reference::drive_fixture::flatten(&car.physics.powertrain.drive_state(&car.physics.motion))
    });
    capture(
        world,
        if world_pass { &current_stages } else { stages },
        baseline,
        out,
        0,
        success,
    );
    if sequence {
        let mut input = Vec::new();
        let mut updates = if world_pass { 2 } else { 1 };
        let steps = if ufo { 48 } else { 4 };
        let mut checks = Vec::new();
        let mut last_flags = 0;
        for index in 1..=steps {
            world.vehicles.entries[0]
                .as_mut()
                .unwrap()
                .physics
                .motion
                .poses
                .base_position = [2000000, 3000000, 4000000];
            let body = world.vehicles.entries[2]
                .as_ref()
                .unwrap()
                .physics
                .motion
                .poses
                .base_position;
            world.camera_position = body.map(|v| v >> 3);
            world.camera_position[0] += match if ufo { 0 } else { index } {
                1 => 20000,
                2 => 10000,
                _ => 0,
            };
            if index == 3 && !ufo {
                world.frame = 0x7fff_ffff;
            }
            if ufo && index == 1 {
                let target = world.vehicles.entries[2].as_ref().unwrap().physics.entity;
                tankracer_game::scene_objects::rcc_lifecycle_probe::spawn_ufo(
                    world, None, target, [0; 3], 100000, true,
                )
                .unwrap();
            }
            ints(&mut input, world.camera_position);
            ints(&mut input, [world.frame.wrapping_add(1) as i32]);
            ints(
                &mut input,
                world.vehicles.entries[0]
                    .as_ref()
                    .unwrap()
                    .physics
                    .motion
                    .poses
                    .base_position,
            );
            updates += tankracer_game::vehicle_runtime::rcc_player_probe::advance(
                world,
                if ufo { 112 } else { 33 },
                &mut devices,
            );
            if index == 3 && !ufo {
                assert_eq!(
                    world.vehicles.entries[2]
                        .as_ref()
                        .unwrap()
                        .physics
                        .pose_input
                        .recoil_mode,
                    1
                );
            }
            let stages = std::array::from_fn(|i| {
                let car = world.vehicles.entries[i].as_ref().unwrap();
                reference::drive_fixture::flatten(
                    &car.physics.powertrain.drive_state(&car.physics.motion),
                )
            });
            let flags = world
                .objects
                .components
                .ufos
                .iter()
                .flatten()
                .next()
                .map_or(0, |u| u.flags);
            if !ufo || index == 1 || index == steps || index % 8 == 0 || flags != last_flags {
                capture(world, &stages, baseline, out, index, true);
                checks.push(index);
            }
            last_flags = flags;
        }
        if ufo {
            assert!(
                world.objects.components.ufos.iter().all(Option::is_none),
                "finish UFO must retire"
            );
            assert!(
                !world.vehicles.entries[2]
                    .as_ref()
                    .unwrap()
                    .physics
                    .collision
                    .disabled
            );
        }
        fs::write(
            out.join("finish-run-checks.json"),
            serde_json::to_vec(&checks).unwrap(),
        )
        .unwrap();
        fs::write(
            out.join("finish-run-input.bin"),
            input
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        fs::write(
            out.join("finish-run-updates.json"),
            serde_json::to_vec(&updates).unwrap(),
        )
        .unwrap();
    }
}
fn capture(
    world: &SinglePlayer,
    stages: &[Vec<i32>; 2],
    baseline: &[Vec<u16>],
    out: &Path,
    index: usize,
    success: bool,
) {
    let own = 0;
    let old = world.vehicles.entries[own].as_ref().unwrap().physics.entity;
    let car = world.vehicles.entries[2].as_ref().unwrap();
    let mut data = vec![0u16; 32];
    let mut sections = Vec::new();
    let mut drive_stage = stages[0].clone();
    if success {
        drive_stage = reference::drive_fixture::flatten(
            &car.physics.powertrain.drive_state(&car.physics.motion),
        );
    }
    sections.push(
        core(car, world.terrain(), &drive_stage)
            .into_iter()
            .flat_map(|v| [v as u16, (v >> 16) as u16])
            .collect::<Vec<_>>(),
    );
    sections.push(complete_scene_assets::car(world, 2));
    let mut prefix = Vec::new();
    prefix_scene_assets::car(&mut prefix, car);
    sections.push(prefix);
    sections.push(player_scene_assets::car_fields(car));
    let mut inventory = Vec::new();
    ints(
        &mut inventory,
        [car.inventory.weapon as i32, i32::from(car.inventory.ammo)],
    );
    sections.push(inventory);
    let mut tail = vec![
        u16::from(car.horn_down),
        u16::from(car.horn_latched),
        car.horn_timer as u16,
    ];
    let gun = car.model.parts[Part::Gun.slot()].as_ref().unwrap();
    for shift in [0, 16, 32, 48] {
        tail.push((gun.revision() >> shift) as u16);
    }
    sections.push(tail);
    let mut hits: Vec<_> = car
        .hit_counts
        .into_iter()
        .flatten()
        .map(u16::from)
        .collect();
    for part in [Part::Body, Part::Turret] {
        let revision = car.model.parts[part.slot()].as_ref().unwrap().revision();
        for shift in [0, 16, 32, 48] {
            hits.push((revision >> shift) as u16);
        }
    }
    let gain = car.model.config.tank.drive.sidegrip_fraction;
    ints(
        &mut hits,
        [i32::from(gain.multiplier), i32::from(gain.shift)],
    );
    sections.push(hits);
    let body = car.model.parts[Part::Body.slot()].as_ref().unwrap();
    let turret = car.model.parts[Part::Turret.slot()].as_ref().unwrap();
    sections.push(vec![
        1,
        u16::from(body.is_private()),
        1,
        u16::from(turret.is_private()),
        turret.geometry().vertices.len() as u16,
        turret.geometry().triangles.len() as u16,
        body.source.bounds[1][2] as u16,
    ]);
    sections.push(geometry(car, Part::Body));
    sections.push(geometry(car, Part::Turret));
    let input = car.input.drive;
    let bounds = car.model.geometry_bounds;
    let track = car.model.parts[Part::LeftTrack.slot()].as_ref();
    let mut metadata = Vec::new();
    ints(
        &mut metadata,
        [
            car.inactive_height,
            input.throttle,
            input.brake,
            input.steering,
            input.action_flags as i32,
            i32::from(input.alternate),
            i32::from(input.alternate_lateral),
            i32::from(input.alternate_rotation),
            i32::from(track.is_some()),
            track.map_or(0, |t| i32::from(t.source.bounds[1][2])),
            bounds.min[0],
            bounds.max[0],
            bounds.min[2],
            bounds.max[2],
        ],
    );
    let t = car.turret;
    ints(
        &mut metadata,
        [
            i32::from(car.input.start_held),
            i32::from(car.input.fast_start),
            i32::from(car.brake.saved_turn),
            car.model.config.tank.drive.max_turn,
            car.physics.yaw_lock,
            t.velocity,
            t.cooldown,
            i32::from(t.centering),
            i32::from(t.moving),
            i32::from(t.center_lock),
            i32::from(t.pressed[0]),
            i32::from(t.pressed[1]),
            i32::from(car.race.driving),
        ],
    );
    sections.push(metadata);
    let mut ai = Vec::new();
    ints(&mut ai, finish_scene_assets::state(car));
    sections.push(ai);
    sections.push(complete_scene_assets::capture(world));
    assert_eq!(
        sections.iter().map(Vec::len).collect::<Vec<_>>(),
        [590, 956, 50, 30, 4, 7, 24, 7, 286, 148, 54, 60, 10468]
    );
    if success {
        for i in 0..2 {
            sections.push(
                core(
                    world.vehicles.entries[i].as_ref().unwrap(),
                    world.terrain(),
                    &stages[i],
                )
                .into_iter()
                .flat_map(|v| [v as u16, (v >> 16) as u16])
                .collect(),
            );
        }
        let mut auxiliary = Vec::new();
        for c in world.vehicles.entries[..3].iter().flatten() {
            let v = &c.physics;
            ints(
                &mut auxiliary,
                [v.impact_cooldown as i32, v.collision.timer],
            );
            ints(&mut auxiliary, v.collision.tank_delta);
            ints(&mut auxiliary, v.collision.ground_delta);
            auxiliary.extend(c.damage.body.map(u16::from));
            auxiliary.extend(c.damage.turret.map(u16::from));
            auxiliary.extend(c.damage.tracks.map(u16::from));
            ints(
                &mut auxiliary,
                [
                    v.collision.material,
                    v.collision.deformable.map_or(-1, |e| e.0 as i32),
                    v.collision.impact_accumulator,
                    v.collision.collision_size as i32,
                    v.collision.obstacle_impulse,
                ],
            );
        }
        assert_eq!(auxiliary.len(), 108);
        auxiliary.push(u16::from(car.rank));
        sections.push(auxiliary);
    }
    for (i, section) in sections.into_iter().enumerate() {
        data[16 + i] = data.len() as u16;
        data.extend(section);
    }
    let entity = car.physics.entity;
    let header = [
        0x4654,
        1,
        data.len() as u16,
        own as u16,
        entity.0 as u16,
        (entity.0 >> 16) as u16,
        car.physics.class.0 as u16,
        old.0 as u16,
        (old.0 >> 16) as u16,
        world.player.0 as u16,
        world.vehicles.visible.len() as u16,
        world.identity.current_slot as u16,
    ];
    data[..12].copy_from_slice(&header);
    assert!(world.vehicles.visible.len() <= 2);
    data[12] = world
        .vehicles
        .visible
        .first()
        .map_or(65535, |id| id.0 as u16);
    data[13] = world
        .vehicles
        .visible
        .get(1)
        .map_or(65535, |id| id.0 as u16);
    data[14] = world.shared.random as u16;
    data[15] = (world.shared.random >> 16) as u16;
    fs::write(
        out.join(if index == 0 {
            "finish-transfer.bin".to_string()
        } else {
            format!("finish-run-{index}.bin")
        }),
        data.iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let state = scenery_frame::capture(world, baseline);
    fs::write(
        out.join(if index == 0 {
            "finish-transfer-world.bin".to_string()
        } else {
            format!("finish-run-world-{index}.bin")
        }),
        state
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
