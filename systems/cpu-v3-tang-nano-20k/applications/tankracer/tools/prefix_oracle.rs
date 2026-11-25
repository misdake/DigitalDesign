//! Complete original car::prepare calls on persistent constructed player state.
mod identity_assets;
#[allow(dead_code)]
mod reference_world;
#[allow(dead_code)]
mod scene_export;
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    dispatch, effect_pool,
    math::Matrix,
    object_query::TankId,
    vehicle, vehicle_prefix,
    vehicle_runtime::{rcc_car_probe, SinglePlayer},
};

fn line(out: &mut Vec<i32>, row: &effect_pool::Line) {
    out.extend(row.points.into_iter().flatten());
    out.extend(row.colors.map(|c| u32::from_le_bytes(c) as i32));
}
fn snapshot(world: &SinglePlayer) -> Vec<i32> {
    let player = world.vehicles.entries[0].as_ref().unwrap();
    let s = &player.prefix;
    let shared = &world.shared;
    let mut out = vec![
        s.alternating_sign,
        s.countdown_throttle,
        s.countdown_rpm,
        s.target,
        i32::from(s.temporary_timer),
        i32::from(s.temporary_slot),
        i32::from(s.restore_sidegrip.multiplier),
        i32::from(s.restore_sidegrip.shift),
        i32::from(s.sidegrip.multiplier),
        i32::from(s.sidegrip.shift),
    ];
    let spin = &s.spin;
    out.extend([
        i32::from(spin.flags),
        i32::from(spin.visible),
        spin.hold,
        spin.color_timer,
        spin.color as i32,
        spin.target_color as i32,
        spin.angle,
        spin.speed,
        i32::from(spin.disabled),
    ]);
    let b = &s.boost;
    out.extend([
        b.extra_speed,
        b.power,
        b.power_loss,
        i32::from(b.timer),
        b.camera_velocity,
        b.camera_timer,
        b.target.0 as i32,
        i32::from(b.active),
        i32::from(b.applied),
        i32::from(b.phase),
    ]);
    for a in &player.physics.contact.sampling.auxiliary {
        out.extend(a.world);
        out.extend([a.kind, a.surface, a.cached]);
    }
    let view = &shared.view;
    out.extend([i32::from(view.enabled), i32::from(view.visible)]);
    out.extend(view.position);
    out.push(view.angular_rate);
    out.extend(view.matrix.0);
    out.extend([
        shared.camera.pitch,
        shared.camera.height,
        shared.camera.distance,
    ]);
    out.extend([i32::from(shared.spin.count), i32::from(shared.spin.pulse)]);
    out.extend([
        shared.boost.texture_frame as i32,
        i32::from(shared.boost.texture_delta),
    ]);
    out.extend(shared.boost.source_uv.map(i32::from));
    out.extend(shared.boost.current_uv.map(i32::from));
    out.extend(shared.boost.feedback);
    out.push(shared.temporary.available as i32);
    out.extend(shared.temporary.timestamps.map(|v| v as i32));
    out.extend(shared.temporary.lines.iter().flatten().map(|h| h.0 as i32));
    out.extend([
        shared.random as i32,
        world.identity.current_slot as i32,
        i32::from(player.physics.player),
        player.physics.entity.0 as i32,
    ]);
    out.extend(rcc_car_probe::prefix_config(world, TankId(0)));
    out.push(shared.pool.line_count as i32);
    for row in &shared.pool.lines {
        line(&mut out, row);
    }
    line(&mut out, &shared.pool.overflow);
    let q = &shared.pool.quads[159];
    out.extend([q.points[3][1], q.points[3][2], q.material as i32]);
    for row in q.style.compat_bytes().chunks_exact(4) {
        out.push(i32::from_le_bytes(row.try_into().unwrap()));
    }
    out.extend(
        shared
            .pool
            .exhausted_colors
            .map(|c| u32::from_le_bytes(c) as i32),
    );
    assert_eq!(out.len(), 938);
    out
}
struct Scenario {
    tag: String,
    player: i32,
    phase: i32,
    distance: i32,
    dispatch_mode: i32,
    temporary: Option<(i16, i16, u8)>,
    mixed: Option<i32>,
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(&args[0]);
    let out = Path::new(&args[1]);
    let mut world = reference_world::two_cars(root, 1);
    let original = world.vehicles.entries[0].as_ref().unwrap().clone();
    let classes = world.objects.world.classes.clone();
    let identities = world.identity.entities.clone();
    let entity = original.physics.entity;
    let peer = world.vehicles.entries[1].as_ref().unwrap().physics.entity;
    let class = original.physics.class;
    let mut cases = Vec::new();
    // Variant dimensions are independent so each branch has an explicit input.
    let mut specifications = Vec::new();
    for player in [0, 1] {
        for phase in [0, 1, 2, 4, 8, 3, 6, 16, 255] {
            for distance in [-9000, -7800, -7799, -7200, -7199, -7000] {
                specifications.push(Scenario {
                    tag: format!("camera-{player}-{phase}-{distance}"),
                    player,
                    phase,
                    distance,
                    dispatch_mode: 0,
                    temporary: None,
                    mixed: None,
                });
            }
        }
    }
    for mode in 1..=6 {
        specifications.push(Scenario {
            tag: format!("dispatch-{mode}"),
            player: 1,
            phase: 1,
            distance: -7000,
            dispatch_mode: mode,
            temporary: None,
            mixed: None,
        });
    }
    for timer in [-1, 0, 1, 33, 34, 32767] {
        for slot in [-1, 0, 3, 4] {
            for handles in 0..3 {
                specifications.push(Scenario {
                    tag: format!("temporary-{timer}-{slot}-{handles}"),
                    player: 1,
                    phase: 8,
                    distance: -8000,
                    dispatch_mode: 0,
                    temporary: Some((timer, slot, handles)),
                    mixed: None,
                });
            }
        }
    }
    for i in 0..24 {
        specifications.push(Scenario {
            tag: format!("mixed-{i}"),
            player: i & 1,
            phase: 1 << ((i % 4) as u32),
            distance: -7000,
            dispatch_mode: 0,
            temporary: None,
            mixed: Some(i),
        });
    }
    for Scenario {
        tag,
        player,
        phase,
        distance,
        dispatch_mode,
        temporary,
        mixed,
    } in specifications
    {
        world.vehicles.entries[0] = Some(original.clone());
        world.objects.world.classes = classes.clone();
        world.identity.entities = identities.clone();
        world.identity.current_slot = 12;
        world.shared.random = 0x1234abcd;
        world.shared.view = Default::default();
        world.shared.spin = Default::default();
        world.shared.camera = vehicle_prefix::boost::Camera {
            pitch: -1638,
            height: 5500,
            distance,
        };
        world.shared.boost = Default::default();
        world.shared.temporary = Default::default();
        world.shared.pool = effect_pool::Pool::default();
        world.shared.boost.source_uv = [0, 65535];
        world.shared.boost.current_uv = [900, 123];
        {
            let p = world.vehicles.entries[0].as_mut().unwrap();
            p.physics.player = player != 0;
            p.prefix = Default::default();
            let s = &mut p.prefix;
            s.alternating_sign = -1;
            s.countdown_throttle = 65535;
            s.countdown_rpm = 3000;
            s.restore_sidegrip.multiplier = 17;
            s.restore_sidegrip.shift = 3;
            s.sidegrip.multiplier = 3;
            s.sidegrip.shift = 7;
            s.boost.active = 1;
            s.boost.phase = phase as u8;
            s.boost.target = entity;
            s.boost.camera_timer = 199;
            s.boost.camera_velocity = if phase == 4 { -4096 } else { 0 };
            s.boost.timer = if phase == 8 { 1 } else { 5000 };
            s.boost.extra_speed = 499999;
            s.boost.power = 12345;
            s.boost.power_loss = 100;
            for (i, a) in p.physics.contact.sampling.auxiliary.iter_mut().enumerate() {
                a.world = [100 + i as i32, 200, 300];
                a.kind = 2;
                a.surface = 3;
                a.cached = -4;
            }
            if dispatch_mode == 1 {
                s.boost.target = peer;
            }
            if dispatch_mode == 2 {
                s.boost.target = tankracer_game::object_query::EntityId(u32::MAX);
            }
        }
        if dispatch_mode >= 3 {
            if dispatch_mode == 6 {
                world.identity.entities.record_mut(entity).unwrap().class = None;
            } else {
                let table = Arc::make_mut(&mut world.objects.world.classes[class.0]);
                if dispatch_mode == 3 {
                    table.maximum_slot = 27;
                }
                if dispatch_mode == 4 {
                    table.methods[28] = None;
                }
                if dispatch_mode == 5 {
                    table.methods[28] = Some(dispatch::MethodId(75));
                }
            }
        }
        let mut controls = Vec::new();
        for i in 0..8 {
            controls.push([33, 31 + i, if i % 3 == 0 { 0 } else { 1 }, 0, 0, 0]);
        }
        if let Some((timer, slot, handles)) = temporary {
            let s = &mut world.vehicles.entries[0].as_mut().unwrap().prefix;
            s.temporary_timer = timer;
            s.temporary_slot = slot;
            world.shared.temporary.available = u32::MAX;
            world.shared.temporary.timestamps = [11, 22, 33, 44];
            for i in 0..9 {
                world
                    .shared
                    .pool
                    .allocate([[i, 1, 2], [3, 4, 5]], [[1, 2, 3, 4]; 2]);
            }
            if (0..4).contains(&slot) {
                for i in 0..9 {
                    world.shared.temporary.lines[slot as usize][i] =
                        effect_pool::LineHandle(if handles == 1 && i == 4 {
                            u32::MAX
                        } else if handles == 2 && i == 4 {
                            0xfe5cbe12
                        } else {
                            i as u32
                        });
                }
            }
        }
        if let Some(i) = mixed {
            let s = &mut world.vehicles.entries[0].as_mut().unwrap().prefix;
            s.spin = vehicle_prefix::spin::State {
                flags: 2,
                visible: 1,
                hold: 0,
                color_timer: 61,
                color: 0xaaaaffff,
                target_color: 0x55550000,
                angle: 32760,
                speed: 16307,
                disabled: 0,
            };
            s.boost.applied = 1;
            s.boost.extra_speed = i32::MAX;
            s.countdown_throttle = i32::MIN;
            s.countdown_rpm = i32::MAX;
            world.shared.view.enabled = true;
            world.shared.view.visible = true;
            world.shared.view.position = [800, 1600, 2400];
            world.shared.view.matrix = if i % 3 == 0 {
                Matrix([0; 9])
            } else {
                Matrix::IDENTITY
            };
            controls = (0..8)
                .map(|k| {
                    [
                        [-33, 0, 1, 33, 61, 255, 256, 1000][(i % 8) as usize],
                        32 + k / 2,
                        k & 1,
                        if i % 2 == 0 { 100 } else { 19300 },
                        0,
                        300,
                    ]
                })
                .collect();
        }
        let scene_name = format!("prefix-scene-{dispatch_mode}.bin");
        let scene = scene_export::encode(&world.objects.world).0;
        let bytes = scene
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        let path = out.join(&scene_name);
        if path.exists() {
            assert_eq!(fs::read(&path).unwrap(), bytes);
        } else {
            fs::write(path, &bytes).unwrap();
        }
        let identity = identity_assets::encode(&world.identity.entities);
        let mut input = snapshot(&world);
        input.extend([bytes.len() as i32, controls.len() as i32]);
        input.extend(controls.iter().flatten());
        let mut frames = Vec::new();
        for c in controls {
            let frame = vehicle::Frame {
                dt: c[0],
                number: c[1] as u32,
                mode: 0,
                input: Default::default(),
                drive: tankracer_game::drive::Environment {
                    active: c[2] != 0,
                    ..Default::default()
                },
            };
            let status = rcc_car_probe::prepare(&mut world, TankId(0), &frame, [c[3], c[4], c[5]])
                .map_or_else(|e| e, |()| 0);
            let mut capture = vec![status];
            capture.extend(snapshot(&world));
            frames.push(capture);
        }
        cases.push(
            json!({"tag":tag,"input":input,"frames":frames,"scene":scene_name,"identity":identity}),
        );
    }
    fs::write(out.join("prefix.json"),serde_json::to_vec(&json!({"scope":"Original car::prepare on constructed vehicle/config, full prefix/shared/pool writes, class dispatch, RNG and error prefixes","cases":cases})).unwrap()).unwrap();
}
