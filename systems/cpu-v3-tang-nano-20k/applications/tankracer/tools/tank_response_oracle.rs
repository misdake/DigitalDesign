//! Ordered reference tank response with concrete shared particles and damage.
use super::*;
use tankracer_game::{tank_collision as tank, vehicle};
struct Services<'a, 'b> {
    vehicles: &'a mut [vehicle::State; 2],
    impacts: &'a mut impact::State,
    context: &'a mut impact::Context<'b>,
    damage: &'a mut [damage::State; 2],
    meshes: &'a mut [Geometry; 2],
    present: [bool; 2],
}
impl tank::Effects for Services<'_, '_> {
    fn impact(&mut self, id: query::TankId, index: usize, kind: i32) -> Result<(), i32> {
        self.impacts
            .emit(&mut self.vehicles[id.0], index as u32, kind, self.context)
            .map(|_| ())
            .map_err(|e| -impact_error(e))
    }
    fn track_damage(&mut self, id: query::TankId, kind: i32, world: [i32; 3]) -> Result<(), i32> {
        let v = &self.vehicles[id.0];
        damage::Target {
            state: &mut self.damage[id.0],
            player: v.player,
            body_pose: v.motion.poses.body,
            body_vertices: [0; 4],
            body_mesh: self.present[id.0].then_some(&mut self.meshes[id.0]),
        }
        .track(kind, world)
        .map_err(|_| -5)
    }
}
pub(super) fn append(cases: &mut Vec<Value>) {
    for variant in 0..16 {
        for speed in [0, 250001, 450000, 900001] {
            for mode in [0, 4] {
                for own in 0..2 {
                    let extent = if variant >= 14 { 600 } else { 100 };
                    let bounds = geometry::Bounds {
                        min: [-extent; 3],
                        max: [extent; 3],
                    };
                    let mut geometry = std::array::from_fn::<_, 2, _>(|i| query::Tank {
                        collider: query::Collider {
                            entity: query::EntityId(10 + i as u32),
                            pose: Pose {
                                matrix: if variant == 3 {
                                    Matrix::from_angles([3000, 13000, 8000])
                                } else {
                                    Matrix::IDENTITY
                                },
                                position: if i == 0 {
                                    [0; 3]
                                } else if variant == 1 || variant >= 14 {
                                    [0, 0, 1000]
                                } else if variant == 2 {
                                    [50, 0, 0]
                                } else {
                                    [0, 0, 50]
                                },
                            },
                            external_basis: Some(Matrix::IDENTITY),
                            bounds,
                            radius: 20,
                        },
                        slot: i as u8,
                        active: true,
                        disabled: false,
                        ready: true,
                        overlap_bounds: bounds,
                        contacts: if variant >= 14 {
                            [[0, 0, 500], [0, 0, 490], [10, 0, 500], [10, 0, 490]]
                        } else if variant == 1 {
                            [[100000; 3]; 4]
                        } else {
                            [[0, 0, 10], [0, 0, -10], [10, 0, 10], [10, 0, -10]]
                        },
                    });
                    if variant == 4 {
                        geometry[own].active = false;
                    }
                    if variant == 5 {
                        geometry[1 - own].disabled = true;
                    }
                    if variant == 6 {
                        geometry[1 - own].ready = false;
                    }
                    if variant == 13 {
                        geometry[1 - own].active = false;
                    }
                    let mut slots = [None; 9];
                    slots[0] = Some(query::TankId(0));
                    slots[1] = Some(query::TankId(1));
                    if variant == 7 {
                        slots[1 - own] = Some(query::TankId(own));
                    }
                    if variant == 8 {
                        slots[1 - own] = Some(query::TankId(7));
                    }
                    if variant == 12 {
                        slots = [None; 9];
                    }
                    let mut states = std::array::from_fn::<_, 2, _>(|i| tank::State {
                        velocity: if i == 0 {
                            [0, 777, speed]
                        } else {
                            [0, -999, 0]
                        },
                        local_velocity: [0, 0, if i == 0 { speed } else { 0 }],
                        previous_position: if variant == 1 {
                            [0, 0, -1600]
                        } else {
                            geometry[i].collider.pose.position
                        },
                        collision_timer: if i == 0 { 77 } else { -55 },
                        player: i == 0,
                    });
                    let mut vehicles = std::array::from_fn::<_, 2, _>(|i| {
                        let mut v = vehicle::State {
                            entity: geometry[i].collider.entity,
                            player: states[i].player,
                            impact_cooldown: 500,
                            ..Default::default()
                        };
                        v.motion.poses.body = geometry[i].collider.pose;
                        v.pose_input.external =
                            tankracer_game::pose::ExternalBasis::Custom(Matrix::IDENTITY);
                        v.powertrain.local_velocity = states[i].local_velocity;
                        for (contact, p) in v.motion.contacts.iter_mut().zip(geometry[i].contacts) {
                            contact.world = p;
                        }
                        v
                    });
                    let count = if variant == 9 { 1 } else { 2 };
                    let env = tank::Environment {
                        mode,
                        dt: 33,
                        separation_limit: if variant == 3 || variant == 14 { 1 } else { 0 },
                    };
                    let present = [variant != 11, true];
                    let mut input = vec![
                        33,
                        mode,
                        env.separation_limit as i32,
                        own as i32,
                        variant,
                        2,
                        count as i32,
                        if variant == 15 { 2 } else { 1 },
                    ];
                    for g in &geometry {
                        input.extend(tank_row(g));
                    }
                    for s in states {
                        input.extend(state_row(s));
                    }
                    input.extend(slots.map(|id| id.map_or(65535, |id| id.0 as i32)));
                    input.extend(present.map(i32::from));
                    assert_eq!(input.len(), 145);
                    let mut state = impact::State::default();
                    if variant == 10 {
                        state.next = 4;
                    }
                    let mut pool = effect_pool::Pool::default();
                    let mut seed = 7u32;
                    let mut peer = None;
                    let mut damage = std::array::from_fn::<_, 2, _>(|_| damage::State::default());
                    let mut meshes = std::array::from_fn::<_, 2, _>(|_| Geometry {
                        vertices: vec![[4096, -17, -4096], [0, 300, 20], [300, 5, 0]],
                        triangles: vec![triangle([0, 1, 2])],
                    });
                    let mut selected = own;
                    let mut result = Ok(None);
                    for _ in 0..input[7] {
                        result = tank::step(
                            query::TankId(selected),
                            &geometry,
                            &mut states[..count],
                            &slots,
                            env,
                            &mut peer,
                            &mut Services {
                                vehicles: &mut vehicles,
                                impacts: &mut state,
                                context: &mut impact::Context {
                                    frame: 17,
                                    random: &mut seed,
                                    pool: &mut pool,
                                    tuning: impact::Tuning::default(),
                                },
                                damage: &mut damage,
                                meshes: &mut meshes,
                                present,
                            },
                        );
                        if result.is_err() {
                            break;
                        }
                        selected = 1 - selected;
                    }
                    let (status, hit) = match result {
                        Ok(hit) => (i32::from(hit.is_some()), hit.map_or(-1, |id| id.0 as i32)),
                        Err(tank::Error::Query(query::Error::Arithmetic)) => (2, -1),
                        Err(tank::Error::Effect(c)) => (-c, -1),
                        Err(tank::Error::Limit) => (6, -1),
                        Err(_) => (4, -1),
                    };
                    let mut expected =
                        impact_snapshot([status, -99, -99, -99], &state, &vehicles[0], &pool, seed);
                    expected.push(hit);
                    expected.extend(peer.map_or([-1, 0, 0], |p| {
                        [p.tank.0 as i32, p.velocity_xz[0], p.velocity_xz[1]]
                    }));
                    for s in states {
                        expected.extend(state_row(s));
                    }
                    expected.push(vehicles[1].impact_cooldown as i32);
                    for i in 0..2 {
                        expected.extend(damage[i].body.map(i32::from));
                        expected.extend(damage[i].turret.map(i32::from));
                        expected.extend(damage[i].tracks.map(i32::from));
                        expected
                            .extend(meshes[i].vertices.iter().flatten().copied().map(i32::from));
                        expected.extend(meshes[i].triangles[0].normal.map(i32::from));
                    }
                    assert_eq!(expected.len(), 1359);
                    cases.push(json!({"kind":17,"input":input,"status":0,"expected":expected}));
                }
            }
        }
    }
}
fn state_row(s: tank::State) -> Vec<i32> {
    s.velocity
        .into_iter()
        .chain(s.local_velocity)
        .chain(s.previous_position)
        .chain([s.collision_timer, i32::from(s.player)])
        .collect()
}
