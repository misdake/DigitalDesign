//! Direct first-map Player bindings and canonical route/racer fields.
use tankracer_game::{
    scene_objects::rcc_lifecycle_probe, symbols::Binding, vehicle_runtime::SinglePlayer,
};

fn ints(out: &mut Vec<u16>, values: impl IntoIterator<Item = i32>) {
    for v in values {
        out.extend([v as u16, (v >> 16) as u16]);
    }
}
fn section(out: &mut Vec<u16>, index: usize, data: &[u16]) {
    out[8 + index * 2] = u16::try_from(out.len()).unwrap();
    out[9 + index * 2] = u16::try_from(data.len()).unwrap();
    out.extend(data);
}
pub fn fields(world: &SinglePlayer) -> Vec<u16> {
    let mut out = Vec::new();
    for car in world.vehicles.entries[..2].iter().flatten() {
        let r = car.route;
        ints(
            &mut out,
            [
                i32::from(r.checkpoint.is_some()),
                r.checkpoint.map_or(0, |v| v.0 as i32),
                r.passed as i32,
                r.completed,
                r.records[0],
                r.records[1],
                r.records[2],
                r.score,
                r.previous_score,
                i32::from(r.wrong_way),
                i32::from(r.warning_timer),
                car.race.elapsed,
                i32::from(car.race.laps),
                i32::from(car.race.driving),
                car.weather as i32,
            ],
        );
    }
    assert_eq!(out.len(), 60);
    out
}
pub fn capture(world: &SinglePlayer) -> Vec<u16> {
    let mut out = vec![0u16; 2];
    out.extend(fields(world));
    for car in world.vehicles.entries[..2].iter().flatten() {
        ints(&mut out, [car.inventory.weapon as i32]);
        out.extend([u16::from(car.inventory.ammo), 0]);
    }
    assert_eq!(out.len(), 70);
    super::projectile_snapshot::flight(&mut out, &world.objects.components.projectiles.flight);
    assert_eq!(out.len(), 124);
    for car in world.vehicles.entries[..2].iter().flatten() {
        out.extend([
            u16::from(car.horn_down),
            u16::from(car.horn_latched),
            car.horn_timer as u16,
        ]);
        let revision = car.model.parts[tankracer_assets::config::Part::Gun.slot()]
            .as_ref()
            .map_or(0, |gun| gun.revision());
        for shift in [0, 16, 32, 48] {
            out.push((revision >> shift) as u16);
        }
    }
    assert_eq!(out.len(), 138);
    let mut projectiles: Vec<_> = world.objects.components.projectiles.entries().collect();
    projectiles.sort_by_key(|(id, _)| id.0);
    for (id, state) in projectiles {
        out[1] += 1;
        out.push(u16::try_from(id.0).unwrap());
        out.extend(super::projectile_snapshot::metadata(world, id.0));
        super::projectile_snapshot::state(&mut out, state);
    }
    assert_eq!(out.len(), 138 + usize::from(out[1]) * 641);
    out[0] = u16::try_from(out.len()).unwrap();
    out
}
pub fn clock(clock: &tankracer_game::race_clock::Clock) -> Vec<u16> {
    let mut out = Vec::new();
    ints(&mut out, [clock.countdown, clock.release, clock.display]);
    out
}
pub fn context(world: &SinglePlayer) -> Vec<u16> {
    let mut out = vec![0u16; 56];
    let c = &world.objects.components;
    let r = &c.waypoints;
    out[..8].copy_from_slice(&[
        0x5053,
        1,
        world.objects.world.objects.len() as u16,
        r.order.len() as u16,
        world.registry.world.0 as u16,
        c.projectiles.class.unwrap().0 as u16,
        c.mines.class.unwrap().0 as u16,
        0,
    ]);
    let mut data = Vec::new();
    ints(
        &mut data,
        [
            r.origin[0],
            r.origin[1],
            r.shifts[0] as i32,
            r.shifts[1] as i32,
            r.radius[0],
            r.radius[1],
            r.backwards_index,
            i32::from(r.fallback.is_some()),
            r.fallback.map_or(0, |v| v.0 as i32),
        ],
    );
    section(&mut out, 0, &data);
    data.clear();
    for p in &r.points {
        let mut row = [0; 7];
        if let Some(p) = p {
            let (kind, next) = match p.next[0] {
                Binding::Direct(None) => (0, 0),
                Binding::Direct(Some(v)) => (1, v.0 as i32),
                Binding::Named(_) => (2, 0),
            };
            row = [
                1,
                p.progress,
                kind,
                next,
                i32::from(p.lap),
                i32::from(p.hairpin),
                p.width,
            ];
        }
        ints(&mut data, row);
    }
    section(&mut out, 1, &data);
    let mut ranges = Vec::new();
    let mut items = Vec::new();
    for cell in &r.cells {
        ranges.extend([(items.len() / 2) as u16, cell.len() as u16]);
        ints(&mut items, cell.iter().map(|v| v.0 as i32));
    }
    section(&mut out, 2, &ranges);
    section(&mut out, 3, &items);
    section(
        &mut out,
        4,
        &r.weather
            .iter()
            .flatten()
            .map(|&v| u16::from(v))
            .collect::<Vec<_>>(),
    );
    section(
        &mut out,
        5,
        &world
            .surfaces()
            .entries
            .iter()
            .map(|s| s.slide[0].multiplier)
            .collect::<Vec<_>>(),
    );
    section(&mut out, 6, &rcc_lifecycle_probe::lap_bindings(world));
    section(&mut out, 7, &rcc_lifecycle_probe::lap_backups(world));
    let p = c.projectiles.resources.unwrap();
    data.clear();
    ints(&mut data, p.tuning.gravity);
    ints(&mut data, p.tuning.speed);
    ints(&mut data, p.tuning.elevation);
    ints(&mut data, p.tuning.snap);
    ints(
        &mut data,
        [
            p.tuning.homing_speed,
            p.tuning.seek_time,
            p.tuning.max_steer,
        ],
    );
    ints(&mut data, p.main.map(|v| v as i32));
    ints(&mut data, p.trail.map(|v| v as i32));
    ints(&mut data, p.homing_tail);
    data.push(p.cup as u16);
    assert_eq!(data.len(), 61);
    section(&mut out, 8, &data);
    out[40] = p.homing_mesh.index() as u16;
    out[41] = (p.homing_mesh.index() >> 16) as u16;
    let m = c.mines.resources.unwrap();
    data.clear();
    ints(&mut data, m.mesh.map(|v| v.index() as i32));
    data.extend(m.radius);
    data.extend(m.bounds.into_iter().flatten().flatten().map(|v| v as u16));
    assert_eq!(data.len(), 18);
    section(&mut out, 9, &data);
    data.clear();
    super::projectile_snapshot::flight(&mut data, &c.projectiles.flight);
    assert_eq!(data.len(), 54);
    section(&mut out, 10, &data);
    assert!(c.projectiles.entries().next().is_none());
    assert!(c.mines.entries().is_empty());
    data.clear();
    ints(&mut data, world.camera_position);
    section(&mut out, 11, &data);
    data.clear();
    for car in world.vehicles.entries[..2].iter().flatten() {
        ints(&mut data, car.model.query_bounds[0].min);
        ints(&mut data, car.model.query_bounds[0].max);
        ints(
            &mut data,
            [car.model.parts[tankracer_assets::config::Part::Body.slot()]
                .as_ref()
                .map_or(0, |m| i32::from(m.radius) * 16)],
        );
    }
    section(&mut out, 12, &data);
    let e = &c.explosion_world;
    data.clear();
    ints(
        &mut data,
        [
            e.count,
            i32::from(e.overflow),
            i32::from(e.fog),
            i32::from(e.flash_timer),
            i32::from(e.flash_active),
        ],
    );
    for slot in e.slots {
        ints(
            &mut data,
            [
                i32::from(slot.entity.is_some()),
                slot.entity.map_or(0, |v| v.0 as i32),
                slot.frame as i32,
            ],
        );
    }
    assert_eq!(data.len(), 100);
    section(&mut out, 13, &data);
    let b = &c.blasts;
    data.clear();
    ints(&mut data, b.times.into_iter().flatten());
    ints(&mut data, b.next.map(|v| v as i32));
    ints(&mut data, b.count.map(|v| v as i32));
    ints(&mut data, [b.total as i32]);
    for row in b.bands {
        ints(&mut data, [row.inner, row.outer]);
        ints(&mut data, row.origin);
        ints(
            &mut data,
            [
                i32::from(row.owner.is_some()),
                row.owner.map_or(0, |v| v.0 as i32),
                i32::from(row.flipped),
            ],
        );
        ints(&mut data, row.handles.map(|v| v.0 as i32));
    }
    for row in b.rings {
        ints(&mut data, [row.life]);
        ints(&mut data, row.origin);
        ints(
            &mut data,
            [
                i32::from(row.owner.is_some()),
                row.owner.map_or(0, |v| v.0 as i32),
                i32::from(row.kind),
                i32::from(row.alpha),
                i32::from(row.rotation),
            ],
        );
        for ray in row.rays {
            ints(
                &mut data,
                [
                    ray.handle.0 as i32,
                    ray.radius,
                    ray.width,
                    ray.zero,
                    ray.angle as i32,
                ],
            );
        }
    }
    assert_eq!(data.len(), 374);
    section(&mut out, 14, &data);
    out[42] = u16::try_from(out.len()).unwrap();
    out
}

pub fn prepared(
    root: &std::path::Path,
) -> (
    tankracer_game::session::Session,
    tankracer_game::scenery::Loaded,
) {
    let (mut session, mut scenery) = super::reference_world::constructed(root)
        .into_session()
        .unwrap();
    let read = |name: &str| {
        std::fs::read(root.join("assets/game").join(name).join("FASTFILE.DAT")).unwrap()
    };
    tankracer_game::scenery::rcc_player_resources::prepare(
        session.world_mut_at_load().unwrap(),
        &mut scenery,
        &read("DATA"),
        &read("LEVELS/ENGLAND"),
        &read("INGAME"),
        0,
    )
    .unwrap();
    (session, scenery)
}
pub fn ready(root: &std::path::Path) -> tankracer_game::session::Session {
    prepared(root).0
}
