//! Real ordinary car contacts followed by scene ticks and Stop dispatch.
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};
use tankracer_game::{
    dispatch, entity_factory as factory,
    math::Matrix,
    object_query::{self, EntityId, ObstacleId, TankId},
    scene_loading,
    scene_objects::{Execution, View},
    vehicle_runtime::{rcc_probe, SinglePlayer},
};
#[allow(dead_code)]
mod reference_world;
mod scene_export;
mod scenery_assets;
fn kind(w: &SinglePlayer, id: usize) -> Option<i32> {
    if w.objects
        .components
        .bounces
        .get(id)
        .is_some_and(Option::is_some)
    {
        return Some(2);
    }
    if let Some(f) = w.objects.components.falls.get(id).and_then(Option::as_ref) {
        let o = w.objects.components.objects[id].as_ref().unwrap();
        if f.attached.is_none() && o.data.auxiliary.is_none() && o.data.debris.is_none() {
            return Some(1);
        }
    }
    if w.objects.world.objects[id].class == w.registry.obstacle {
        return Some(0);
    }
    None
}
fn query(w: &SinglePlayer, a: [i32; 3], b: [i32; 3]) -> Option<usize> {
    let grid = &w.objects.world.grid;
    let mut hit = object_query::Hit::default();
    object_query::obstacles(
        &object_query::Grid {
            origin: grid.origin,
            shifts: grid.shifts,
            cells: grid.cells.each_ref().map(Vec::as_slice),
        },
        &w.objects.world.colliders,
        w.vehicles.entries[0].as_ref().unwrap().physics.entity,
        a,
        b,
        &mut hit,
    )
    .unwrap();
    hit.object.map(|id| id.0)
}
fn state(w: &SinglePlayer, id: usize, tag: i32) -> Vec<i32> {
    let mut out = Vec::new();
    if tag == 1 {
        let f = w.objects.components.falls[id].as_ref().unwrap();
        out.extend([
            f.angular_velocity,
            f.rotation,
            f.turn_rate,
            f.slope_limit,
            f.last_vehicle.map_or(-1, |e| e.0 as i32),
        ]);
    } else if tag == 2 {
        let b = &w.objects.components.bounces[id].as_ref().unwrap().state;
        out.extend(b.velocity);
        out.extend([
            b.spin,
            b.ground,
            b.timer as i32,
            b.age as i32,
            b.airborne as i32,
            b.reverse_spin as i32,
            b.special as i32,
            b.raised as i32,
        ]);
    }
    out.resize(11, 0);
    out
}
fn collider(w: &SinglePlayer, id: usize) -> Vec<i32> {
    let c = w.objects.world.colliders[id];
    let mut out = vec![c.entity.0 as i32];
    out.extend(c.pose.position);
    out.extend(c.pose.matrix.0);
    out.extend(c.external_basis.unwrap_or(Matrix::IDENTITY).0);
    out.extend(c.bounds.min);
    out.extend(c.bounds.max);
    out.extend([c.radius as i32, i32::from(c.external_basis.is_some())]);
    assert_eq!(out.len(), 30);
    out
}
fn properties(w: &SinglePlayer, id: usize) -> Vec<i32> {
    let p = w.objects.world.objects[id];
    let mut out = vec![
        p.class.0 as i32,
        p.weight,
        p.threshold,
        p.activity,
        p.flags as i32,
    ];
    out.extend(p.mesh_bounds.into_iter().flatten().map(i32::from));
    out
}
fn cell(w: &SinglePlayer, id: usize) -> [i32; 3] {
    w.objects.components.objects[id]
        .as_ref()
        .unwrap()
        .data
        .cell
        .map_or([0; 3], |c| [c.x as i32, c.z as i32, 1])
}
fn snapshot(w: &SinglePlayer, id: usize, tag: i32, status: i32) -> Vec<i32> {
    let v = &w.vehicles.entries[0].as_ref().unwrap().physics;
    let mut out = vec![status];
    out.extend(v.motion.velocity);
    out.extend([
        v.collision.impact_accumulator,
        v.collision.collision_size as i32,
        w.identity.current_slot as i32,
        w.shared.random as i32,
    ]);
    out.extend(collider(w, id));
    out.extend(properties(w, id));
    out.extend(state(w, id, tag));
    out.extend(cell(w, id));
    let entity = w.objects.world.colliders[id].entity;
    out.extend([
        w.identity
            .ticks
            .flags
            .get(entity.0 as usize)
            .copied()
            .unwrap_or(0) as i32,
        w.identity.ticks.next.map_or(-1, |e| e.0 as i32),
        w.identity.ticks.order.len() as i32,
    ]);
    for i in 0..32 {
        out.push(w.identity.ticks.order.get(i).map_or(-1, |e| e.0 as i32));
    }
    out.extend([
        w.identity.queues.notifications.len() as i32,
        w.identity.queues.destruction.len() as i32,
    ]);
    for list in [
        &w.identity.queues.notifications,
        &w.identity.queues.destruction,
    ] {
        for i in 0..16 {
            out.push(list.get(i).map_or(-1, |e| e.0 as i32));
        }
    }
    out.extend([
        i32::from(w.objects.components.ordinary.contains(&ObstacleId(id))),
        i32::from(w.objects.components.special.contains(&ObstacleId(id))),
    ]);
    for ids in &w.objects.world.grid.cells {
        out.push(ids.iter().filter(|o| o.0 == id).count() as i32);
    }
    assert_eq!(out.len(), 198);
    out
}
fn code(error: factory::Error) -> i32 {
    match error {
        factory::Error::Dispatch(dispatch::Error::Method(code)) => code,
        _ => -1,
    }
}
// Enter through the player's actual tick binding, then call the original
// private contact entry with that invocation's Context. The harness moves the
// identity owner out solely to express its disjoint borrow to the host Rust
// compiler; all nested activation still changes this one authoritative state.
struct CarContact<'a> {
    world: &'a mut SinglePlayer,
    mode: i32,
    limit: u32,
    scenery_ticks: u16,
}
impl factory::Methods<tankracer_game::scene_entity::Handle> for CarContact<'_> {
    fn invoke(
        &mut self,
        method: dispatch::MethodId,
        mut call: factory::Invocation<'_, tankracer_game::scene_entity::Handle>,
        env: factory::Environment<'_>,
    ) -> Result<(), i32> {
        if method != dispatch::MethodId(74) {
            if matches!(method.0, 65 | 80) {
                self.scenery_ticks += 1;
            }
            return factory::Methods::invoke(
                &mut View {
                    execution: Execution::Simulation {
                        random: &mut self.world.shared.random,
                    },
                    world: &mut self.world.objects.world,
                    components: &mut self.world.objects.components,
                    vehicles: Some(&mut self.world.vehicles),
                    effect_pool: Some(&mut self.world.shared.pool),
                    temporary: Some(&mut self.world.shared.temporary),
                },
                method,
                call,
                env,
            );
        }
        assert_eq!(
            call.entity,
            self.world.vehicles.entries[0]
                .as_ref()
                .unwrap()
                .physics
                .entity
        );
        rcc_probe::contacts(
            self.world,
            TankId(0),
            33,
            self.mode,
            2,
            self.limit,
            Some(&mut call.context),
        )
    }
}
fn scheduled_contact(w: &mut SinglePlayer, mode: i32, limit: u32, frame: bool) -> Result<u16, i32> {
    let entity = w.vehicles.entries[0].as_ref().unwrap().physics.entity;
    let mut identity = std::mem::replace(&mut w.identity, factory::State::new(2).unwrap());
    let classes = w.objects.world.classes.clone();
    let template = w.registry.environment(limit);
    let env = factory::Environment {
        classes: &classes,
        ..template
    };
    let mut methods = CarContact {
        world: w,
        mode,
        limit,
        scenery_ticks: 0,
    };
    let mut context = identity.context();
    let result = if frame {
        context
            .tick(scene_loading::TICK, env, &mut methods)
            .and_then(|_| context.drain(scene_loading::STOP, env, &mut methods))
    } else {
        context
            .dispatch(entity, scene_loading::TICK, None, env, &mut methods)
            .map(|_| ())
    };
    let ticks = methods.scenery_ticks;
    w.identity = identity;
    result.map(|_| ticks).map_err(code)
}
fn advance(w: &mut SinglePlayer, id: usize, action: usize, limit: u32) -> i32 {
    let env = w.registry.environment(limit);
    w.objects.components.dt = 33;
    let entity = w.objects.world.colliders[id].entity;
    let mut view = View {
        execution: Execution::Simulation {
            random: &mut w.shared.random,
        },
        world: &mut w.objects.world,
        components: &mut w.objects.components,
        vehicles: Some(&mut w.vehicles),
        effect_pool: Some(&mut w.shared.pool),
        temporary: Some(&mut w.shared.temporary),
    };
    let mut context = w.identity.context();
    if action == 4 {
        context
            .queues
            .queue(context.ticks, entity, false, limit)
            .unwrap();
    } else if let Err(e) = context.dispatch(entity, scene_loading::TICK, None, env, &mut view) {
        return code(e);
    }
    context
        .drain(scene_loading::STOP, env, &mut view)
        .err()
        .map_or(0, code)
}
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let out = Path::new(&args[0]);
    let root = Path::new(&args[1]);
    let mut session = reference_world::complete(root);
    let initial = session.world_mut_at_load().unwrap();
    let (scene, _) = scene_export::encode(&initial.objects.world);
    assert_eq!(
        fs::read(out.join("car-deform-scene.bin")).unwrap(),
        scene
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    );
    let mut segments = BTreeMap::new();
    for c in &initial.objects.world.colliders {
        for axis in [0, 2] {
            let mut a = c.pose.position;
            let mut b = a;
            a[axis] += 10000;
            b[axis] -= 10000;
            if let Some(id) = query(initial, a, b) {
                if let Some(tag) = kind(initial, id) {
                    segments.entry(id).or_insert((a, b, tag));
                }
            }
        }
    }
    let mut cases = Vec::<Value>::new();
    let mut counts = [0usize; 3];
    for (id, (a, b, tag)) in segments {
        counts[tag as usize] += 1;
        let variants = if counts[tag as usize] == 1 {
            if tag == 0 {
                8
            } else {
                10
            }
        } else {
            3
        };
        for variant in 0..variants {
            let mut session = reference_world::complete(root);
            let w = session.world_mut_at_load().unwrap();
            let contact_limit = if variant == 3 { 0 } else { 100000 };
            let tick_limit = if variant == 7 {
                0
            } else if variant == 9 {
                1
            } else {
                100000
            };
            let mode = if variant == 6 { 4 } else { 0 };
            if variant == 8 {
                // Controlled physical initial state: unrelated actors are
                // outside this car/scenery scheduler qualification sequence.
                let car = w.vehicles.entries[0].as_ref().unwrap().physics.entity;
                for flags in &mut w.identity.ticks.flags {
                    *flags &= !16;
                }
                w.identity.ticks.order.clear();
                w.identity.ticks.enable(car).unwrap();
                w.identity.ticks.next = None;
                w.objects.components.dt = 33;
            }
            if variant == 4 {
                w.identity
                    .ticks
                    .flags
                    .truncate(w.objects.world.colliders[id].entity.0 as usize);
            }
            if variant == 5 {
                w.objects.world.objects[id].activity = 1;
                w.objects.world.objects[id].flags |= 1;
            }
            let env = w
                .registry
                .hit_environment(w.objects.components.fall_tuning, false, 65536);
            let v = &mut w.vehicles.entries[0].as_mut().unwrap().physics;
            v.motion.poses.physical_position = a;
            v.motion.previous_position =
                std::array::from_fn(|i| a[i].wrapping_mul(2).wrapping_sub(b[i]));
            v.motion.poses.base_position = a;
            v.motion.poses.reference = Matrix::IDENTITY;
            v.motion.poses.body.matrix = Matrix::IDENTITY;
            v.motion.velocity = if variant == 2 {
                [-10000, -10000, 10000]
            } else {
                [-400000, -100000, 200000]
            };
            v.powertrain.local_velocity[2] = if variant == 1 { -300000 } else { 300000 };
            v.collision.impact_accumulator = 123;
            v.collision.collision_size = 55;
            w.shared.random = 7;
            w.identity.current_slot = 5;
            let mut input = vec![
                tag + if variant == 8 { 4 } else { 0 },
                5,
                33,
                mode,
                v.entity.0 as i32,
                v.powertrain.local_velocity[2],
                7,
                123,
                55,
                5,
                env.minimum_turn_rate,
                env.maximum_turn_rate,
            ];
            input.extend(a);
            input.extend(v.motion.previous_position);
            input.extend(a);
            input.extend(v.motion.velocity);
            input.extend(Matrix::IDENTITY.0);
            input.extend(Matrix::IDENTITY.0);
            input.push(1);
            input.extend(collider(w, id));
            input.extend(properties(w, id));
            let live = scenery_assets::encode(w);
            let row = &live[live[4] as usize + id * 8..][..8];
            let values = |offset: usize, count: usize| {
                live[offset..offset + count * 2]
                    .chunks_exact(2)
                    .map(|v| i32::from(v[0]) | (i32::from(v[1]) << 16))
                    .collect::<Vec<_>>()
            };
            let state_count = if tag == 1 {
                5
            } else if tag == 2 {
                11
            } else {
                0
            };
            let mut initial = values(live[6] as usize + row[1] as usize, state_count);
            initial.resize(11, 0);
            assert_eq!(initial, state(w, id, tag));
            input.extend(initial);
            let config_count = if tag == 1 {
                4
            } else if tag == 2 {
                11
            } else {
                0
            };
            let mut config = values(live[8] as usize + row[2] as usize, config_count);
            config.resize(11, 0);
            input.extend(config);
            input.push(i32::from(row[3]));
            input.extend(cell(w, id));
            input.extend([
                w.identity.ticks.flags.len() as i32,
                w.identity.ticks.order.len() as i32,
                w.identity.ticks.next.map_or(-1, |e| e.0 as i32),
            ]);
            assert_eq!(input.len(), 113);
            assert!(w.identity.ticks.flags.len() <= 512);
            for i in 0..512 {
                input.push(w.identity.ticks.flags.get(i).copied().unwrap_or(0) as i32);
            }
            for i in 0..32 {
                input.push(w.identity.ticks.order.get(i).map_or(-1, |e| e.0 as i32));
            }
            input.extend([
                (scene.len() * 2) as i32,
                0,
                (scene.len() * 2).div_ceil(128) as i32,
            ]);
            input.extend([
                w.identity.queues.notifications.len() as i32,
                w.identity.queues.destruction.len() as i32,
            ]);
            for list in [
                &w.identity.queues.notifications,
                &w.identity.queues.destruction,
            ] {
                for i in 0..16 {
                    input.push(list.get(i).map_or(-1, |e| e.0 as i32));
                }
            }
            for entity in 0..512 {
                input.push(
                    w.identity
                        .entities
                        .record(EntityId(entity))
                        .and_then(|r| r.class)
                        .map_or(65535, |c| c.0 as i32),
                );
            }
            for ids in &w.objects.world.grid.cells {
                input.push(ids.iter().filter(|o| o.0 == id).count() as i32);
            }
            input.extend([
                i32::from(w.objects.components.ordinary.contains(&ObstacleId(id))),
                i32::from(w.objects.components.special.contains(&ObstacleId(id))),
            ]);
            assert_eq!(input.len(), 1272);
            input.extend(w.objects.world.grid.origin);
            input.extend(w.objects.world.grid.shifts.map(|v| v as i32));
            input.extend([contact_limit as i32, tick_limit as i32]);
            let mut expected = Vec::new();
            let mut completed = 0;
            let initial_collider = collider(w, id);
            for action in 0..5 {
                let status = if variant == 8 {
                    let result = scheduled_contact(w, mode, contact_limit, true);
                    if action == 0 {
                        assert_eq!(
                            result,
                            Ok(0),
                            "newly enabled last member must wait until the next frame"
                        );
                    } else if action == 1 {
                        assert_eq!(
                            result,
                            Ok(1),
                            "the activated member must tick in the next frame"
                        );
                    }
                    result.err().unwrap_or(0)
                } else if action == 0 {
                    if variant % 2 == 0 {
                        scheduled_contact(w, mode, contact_limit, false).map(|_| ())
                    } else {
                        rcc_probe::obstacles_limit(w, TankId(0), 33, mode, contact_limit)
                    }
                    .err()
                    .unwrap_or(0)
                } else {
                    advance(w, id, action, tick_limit)
                };
                if variant == 8 && action == 0 {
                    assert_eq!(status, 0);
                    assert!(w
                        .identity
                        .ticks
                        .order
                        .contains(&w.objects.world.colliders[id].entity));
                }
                expected.extend(snapshot(w, id, tag, status));
                completed += 1;
                if status != 0 {
                    break;
                }
            }
            if variant == 8 {
                assert_eq!(completed, 5);
                assert_ne!(
                    collider(w, id),
                    initial_collider,
                    "following frames must update the same object"
                );
            }
            if variant == 4 || variant == 7 || (variant == 3 && tag != 0) {
                assert_eq!(
                    expected[(completed - 1) * 198],
                    -1,
                    "missing failure prefix: kind {tag}, variant {variant}"
                );
            }
            input[1] = completed as i32;
            for i in 0..512 {
                expected.push(w.identity.ticks.flags.get(i).copied().unwrap_or(0) as i32);
            }
            cases.push(json!({"kind":31,"input":input,"expected":expected,"status":0,"object":id,"variant":variant,"tag":tag}));
        }
    }
    assert!(
        counts[1] >= 4 && counts[2] >= 2 && counts[0] >= 1,
        "ordinary contact coverage: {counts:?}"
    );
    fs::write(
        out.join("car-obstacles.json"),
        serde_json::to_vec(&cases).unwrap(),
    )
    .unwrap();
    println!("ordinary car contacts: {counts:?}, {} cases", cases.len());
}
