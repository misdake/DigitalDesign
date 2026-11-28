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
        out.extend(car_fields(car));
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
    out.push(u16::from(world.objects.components.overlay.is_some()));
    out.extend(
        world
            .objects
            .components
            .overlay
            .unwrap_or([0; 4])
            .map(u16::from),
    );
    assert_eq!(out.len(), 143);
    let marks = &world.objects.components.projectiles.marks;
    ints(
        &mut out,
        [marks.count as i32, marks.head as i32, marks.next as i32],
    );
    ints(&mut out, marks.handles.map(|v| v.0 as i32));
    out.extend(explosion_world(world));
    out.extend(blasts(world));
    let impulse = world.objects.components.camera_impulse;
    ints(
        &mut out,
        [
            i32::from(impulse.is_some()),
            impulse.map_or(0, |v| v.0 as i32),
            impulse.map_or(0, |v| v.1),
        ],
    );
    assert_eq!(out.len(), 677);
    for car in world.vehicles.entries[..2].iter().flatten() {
        out.extend(car.hit_counts.into_iter().flatten().map(u16::from));
        for part in [
            tankracer_assets::config::Part::Body,
            tankracer_assets::config::Part::Turret,
        ] {
            let revision = car.model.parts[part.slot()]
                .as_ref()
                .map_or(0, |m| m.revision());
            for shift in [0, 16, 32, 48] {
                out.push((revision >> shift) as u16);
            }
        }
        let gain = car.model.config.tank.drive.sidegrip_fraction;
        ints(
            &mut out,
            [i32::from(gain.multiplier), i32::from(gain.shift)],
        );
    }
    assert_eq!(out.len(), 725);
    for car in world.vehicles.entries[..2].iter().flatten() {
        if let Some(part) = &car.model.parts[tankracer_assets::config::Part::Turret.slot()] {
            let geometry = part.geometry();
            out.extend(geometry.vertices.iter().flatten().map(|v| *v as u16));
            out.extend(
                geometry
                    .triangles
                    .iter()
                    .flat_map(|f| f.normal.map(|v| v as u16)),
            );
        }
    }
    let mines = &world.objects.components.mines;
    ints(&mut out, [mines.count as i32]);
    out.extend([u16::try_from(mines.order.len()).unwrap(), 0]);
    assert!(mines.order.len() <= 32);
    for i in 0..32 {
        ints(&mut out, [mines.order.get(i).map_or(0, |id| id.0 as i32)]);
    }
    let occupied = &world.objects.components.ufo_occupied;
    let visible = &world.vehicles.visible;
    assert!(occupied.len() <= 32 && visible.len() <= 9);
    out.extend([occupied.len() as u16, visible.len() as u16, 0, 0]);
    for i in 0..32 {
        ints(&mut out, [occupied.get(i).map_or(0, |v| v.0 as i32)]);
    }
    for i in 0..9 {
        ints(&mut out, [visible.get(i).map_or(0, |v| v.0 as i32)]);
    }
    for id in super::projectile_snapshot::EMITTER_IDS {
        let Some(tankracer_game::actor_startup::Actor::Emitter(e)) = world
            .objects
            .components
            .actors
            .get(tankracer_game::object_query::ObstacleId(id))
        else {
            panic!("first-map emitter owner")
        };
        super::projectile_snapshot::emitter(&mut out, e);
    }
    for id in super::projectile_snapshot::TRAIN_IDS {
        let Some(tankracer_game::actor_startup::Actor::Train(t)) = world
            .objects
            .components
            .actors
            .get(tankracer_game::object_query::ObstacleId(id))
        else {
            panic!("first-map Train owner")
        };
        super::projectile_snapshot::train(&mut out, t);
    }
    let Some(tankracer_game::actor_startup::Actor::Noisy { selected, .. }) = world
        .objects
        .components
        .actors
        .get(tankracer_game::object_query::ObstacleId(0))
    else {
        panic!("first-map Noisy owner")
    };
    ints(&mut out, [*selected]);
    out.extend([
        u16::from(world.objects.components.actors.smoke_sequence.0),
        0,
    ]);
    let Some(tankracer_game::actor_startup::Actor::Smoker(s)) = world
        .objects
        .components
        .actors
        .get(tankracer_game::object_query::ObstacleId(1))
    else {
        panic!("first-map Smoker owner")
    };
    super::projectile_snapshot::smoker(&mut out, s);
    capture_flocks(&mut out, world);
    let mut weapons = Vec::new();
    for (id, state) in world.objects.components.projectiles.entries() {
        let mut payload = Vec::new();
        super::projectile_snapshot::state(&mut payload, state);
        weapons.push((id.0, 1, payload));
    }
    for (id, state) in mines.entries() {
        let mut payload = Vec::new();
        super::projectile_snapshot::mine(&mut payload, state);
        weapons.push((id.0, 2, payload));
    }
    for (id, state) in world.objects.components.explosions.iter().enumerate() {
        if let Some(state) = state {
            let mut payload = Vec::new();
            super::projectile_snapshot::explosion(&mut payload, state);
            weapons.push((id, 3, payload));
        }
    }
    for (id, state) in world.objects.components.ufos.iter().enumerate() {
        if let Some(state) = state {
            let mut payload = Vec::new();
            super::projectile_snapshot::ufo(&mut payload, state);
            weapons.push((id, 4, payload));
        }
    }
    weapons.sort_by_key(|v| v.0);
    out[1] = u16::try_from(weapons.len()).unwrap();
    for (id, kind, payload) in weapons {
        out.extend([u16::try_from(id).unwrap(), kind]);
        out.extend(super::projectile_snapshot::metadata(world, id));
        out.extend(payload);
    }
    out[0] = u16::try_from(out.len()).unwrap();
    out
}
pub fn clock(clock: &tankracer_game::race_clock::Clock) -> Vec<u16> {
    let mut out = Vec::new();
    ints(&mut out, [clock.countdown, clock.release, clock.display]);
    out
}
pub fn flock_initial(world: &SinglePlayer) -> Vec<u16> {
    let mut out = vec![0u16; 16];
    out[..9].copy_from_slice(&[0x464c, 1, 3, 2, 288, 24, 348, 1060, 1188]);
    for id in super::projectile_snapshot::FLOCK_IDS {
        let Some(tankracer_game::actor_startup::Actor::Flock(f)) = world
            .objects
            .components
            .actors
            .get(tankracer_game::object_query::ObstacleId(id))
        else {
            panic!("first-map Flock owner")
        };
        assert_eq!(f.models.len(), 2);
        assert_eq!(f.meshes.len(), 2);
        out.extend([id as u16, 2]);
        ints(&mut out, [f.range]);
        super::projectile_snapshot::flock(&mut out, f);
        for (i, m) in f.models.iter().enumerate() {
            assert_eq!(m.geometry.vertices.len(), 8);
            out.extend([
                f.meshes[i].index() as u16,
                f.extrema[i][0] as u16,
                f.extrema[i][1] as u16,
                8,
            ]);
            out.extend(m.geometry.vertices.iter().flatten().map(|&v| v as u16));
        }
    }
    assert_eq!(out.len(), 1060);
    out
}
fn capture_flocks(out: &mut Vec<u16>, world: &SinglePlayer) {
    use tankracer_game::scene_meshes::rcc_probe;
    let start = out.len();
    for id in super::projectile_snapshot::FLOCK_IDS {
        let Some(tankracer_game::actor_startup::Actor::Flock(f)) = world
            .objects
            .components
            .actors
            .get(tankracer_game::object_query::ObstacleId(id))
        else {
            panic!("first-map Flock owner")
        };
        super::projectile_snapshot::flock(out, f);
        for m in &f.models {
            assert_eq!(m.geometry.vertices.len(), 8);
            out.extend(m.geometry.vertices.iter().flatten().map(|&v| v as u16));
        }
        for &id in &f.meshes {
            let meshes = &world.objects.components.surfaces.meshes;
            assert!(id.index() < meshes.len());
            let (g, _, _, v) = rcc_probe::inspect(meshes, id);
            out.extend([
                1,
                u16::from(g.is_some()),
                v as u16,
                (v >> 16) as u16,
                (v >> 32) as u16,
                (v >> 48) as u16,
            ]);
            if let Some(g) = g {
                assert_eq!(g.vertices.len(), 8);
                out.extend(g.vertices.iter().flatten().map(|&v| v as u16));
            } else {
                out.extend([0; 24]);
            }
        }
    }
    assert_eq!(out.len() - start, 1188);
}
pub fn context(world: &SinglePlayer) -> Vec<u16> {
    let mut out = vec![0u16; 60];
    let c = &world.objects.components;
    let r = &c.waypoints;
    out[..8].copy_from_slice(&[
        0x5053,
        5,
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
    data = explosion_world(world);
    section(&mut out, 13, &data);
    data = blasts(world);
    section(&mut out, 14, &data);
    data.clear();
    ints(&mut data, [c.explosion_class.unwrap().0 as i32]);
    let e = c.explosion_resources;
    ints(
        &mut data,
        [
            e.main as i32,
            e.smoke as i32,
            i32::from(e.small),
            e.damping as i32,
            i32::from(e.map_fog),
        ],
    );
    ints(&mut data, world.camera_basis.0);
    let m = c.blasts.materials;
    ints(
        &mut data,
        [i32::from(m.is_some()), m.map_or(0, |v| v.band as i32)],
    );
    ints(&mut data, m.map_or([0; 3], |v| v.ring).map(|v| v as i32));
    ints(&mut data, [p.crater as i32, c.stamp_units]);
    assert_eq!(data.len(), 44);
    let hit = c.projectiles.shell_hit;
    for gain in [
        hit.player_turn,
        hit.ai_turn[0],
        hit.ai_turn[1],
        hit.ai_turn[2],
        hit.player_speed,
        hit.ai_speed,
        hit.player_push,
        hit.ai_push,
    ] {
        data.extend([gain.multiplier as u16, gain.shift as u16]);
    }
    data.push(hit.slow_after);
    let hit = c.projectiles.homer_hit;
    for gain in [hit.player_turn, hit.ai_turn, hit.player_speed, hit.ai_speed] {
        data.extend([gain.multiplier as u16, gain.shift as u16]);
    }
    assert_eq!(data.len(), 69);
    out[45] = 725;
    for (i, car) in world.vehicles.entries[..2].iter().flatten().enumerate() {
        out[43 + i] = u16::try_from(data.len()).unwrap();
        let body = car.model.parts[tankracer_assets::config::Part::Body.slot()].as_ref();
        let turret = car.model.parts[tankracer_assets::config::Part::Turret.slot()].as_ref();
        let (vertices, faces) = turret.map_or((0, 0), |m| {
            let g = m.geometry();
            (g.vertices.len(), g.triangles.len())
        });
        data.extend([
            u16::from(body.is_some()),
            u16::from(body.is_some_and(|m| m.is_private())),
            u16::from(turret.is_some()),
            u16::from(turret.is_some_and(|m| m.is_private())),
        ]);
        data.extend(car.model.damage_vertices[1].map(u16::from));
        data.extend([
            vertices as u16,
            faces as u16,
            body.map_or(0, |m| m.source.bounds[1][2]) as u16,
        ]);
        if let Some(turret) = turret {
            data.extend(
                turret
                    .geometry()
                    .triangles
                    .iter()
                    .flat_map(|f| f.indices.map(u16::from)),
            );
        }
        out[45] += u16::try_from((vertices + faces) * 3).unwrap();
    }
    out[45] += 68;
    out[53] = data.len() as u16;
    let (class, resources) = rcc_lifecycle_probe::ufo_config(world);
    let resources = resources.unwrap();
    out[46] = class.unwrap().0 as u16;
    out[47] = 1;
    out[48] = data.len() as u16;
    ints(
        &mut data,
        [
            resources.mesh.index() as i32,
            resources.material as i32,
            resources.height,
        ],
    );
    out[49] = data.len() as u16;
    out[51] = r.points.len() as u16;
    assert!(r.points.len() <= 240 && r.order.len() <= 128);
    for p in &r.points {
        ints(
            &mut data,
            [i32::from(p.is_some()), p.as_ref().map_or(0, |p| p.progress)],
        );
    }
    out[50] = data.len() as u16;
    out[52] = r.order.len() as u16;
    ints(&mut data, r.order.iter().map(|v| v.0 as i32));
    out[45] += 86 + 250;
    out[54] = data.len() as u16;
    out[55] = super::projectile_snapshot::EMITTER_IDS.len() as u16;
    for id in super::projectile_snapshot::EMITTER_IDS {
        data.push(id as u16);
        let Some(tankracer_game::actor_startup::Actor::Emitter(e)) =
            c.actors.get(tankracer_game::object_query::ObstacleId(id))
        else {
            panic!("first-map emitter owner")
        };
        super::projectile_snapshot::emitter(&mut data, e);
    }
    out[7] = data.len() as u16;
    out[45] += 120;
    for id in super::projectile_snapshot::TRAIN_IDS {
        let Some(tankracer_game::actor_startup::Actor::Train(t)) =
            c.actors.get(tankracer_game::object_query::ObstacleId(id))
        else {
            panic!("first-map Train owner")
        };
        data.extend([id as u16, u16::from(t.sound.is_some())]);
        super::projectile_snapshot::train(&mut data, t);
    }
    out[56] = data.len() as u16;
    out[58] = 1;
    out[59] = 0;
    let Some(tankracer_game::actor_startup::Actor::Noisy { sounds, selected }) =
        c.actors.get(tankracer_game::object_query::ObstacleId(0))
    else {
        panic!("first-map Noisy owner")
    };
    data.extend([
        0x414d,
        1,
        sounds.len() as u16,
        u16::from(c.actors.smoke_sequence.0),
    ]);
    ints(&mut data, [*selected]);
    for sound in sounds {
        ints(&mut data, sound.values);
        ints(&mut data, [sound.sample as i32]);
    }
    let Some(tankracer_game::actor_startup::Actor::Smoker(s)) =
        c.actors.get(tankracer_game::object_query::ObstacleId(1))
    else {
        panic!("first-map Smoker owner")
    };
    super::projectile_snapshot::smoker(&mut data, s);
    out[57] = data.len() as u16 - out[56];
    out[45] += 430 + 1188;
    section(&mut out, 15, &data);
    out[42] = u16::try_from(out.len()).unwrap();
    out
}

pub fn prepared(
    root: &std::path::Path,
) -> (
    tankracer_game::session::Session,
    tankracer_game::scenery::Loaded,
) {
    prepare(root, super::reference_world::constructed(root))
}
pub fn prepare(
    root: &std::path::Path,
    constructed: tankracer_game::world_startup::Constructed,
) -> (
    tankracer_game::session::Session,
    tankracer_game::scenery::Loaded,
) {
    let (mut session, mut scenery) = constructed.into_session().unwrap();
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

fn explosion_world(world: &SinglePlayer) -> Vec<u16> {
    let mut out = Vec::new();
    let e = &world.objects.components.explosion_world;
    ints(
        &mut out,
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
            &mut out,
            [
                i32::from(slot.entity.is_some()),
                slot.entity.map_or(0, |v| v.0 as i32),
                slot.frame as i32,
            ],
        );
    }
    assert_eq!(out.len(), 100);
    out
}

fn blasts(world: &SinglePlayer) -> Vec<u16> {
    let mut out = Vec::new();
    let b = &world.objects.components.blasts;
    ints(&mut out, b.times.into_iter().flatten());
    ints(&mut out, b.next.map(|v| v as i32));
    ints(&mut out, b.count.map(|v| v as i32));
    ints(&mut out, [b.total as i32]);
    for row in b.bands {
        ints(&mut out, [row.inner, row.outer]);
        ints(&mut out, row.origin);
        ints(
            &mut out,
            [
                i32::from(row.owner.is_some()),
                row.owner.map_or(0, |v| v.0 as i32),
                i32::from(row.flipped),
            ],
        );
        ints(&mut out, row.handles.map(|v| v.0 as i32));
    }
    for row in b.rings {
        ints(&mut out, [row.life]);
        ints(&mut out, row.origin);
        ints(
            &mut out,
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
                &mut out,
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
    assert_eq!(out.len(), 374);
    out
}

pub fn car_fields(car: &tankracer_game::vehicle_startup::Player) -> Vec<u16> {
    let mut out = Vec::new();
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
    out
}
