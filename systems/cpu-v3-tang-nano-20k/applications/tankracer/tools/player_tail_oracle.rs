//! Original post-physics Player body with independent geometry/result inputs.
use serde_json::json;
use std::{fs, path::Path};
use tankracer_assets::config::Part;
use tankracer_game::{
    player_input::Keys,
    vehicle_runtime::{SinglePlayer, rcc_player_probe as probe},
};
fn fields(world: &SinglePlayer) -> Vec<u16> {
    let car = world.vehicles.entries[0].as_ref().unwrap();
    let mut out = vec![
        u16::from(car.horn_down),
        u16::from(car.horn_latched),
        car.horn_timer as u16,
    ];
    let revision = car.model.parts[Part::Gun.slot()]
        .as_ref()
        .map_or(0, |g| g.revision());
    for shift in [0, 16, 32, 48] {
        out.push((revision >> shift) as u16);
    }
    out
}
fn gun(world: &SinglePlayer) -> Vec<u16> {
    world.vehicles.entries[0].as_ref().unwrap().model.parts[Part::Gun.slot()]
        .as_ref()
        .map_or(Vec::new(), |g| {
            g.geometry()
                .vertices
                .iter()
                .flatten()
                .map(|x| *x as u16)
                .collect()
        })
}
fn before(values: Option<[Option<i16>; 4]>) -> Vec<u16> {
    let mut out = vec![0u16; 5];
    if let Some(values) = values {
        out[0] = 16;
        for (i, value) in values.into_iter().enumerate() {
            if let Some(value) = value {
                out[0] |= 1 << i;
                out[1 + i] = value as u16;
            }
        }
    }
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..15 {
        let mut world = super::reference_world::race(root);
        world.vehicles.racers[2..].fill(None);
        if branch == 13 {
            world.vehicles.entries.push(None);
        }
        if branch == 13 || branch == 14 {
            world.vehicles.racers[2] =
                Some(tankracer_game::object_query::TankId(if branch == 13 {
                    1
                } else {
                    0
                }));
        }
        let car = world.vehicles.entries[0].as_mut().unwrap();
        car.visible = true;
        if branch == 2 || branch == 7 || branch == 12 {
            car.horn_down = true;
            car.horn_latched = true;
            car.horn_timer = 34;
        } else if branch == 3 {
            car.horn_timer = -17;
        } else if branch == 8 {
            car.horn_down = true;
            car.horn_latched = true;
            car.horn_timer = 70;
        } else if branch == 13 || branch == 14 {
            car.horn_timer = 1;
        }
        let revision = match branch {
            4 => 65535,
            5 => u64::from(u32::MAX),
            6 => u64::MAX,
            _ => 9,
        };
        car.model.parts[Part::Gun.slot()]
            .as_mut()
            .unwrap()
            .rcc_set_revision(revision);
        if branch == 9 {
            car.model.parts[Part::Gun.slot()] = None;
        }
        if branch == 10 {
            car.model.gun_vertices[3] = u8::MAX;
        }
        let original_gun = gun(&world);
        let car = world.vehicles.entries[0].as_ref().unwrap();
        let present = car.model.parts[Part::Gun.slot()].is_some();
        let mut binding = vec![u16::from(present), (original_gun.len() / 3) as u16];
        binding.extend(car.model.gun_vertices.map(u16::from));
        let initial = fields(&world);
        let mut frames = Vec::new();
        for frame in 0..3 {
            let prior = probe::gun_z(&world);
            let indices = world.vehicles.entries[0]
                .as_ref()
                .unwrap()
                .model
                .gun_vertices;
            let mut mutation = None;
            if matches!(branch, 4..=7 | 10) {
                mutation = Some(usize::from(indices[0]));
            }
            if branch == 11 {
                mutation = (0..original_gun.len() / 3).find(|i| !indices.contains(&(*i as u8)));
                assert!(mutation.is_some());
            }
            if let Some(vertex) = mutation {
                let g = world.vehicles.entries[0].as_mut().unwrap().model.parts[Part::Gun.slot()]
                    .as_mut()
                    .unwrap();
                let version = g.revision();
                g.edit(|geometry| {
                    geometry.vertices[vertex][2] = geometry.vertices[vertex][2].wrapping_add(1)
                })
                .unwrap();
                g.rcc_set_revision(version);
            }
            let post_gun = gun(&world);
            let success = branch != 7 && branch != 12;
            let keys_present = branch != 8;
            let down = if branch == 2 {
                frame != 1
            } else {
                branch == 1 && frame != 2
            };
            let keys = keys_present.then_some((
                Keys {
                    horn: down,
                    ..Default::default()
                },
                4,
            ));
            let status = probe::update(
                &mut world,
                keys,
                33,
                frame + 1,
                100000,
                &mut false,
                &mut super::driving_devices::Devices { models: &[] },
                probe::Stage::AfterPhysics {
                    before: prior,
                    result: if success { Ok(()) } else { Err(-77) },
                },
            );
            assert_eq!(
                status,
                if branch == 13 {
                    Err(-1)
                } else if success {
                    Ok(())
                } else {
                    Err(-77)
                }
            );
            frames.push(
                json!({"dt":33,"flags":3 | (u16::from(keys_present)<<2) | (u16::from(down)<<3),
                "success":success,"peer_ok":branch!=13,"status":status.err().unwrap_or(0),
                "gun":post_gun,"before":before(prior),"fields":fields(&world)}),
            );
        }
        cases.push(json!({"tag":format!("tail-{branch}"),"gun":original_gun,"binding":binding,"fields":initial,"frames":frames}));
    }
    fs::write(out.join("player-tail.json"), serde_json::to_vec(&json!({"cases":cases,
        "scope":"Original post-physics Player body: gun option/index snapshots, u64 revision carries, unchanged/untracked vertices, failed-physics publication before error, horn press/hold/release/expiry and absent keys. Peer AI notifications are excluded."})).unwrap()).unwrap();
}
