//! Original ENGLAND gate/progress queries and player route/race-clock updates.
use super::{identity_assets, ints, scene_export};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    math::Matrix, object_query::EntityId, player_route, race_clock, symbols::Binding, waypoint,
};
fn snapshot(
    s: &player_route::State,
    racers: &[race_clock::Racer; 3],
    clock: &race_clock::Clock,
    result: [i32; 2],
    dt: i32,
) -> Vec<u16> {
    let mut out = Vec::new();
    ints(
        &mut out,
        [
            i32::from(s.checkpoint.is_some()),
            s.checkpoint.map_or(0, |id| id.0 as i32),
            s.passed as i32,
            s.completed,
            s.records[0],
            s.records[1],
            s.records[2],
            s.score,
            s.previous_score,
            i32::from(s.wrong_way),
            i32::from(s.warning_timer),
        ],
    );
    for r in racers {
        ints(
            &mut out,
            [r.elapsed, i32::from(r.laps), i32::from(r.driving)],
        );
    }
    ints(
        &mut out,
        [
            clock.countdown,
            clock.release,
            clock.display,
            result[0],
            result[1],
            dt,
        ],
    );
    assert_eq!(out.len(), 52);
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..92 {
        let mut world = super::reference_world::full_race(root);
        let order = world.objects.components.waypoints.order.clone();
        assert_eq!(order.len(), 36);
        let current = order[branch % order.len()];
        let entity = world.objects.world.colliders[current.0].entity;
        let Binding::Direct(Some(target)) = world
            .objects
            .components
            .waypoints
            .point(current)
            .unwrap()
            .next[0]
        else {
            panic!("linked route")
        };
        let target_id = world
            .identity
            .entities
            .record(target)
            .unwrap()
            .object
            .unwrap()
            .obstacle()
            .unwrap();
        let collider = &world.objects.world.colliders[target_id.0];
        let offset = collider
            .external_basis
            .map_or([0, 0, 1024], |m| m.local_to_world([0, 0, 1024]));
        let mut position =
            std::array::from_fn::<_, 3, _>(|i| collider.pose.position[i].wrapping_add(offset[i]));
        let mut state = player_route::State {
            checkpoint: Some(entity),
            passed: 123,
            records: [100, 200, 300],
            score: 100000,
            previous_score: 110000,
            ..Default::default()
        };
        let mut racers = [
            race_clock::Racer {
                elapsed: 12345,
                laps: 0,
                driving: true,
            },
            race_clock::Racer {
                elapsed: 23456,
                laps: 2,
                driving: false,
            },
            race_clock::Racer {
                elapsed: -100,
                laps: -1,
                driving: true,
            },
        ];
        let mut clock = race_clock::Clock::default();
        let mut result = [777, 888];
        let mut dt = 17;
        let mut limit = 100000u32;
        let mut named = None;
        if branch == 40 || branch == 61 {
            named = Some(
                tankracer_game::symbols::Symbols::default()
                    .intern("RCC_UNRESOLVED_ROUTE")
                    .unwrap(),
            );
        }
        match branch {
            36 => state.checkpoint = None,
            37 => state.checkpoint = Some(EntityId(0xfffffffb)),
            38 => world.objects.components.waypoints.points[current.0] = None,
            39 => {
                world.objects.components.waypoints.points[current.0]
                    .as_mut()
                    .unwrap()
                    .next[0] = Binding::Direct(None)
            }
            40 | 61 => {
                world.objects.components.waypoints.points[current.0]
                    .as_mut()
                    .unwrap()
                    .next[0] = Binding::Named(named.unwrap())
            }
            41 => {
                world.objects.components.waypoints.points[current.0]
                    .as_mut()
                    .unwrap()
                    .next[0] = Binding::Direct(Some(EntityId(0xfffffffb)))
            }
            42 => world.objects.components.waypoints.points[target_id.0] = None,
            43 => {
                world.objects.world.colliders[target_id.0].external_basis = None;
                position = world.objects.world.colliders[target_id.0].pose.position;
                position[2] = position[2].wrapping_add(1024);
            }
            44 | 45 | 46 | 47 => {
                world.objects.world.colliders[target_id.0].external_basis = Some(Matrix::IDENTITY);
                position = world.objects.world.colliders[target_id.0].pose.position;
                position[2] = position[2].wrapping_add(1024);
                if branch == 44 {
                    position[0] = position[0].wrapping_add(
                        world
                            .objects
                            .components
                            .waypoints
                            .point(target_id)
                            .unwrap()
                            .width
                            >> 1,
                    );
                }
                if branch == 45 {
                    position[2] = position[2].wrapping_sub(1024);
                }
                if branch == 46 {
                    position[1] = position[1].wrapping_add(51200);
                }
                if branch == 47 {
                    position[0] = position[0].wrapping_add(i32::MIN);
                }
            }
            48..=52 => {
                world.objects.components.waypoints.points[target_id.0]
                    .as_mut()
                    .unwrap()
                    .lap = 1;
                if branch == 49 {
                    state.completed = -1;
                    racers[0].laps = -1;
                }
                if branch == 50 {
                    state.completed = 3;
                    racers[0].laps = 3;
                }
                if branch == 51 {
                    state.completed = 0;
                    racers[0].laps = 2;
                }
                if branch == 52 {
                    state.completed = 0;
                    racers[0].laps = 32767;
                    state.passed = u32::MAX;
                }
            }
            53 => limit = 0,
            54 => limit = 1,
            55 => world
                .objects
                .components
                .waypoints
                .cells
                .iter_mut()
                .for_each(Vec::clear),
            56 => {
                for p in world
                    .objects
                    .components
                    .waypoints
                    .points
                    .iter_mut()
                    .flatten()
                {
                    p.progress = -1;
                }
            }
            57 => {
                for id in &order {
                    world.objects.world.colliders[id.0].external_basis = None;
                }
            }
            58 => {
                for p in world
                    .objects
                    .components
                    .waypoints
                    .points
                    .iter_mut()
                    .flatten()
                {
                    p.hairpin = 1;
                    p.width = 1;
                }
            }
            59 | 60 => {
                let routes = &mut world.objects.components.waypoints;
                routes.backwards_index = 5;
                routes.points[current.0].as_mut().unwrap().progress = 20;
                routes.points[target_id.0].as_mut().unwrap().progress = 2;
                routes.points[target_id.0].as_mut().unwrap().lap = 0;
                if branch == 59 {
                    routes.fallback = None;
                }
                for cell in &mut routes.cells {
                    *cell = vec![target_id];
                }
                let collider = &mut world.objects.world.colliders[target_id.0];
                collider.external_basis = Some(Matrix::IDENTITY);
                position = collider.pose.position;
                position[2] = position[2].wrapping_sub(1024);
            }
            62 => limit = u32::MAX,
            63 => {
                let other = order[(branch + 2) % order.len()];
                let pose = world.objects.world.colliders[target_id.0].pose;
                for id in [target_id, other] {
                    world.objects.world.colliders[id.0].pose = pose;
                    world.objects.world.colliders[id.0].external_basis = Some(Matrix::IDENTITY);
                }
                for cell in &mut world.objects.components.waypoints.cells {
                    *cell = vec![target_id, other];
                }
                position = pose.position;
                position[2] = position[2].wrapping_sub(1024);
            }
            64 => position = [i32::MIN, i32::MAX, i32::MIN],
            65 => position[1] = position[1].wrapping_add(i32::MIN),
            66 => position[0] = position[0].wrapping_add(153600),
            67 => {
                world.objects.components.waypoints.radius = [0, 0];
            }
            _ => (),
        }
        if branch == 61 {
            world.objects.components.waypoints.points[target_id.0]
                .as_mut()
                .unwrap()
                .lap = 1;
            for cell in &mut world.objects.components.waypoints.cells {
                *cell = vec![target_id];
            }
            let collider = &mut world.objects.world.colliders[target_id.0];
            collider.external_basis = Some(Matrix::IDENTITY);
            position = collider.pose.position;
            position[2] = position[2].wrapping_sub(1024);
        }
        if (68..80).contains(&branch) {
            let i = branch - 68;
            state.wrong_way = (i % 4) as u8;
            state.warning_timer = [40, 50, 175][i / 4];
            state.previous_score = 1000;
            racers[0].driving = i % 3 != 0;
        }
        if branch >= 80 {
            let i = branch - 80;
            clock.countdown = [3000, 1500, 1001, 1000, 1, 0][i % 6];
            clock.release = if i >= 6 { 1000 } else { 0 };
            racers[0].elapsed = i32::MAX;
        }
        let routes = &world.objects.components.waypoints;
        let mut header = Vec::new();
        ints(
            &mut header,
            [
                routes.origin[0],
                routes.origin[1],
                routes.shifts[0] as i32,
                routes.shifts[1] as i32,
                routes.radius[0],
                routes.radius[1],
                routes.backwards_index,
                i32::from(routes.fallback.is_some()),
                routes.fallback.map_or(0, |id| id.0 as i32),
            ],
        );
        let mut points = Vec::new();
        for p in &routes.points {
            let mut row = [0; 7];
            if let Some(p) = p {
                let (kind, next) = match p.next[0] {
                    Binding::Direct(None) => (0, 0),
                    Binding::Direct(Some(id)) => (1, id.0 as i32),
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
            ints(&mut points, row);
        }
        let mut ranges = Vec::new();
        let mut items = Vec::new();
        for cell in &routes.cells {
            ranges.extend([(items.len() / 2) as u16, cell.len() as u16]);
            ints(&mut items, cell.iter().map(|id| id.0 as i32));
        }
        let mut colliders = Vec::new();
        for i in 0..world.objects.world.colliders.len() {
            colliders.extend_from_slice(&scene_export::record(&world.objects.world, i)[..60]);
        }
        let identity = identity_assets::encode(&world.identity.entities);
        let initial = snapshot(&state, &racers, &clock, result, dt);
        let mut controls = Vec::new();
        if branch < 68 {
            for op in [0, 1, 2, 1] {
                controls.push([
                    op,
                    40,
                    position[0],
                    position[1],
                    position[2],
                    entity.0 as i32,
                    entity.0 as i32,
                    limit as i32,
                    0,
                    0,
                ]);
            }
        } else if branch < 80 {
            for i in 0..4 {
                controls.push([
                    3,
                    if i == 3 { -40 } else { 40 },
                    0,
                    0,
                    0,
                    entity.0 as i32,
                    (entity.0 ^ u32::from(i == 1)) as i32,
                    limit as i32,
                    if i >= 2 { 1200 } else { 900 - i * 100 },
                    0,
                ]);
            }
        } else {
            for elapsed in [
                40,
                1000,
                0,
                if branch & 1 == 0 { i32::MAX } else { i32::MIN },
            ] {
                controls.push([4, elapsed, 0, 0, 0, entity.0 as i32, 0, limit as i32, 0, 0]);
            }
        }
        let mut frames = Vec::new();
        for c in &controls {
            let view = waypoint::View {
                routes: &world.objects.components.waypoints,
                world: &world.objects.world,
                entities: &world.identity.entities,
            };
            let p = [c[2], c[3], c[4]];
            let status = match c[0] {
                0 => match state.cross(&view, p, &mut racers[0]) {
                    Ok(events) => {
                        result[0] = i32::from(events);
                        0
                    }
                    Err(e) => e,
                },
                1 => state
                    .progress(
                        &view,
                        p,
                        &racers[0],
                        player_route::Frame {
                            entity: EntityId(c[5] as u32),
                            number: c[6] as u32,
                            dt: c[1],
                            work_limit: c[7] as u32,
                        },
                    )
                    .err()
                    .unwrap_or(0),
                2 => match view.nearest(p, c[7] as u32) {
                    Ok(id) => {
                        result = [i32::from(id.is_some()), id.map_or(0, |v| v.0 as i32)];
                        0
                    }
                    Err(e) => e,
                },
                3 => {
                    state.score = c[8];
                    state.warning(racers[0].driving, EntityId(c[5] as u32), c[6] as u32, c[1]);
                    0
                }
                4 => {
                    clock.tick(c[1], racers.iter_mut());
                    dt = race_clock::physics_dt(c[1]);
                    0
                }
                _ => panic!("operation"),
            };
            let mut frame = Vec::new();
            ints(&mut frame, [status]);
            frame.extend(snapshot(&state, &racers, &clock, result, dt));
            frames.push(frame);
        }
        cases.push(json!({"tag":format!("route-{branch}"),"banked":branch%3==0,"state":initial,"header":header,"points":points,"ranges":ranges,"items":items,"colliders":colliders,"identity":identity,"controls":controls,"frames":frames}));
    }
    fs::write(out.join("player-route.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original ENGLAND checkpoint crossing, spatial progress/nearest queries, lap records, fallback and wrong-way state, plus raw-time race clocks for three racers. Actual linked identities/colliders, branch failures and short sequences; mixed Tank/fire dispatch remains separate."})).unwrap()).unwrap();
}
