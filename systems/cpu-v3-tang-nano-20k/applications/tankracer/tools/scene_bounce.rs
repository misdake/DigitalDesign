//! Connected Bounce contacts and ticks on the original complete ENGLAND world.
use super::*;
use tankracer_game::{
    bounce,
    dispatch::MethodId,
    obstacle_collision as obstacle,
    scene_objects::collision::{Collision, Services, Sound},
    vehicle_runtime::SinglePlayer,
};
struct Devices;
impl Services for Devices {
    fn sound(
        &mut self,
        _: Sound,
        _: &mut Collision<'_>,
        _: obstacle::Invocation<'_>,
    ) -> Result<(), i32> {
        panic!("Bounce sound");
    }
    fn feedback(
        &mut self,
        _: i32,
        _: &mut Collision<'_>,
        _: obstacle::Invocation<'_>,
    ) -> Result<(), i32> {
        panic!("feedback disabled");
    }
}
struct Methods<'a> {
    collision: Collision<'a>,
    environment: tankracer_game::breakable::Environment,
    calls: usize,
}
impl obstacle::Effects for Methods<'_> {
    fn method(&mut self, method: MethodId, call: obstacle::Invocation<'_>) -> Result<(), i32> {
        assert_eq!(method, tankracer_game::scene_objects::BOUNCE_HIT);
        self.calls += 1;
        self.collision
            .method(method, call, self.environment, &mut Devices, true)
    }
    fn feedback(&mut self, _: i32, _: &mut obstacle::State, _: &mut u32) -> Result<(), i32> {
        panic!("feedback disabled");
    }
    fn impact(
        &mut self,
        _: &mut obstacle::State,
        _: usize,
        _: i32,
        _: &mut u32,
    ) -> Result<(), i32> {
        panic!("unexpected impact");
    }
}
fn state(s: &bounce::State) -> [i32; 11] {
    [
        s.velocity[0],
        s.velocity[1],
        s.velocity[2],
        s.spin,
        s.ground,
        i32::from(s.timer),
        i32::from(s.age),
        i32::from(s.airborne),
        i32::from(s.reverse_spin),
        i32::from(s.special),
        i32::from(s.raised),
    ]
}
fn config(c: bounce::Config) -> [i32; 10] {
    [
        i32::from(c.elasticity),
        i32::from(c.friction),
        i32::from(c.height_factor),
        i32::from(c.hit_spin.multiplier as i16),
        i32::from(c.hit_spin.shift),
        i32::from(c.slow_spin.multiplier as i16),
        i32::from(c.slow_spin.shift),
        i32::from(c.deflection),
        c.gravity,
        i32::from(c.height_period),
    ]
}
fn snapshot(
    w: &SinglePlayer,
    id: usize,
    car: &obstacle::State,
    hit: &Hit<ObstacleId>,
    status: i32,
) -> Vec<i32> {
    let mut row = vec![
        status,
        hit.entity.map_or(-1, |v| v.0 as i32),
        hit.object.map_or(-1, |v| v.0 as i32),
        hit.code,
    ];
    row.extend(hit.point);
    row.extend(hit.normal);
    row.extend(car.motion.velocity);
    row.extend([
        car.collision.impact_accumulator,
        car.collision.collision_size as i32,
    ]);
    row.extend(state(&w.objects.components.bounces[id].unwrap().state));
    let object = w.objects.components.objects[id].as_ref().unwrap();
    row.extend([
        object.data.cell.map_or(0, |c| i32::from(c.x)),
        object.data.cell.map_or(0, |c| i32::from(c.z)),
        i32::from(object.data.cell.is_some()),
    ]);
    let p = w.objects.world.objects[id];
    row.extend([
        p.class.0 as i32,
        p.weight,
        p.threshold,
        p.activity,
        p.flags as i32,
    ]);
    for v in record(&w.objects.world, id)[..60].as_chunks::<2>().0 {
        row.push(i32::from(v[0]) | (i32::from(v[1]) << 16));
    }
    let entity = w.objects.world.colliders[id].entity;
    row.extend([
        w.identity.current_slot as i32,
        w.shared.random as i32,
        w.shared.random as i32,
        w.identity.ticks.flags[entity.0 as usize] as i32,
    ]);
    assert_eq!(row.len(), 68);
    for (ids, capacity) in [
        (
            w.identity
                .ticks
                .order
                .iter()
                .map(|v| v.0 as i32)
                .collect::<Vec<_>>(),
            256,
        ),
        (
            w.objects
                .components
                .ordinary
                .iter()
                .map(|v| v.0 as i32)
                .collect(),
            256,
        ),
        (
            w.objects
                .components
                .special
                .iter()
                .map(|v| v.0 as i32)
                .collect(),
            256,
        ),
        (
            w.identity
                .queues
                .notifications
                .iter()
                .map(|v| v.0 as i32)
                .collect(),
            32,
        ),
        (
            w.identity
                .queues
                .destruction
                .iter()
                .map(|v| v.0 as i32)
                .collect(),
            32,
        ),
    ] {
        assert!(ids.len() <= capacity);
        row.push(ids.len() as i32);
        row.extend(&ids);
        row.extend(std::iter::repeat_n(-1, capacity - ids.len()));
    }
    let grid = &w.objects.world.grid;
    row.extend(grid.cells.iter().map(|v| v.len() as i32));
    let count: usize = grid.cells.iter().map(Vec::len).sum();
    assert!(count <= 256);
    row.extend(grid.cells.iter().flatten().map(|v| v.0 as i32));
    row.extend(std::iter::repeat_n(-1, 256 - count));
    assert_eq!(row.len(), 1225);
    row
}
pub(super) fn export(
    w: &mut SinglePlayer,
    segments: &[(usize, [i32; 3], [i32; 3])],
    out: &Path,
) -> Result<usize, Box<dyn Error>> {
    assert_eq!(
        segments.len(),
        2,
        "both original Bounce profiles need a real query"
    );
    let mut car = w.vehicles.entries[w.player.0]
        .as_ref()
        .unwrap()
        .physics
        .clone();
    let mut initial = vec![0x424e, 1, 0];
    for v in car.motion.poses.reference.0 {
        append_i32(&mut initial, v);
    }
    for v in car
        .external_basis()
        .unwrap_or(tankracer_game::math::Matrix::IDENTITY)
        .0
    {
        append_i32(&mut initial, v);
    }
    initial.push(u16::from(car.external_basis().is_some()));
    assert_eq!(initial.len(), 40);
    for v in [
        car.collision.impact_accumulator,
        car.collision.collision_size as i32,
    ] {
        append_i32(&mut initial, v);
    }
    initial.push(w.registry.fall.0 as u16);
    append_i32(&mut initial, car.entity.0 as i32);
    assert_eq!(initial.len(), 47);
    let mut indices = BTreeMap::new();
    let mut split = 0;
    for (id, component) in w.objects.components.bounces.iter().enumerate() {
        let Some(c) = component else { continue };
        let object = w.objects.components.objects[id].as_ref().unwrap();
        let base = initial.len();
        indices.insert(id, usize::from(initial[2]));
        initial[2] += 1;
        initial.push(id as u16);
        for v in state(&c.state).into_iter().chain(config(c.config)) {
            append_i32(&mut initial, v);
        }
        initial.extend(object.data.cell.map_or([0, 0, 0], |c| [c.x as u16, c.z, 1]));
        initial.extend([
            u16::from(c.reference_alias),
            u16::from(object.data.mesh.is_some()),
        ]);
        append_i32(&mut initial, object.data.impact_threshold);
        assert_eq!(initial.len() - base, 50);
        let class = &w.objects.world.classes[w.objects.world.objects[id].class.0];
        assert_eq!(
            class.methods[24],
            Some(tankracer_game::scene_objects::BOUNCE_HIT)
        );
        assert_eq!(
            class.methods[12],
            Some(tankracer_game::scene_objects::BOUNCE_TICK)
        );
        assert_eq!(
            class.methods[4],
            Some(tankracer_game::breakable::ENABLE_TICK)
        );
        split += usize::from(w.objects.world.objects[id].threshold != object.data.impact_threshold);
    }
    assert!(initial[2] <= 32);
    binary(&out.join("bounce-initial.bin"), &initial)?;
    let env = obstacle::Environment {
        mode: 0,
        dt: 33,
        feedback: false,
        player_class: car.class,
        breakable_class: w.registry.fall,
        obstacle_slot: 24,
        deformable_slot: 21,
        tuning: obstacle::Tuning::default(),
    };
    let hit_env = w
        .registry
        .hit_environment(w.objects.components.fall_tuning, false, 10000);
    let mut records = Vec::new();
    let mut landed = false;
    let mut stopped = false;
    for action in 0..55 {
        for &(id, a, b) in segments {
            let dt = 33;
            let mut control = vec![if action == 0 { 1 } else { 2 }, indices[&id] as u16];
            append_i32(&mut control, dt);
            for v in a
                .into_iter()
                .chain(b)
                .chain([-400000, -100000, 200000])
                .chain([300000])
            {
                append_i32(&mut control, v);
            }
            assert_eq!(control.len(), 24);
            let mut hit = Hit::default();
            let status;
            if action == 0 {
                car.motion.poses.base_position = a;
                car.motion.poses.physical_position = a;
                car.motion.previous_position =
                    std::array::from_fn(|i| a[i].wrapping_mul(2).wrapping_sub(b[i]));
                car.motion.velocity = [-400000, -100000, 200000];
                car.powertrain.local_velocity[2] = 300000;
                let g = &w.objects.world;
                assert!(query::obstacles(
                    &query::Grid {
                        origin: g.grid.origin,
                        shifts: g.grid.shifts,
                        cells: g.grid.cells.each_ref().map(Vec::as_slice)
                    },
                    &g.colliders,
                    car.entity,
                    a,
                    b,
                    &mut hit
                )
                .unwrap());
                assert_eq!(hit.object, Some(ObstacleId(id)));
                let (lifecycle, slot) = w.identity.split();
                let mut methods = Methods {
                    collision: Collision {
                        components: &mut w.objects.components,
                        lifecycle,
                        environment: w.registry.environment(10000),
                        cars: None,
                    },
                    environment: hit_env,
                    calls: 0,
                };
                obstacle::obstacles(
                    &mut w.objects.world,
                    &mut car,
                    env,
                    obstacle::Globals {
                        random: &mut w.shared.random,
                        current_slot: slot,
                    },
                    &mut methods,
                )
                .map_err(|e| format!("Bounce contact {id}: {e:?}"))?;
                assert_eq!(methods.calls, 1);
                status = 1;
            } else {
                w.objects.components.dt = dt;
                let entity = w.objects.world.colliders[id].entity;
                let result = w.identity.context().dispatch(
                    entity,
                    tankracer_game::scene_loading::TICK,
                    None,
                    w.registry.environment(10000),
                    &mut w
                        .objects
                        .updates_with_vehicles(&mut w.shared.random, &mut w.vehicles),
                );
                assert!(result.is_ok(), "Bounce tick {id}: {result:?}");
                status = 0;
                let s = &w.objects.components.bounces[id].unwrap().state;
                landed |= s.airborne == 0;
                stopped |= w.objects.world.objects[id].activity == 0;
            }
            for v in snapshot(w, id, &car, &hit, status) {
                append_i32(&mut control, v);
            }
            assert_eq!(control.len(), 2474);
            records.extend(control);
        }
    }
    assert!(
        landed && stopped,
        "connected Bounce sequence: landed={landed}, stopped={stopped}, final={:?}",
        segments
            .iter()
            .map(|(id, _, _)| (
                *id,
                w.objects.components.bounces[*id].unwrap(),
                w.objects.world.objects[*id].activity,
                w.objects.components.objects[*id]
                    .as_ref()
                    .unwrap()
                    .data
                    .impact_threshold
            ))
            .collect::<Vec<_>>()
    );
    binary(&out.join("bounce-updates.bin"), &records)?;
    println!(
        "Bounce: {} authored objects, {split} with different settling/contact values",
        initial[2]
    );
    Ok(records.len() / 2474)
}
