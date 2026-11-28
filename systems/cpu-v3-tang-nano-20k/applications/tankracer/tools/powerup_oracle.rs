//! Authored pickup flight/settling and inventory sequences on original methods.
#[allow(dead_code)]
#[path = "terrain_assets.rs"]
mod pages;
#[allow(dead_code)]
mod reference_world;
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    actor_startup::{Actor, pickup::Pickup},
    inventory,
    object_query::ObstacleId,
    pose::Pose,
    powerup,
};

fn clone_pickup(p: &Pickup) -> Pickup {
    let f = &p.flight;
    Pickup {
        flight: powerup::Flight {
            angular: f.angular,
            normal: f.normal,
            parachute: f.parachute,
            blink_time: f.blink_time,
            blink_count: f.blink_count,
            delay: f.delay,
            disabled: f.disabled,
        },
        kind: p.kind,
        model: Arc::clone(&p.model),
        parachute: Arc::clone(&p.parachute),
        parachute_mesh: p.parachute_mesh,
        ground: p.ground,
        fall_range: p.fall_range,
        phase: p.phase,
        added_lap: p.added_lap,
        regeneration: p.regeneration,
        restitution: p.restitution,
        auxiliary: p.auxiliary,
        mass: p.mass,
    }
}
fn pose(out: &mut Vec<i32>, p: Pose) {
    out.extend(p.matrix.0);
    out.extend(p.position);
}
fn values(p: &Pickup, body: Pose, flags: u32) -> Vec<i32> {
    let f = &p.flight;
    let mut out = f.angular.to_vec();
    out.extend(f.normal);
    out.extend([
        f.blink_time,
        f.blink_count as i32,
        f.delay,
        i32::from(f.disabled),
        i32::from(f.parachute.is_some()),
        p.ground,
        p.fall_range,
        p.regeneration,
    ]);
    if let Some(p) = f.parachute {
        pose(&mut out, p);
    } else {
        out.extend([0; 12]);
    }
    pose(&mut out, body);
    out.push(flags as i32);
    assert_eq!(out.len(), 39);
    out
}
fn case(
    tag: String,
    mut p: Pickup,
    mut body: Pose,
    mut flags: u32,
    controls: Vec<[i32; 14]>,
    data: &tankracer_assets::terrain::TerrainData,
) -> Value {
    let mut input = vec![0, controls.len() as i32];
    input.extend(values(&p, body, flags));
    input.extend(controls.iter().flatten());
    let mut frames = Vec::new();
    for c in controls {
        let frame = powerup::Frame {
            dt: c[1],
            number: c[2] as u32,
            entity: c[3] as u32,
            player: [c[4], c[5], c[6]],
            camera: [c[7], c[8], c[9]],
            visibility: c[10] as u16,
            auxiliary_active: c[11] != 0,
            separation: c[12],
            diameter: c[13] as u32,
        };
        let status = if c[0] == 1 {
            p.finish_frame(&mut flags, frame.dt);
            0
        } else {
            match p.phase(&mut body, &mut flags, frame, data) {
                Err(code) => code,
                Ok(action) => {
                    if c[0] == 2 {
                        p.finish_frame(&mut flags, frame.dt);
                    }
                    match action {
                        powerup::Action::None => 0,
                        powerup::Action::Activate => 1,
                        powerup::Action::Auxiliary => 2,
                    }
                }
            }
        };
        let mut out = vec![status];
        out.extend(values(&p, body, flags));
        frames.push(out);
    }
    json!({"tag":tag,"input":input,"frames":frames})
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(&args[0]);
    let out = Path::new(&args[1]);
    let mut session = reference_world::complete(root);
    let world = session.world_mut_at_load().unwrap();
    let terrain = world.terrain();
    let geometry = *world.objects.components.actors.pickup_geometry();
    let mut cases = Vec::new();
    let mut authored = Vec::new();
    for id in 0..world.objects.world.colliders.len() {
        let Some(Actor::Powerup(original)) = world.objects.components.actors.get(ObstacleId(id))
        else {
            continue;
        };
        let original_pose = world.objects.world.colliders[id].pose;
        let entity = world.objects.world.colliders[id].entity.0;
        authored.push(json!({"object":id,"kind":original.kind,"position":original_pose.position,
            "phase":original.phase,"regeneration":original.regeneration,"fall_range":original.fall_range}));
        for variant in 0..10 {
            let mut p = clone_pickup(original);
            let mut body = original_pose;
            let mut flags = 0u32;
            let mut c = [
                2,
                33,
                entity as i32,
                entity as i32,
                body.position[0],
                body.position[1],
                body.position[2],
                body.position[0] >> 3,
                body.position[1] >> 3,
                body.position[2] >> 3,
                255,
                0,
                geometry.separation,
                geometry.diameter as i32,
            ];
            match variant {
                0 => {}
                1 => {
                    c[4] = body.position[0].wrapping_add(p.fall_range);
                }
                2 => {
                    p.flight.disabled = true;
                    c[11] = 1;
                }
                3 => {
                    p.regeneration = 0;
                    p.flight.delay = 34;
                }
                4 => {
                    flags = 1;
                    body.position[1] = p
                        .ground
                        .wrapping_add(geometry.diameter as i32)
                        .wrapping_add(1);
                }
                5 => {
                    flags = 5;
                    p.flight.normal = [0, 4096, 0];
                }
                6 => {
                    flags = 5;
                    p.flight.normal = [0, 4096, 0];
                    p.flight.parachute = Some(body);
                    p.flight.blink_count = 10;
                    p.flight.blink_time = 30;
                }
                7 => {
                    flags = 13;
                    p.flight.parachute = Some(body);
                }
                8 => {
                    flags = 13;
                    c[7] = c[7].wrapping_add(255 * 128);
                }
                _ => {
                    flags = 0x80000005;
                    p.flight.parachute = Some(body);
                    p.flight.normal = [-2000, 32767, 4096];
                    c[1] = -33;
                }
            }
            let controls = (0..8)
                .map(|i| {
                    let mut v = c;
                    v[2] = v[2].wrapping_add(i);
                    v
                })
                .collect();
            cases.push(case(
                format!("authored-{id}-{variant}"),
                p,
                body,
                flags,
                controls,
                terrain,
            ));
        }
        // Byte/count wrap and same-frame finish run independently of movement.
        for count in [9, 10, 11, u32::MAX] {
            for timer in [30, i32::MAX] {
                let mut p = clone_pickup(original);
                p.flight.blink_count = count;
                p.flight.blink_time = timer;
                let c = [
                    1,
                    1,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    geometry.separation,
                    geometry.diameter as i32,
                ];
                cases.push(case(
                    format!("blink-{id}-{count}-{timer}"),
                    p,
                    original_pose,
                    0x80000004,
                    vec![c, c],
                    terrain,
                ));
            }
        }
    }
    let amounts = *world.objects.components.actors.pickup_amounts();
    for weapon in [0, 1, 7, 8, 9, 10, u32::MAX] {
        for ammo in 0..=255 {
            let mut state = inventory::State { weapon, ammo };
            let mut cooldown = 1000;
            let controls = [weapon, 9, 7, 11, u32::MAX];
            let mut input = vec![
                1,
                controls.len() as i32,
                weapon as i32,
                i32::from(ammo),
                cooldown,
            ];
            input.extend(amounts.map(i32::from));
            let mut frames = Vec::new();
            for kind in controls {
                input.extend([kind as i32, 1]);
                let status = state.collect(kind, &amounts).map_or_else(|e| e, |()| 0);
                if status == 0 {
                    state.collected(&mut cooldown);
                }
                frames.push(vec![
                    status,
                    state.weapon as i32,
                    i32::from(state.ammo),
                    cooldown,
                ]);
            }
            cases.push(
                json!({"tag":format!("inventory-{weapon}-{ammo}"),"input":input,"frames":frames}),
            );
        }
    }
    let mut cells = Vec::new();
    for cell in 0..64 {
        cells.extend(pages::cell_bytes(terrain, cell));
    }
    fs::write(out.join("powerup-cells.bin"), cells).unwrap();
    fs::write(out.join("powerup.json"),serde_json::to_vec(&json!({"scope":"Original authored ENGLAND pickup phase/finish and weapon inventory, complete pose/flight state and committed errors; scene actions and collection effects are separate","authored":authored,"cases":cases})).unwrap()).unwrap();
}
