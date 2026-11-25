//! Ground response with the reference's real synchronous particle/damage services.
use super::*;
use tankracer_game::{ground_collision as ground, pose, vehicle};
struct Services<'a, 'b> {
    vehicle: &'a mut vehicle::State,
    impacts: &'a mut impact::State,
    context: &'a mut impact::Context<'b>,
    damage: &'a mut damage::State,
    mesh: &'a mut Geometry,
    present: bool,
}
impl ground::Effects for Services<'_, '_> {
    fn impact(&mut self, index: usize, kind: i32) -> Result<(), i32> {
        self.impacts
            .emit(self.vehicle, index as u32, kind, self.context)
            .map(|_| ())
            .map_err(|e| -impact_error(e))
    }
    fn track_damage(&mut self, kind: i32, world: [i32; 3]) -> Result<(), i32> {
        damage::Target {
            state: self.damage,
            player: self.vehicle.player,
            body_pose: self.vehicle.motion.poses.body,
            body_vertices: [0; 4],
            body_mesh: self.present.then_some(self.mesh),
        }
        .track(kind, world)
        .map_err(|_| -5)
    }
}
pub(super) fn append(cases: &mut Vec<Value>) {
    for kinds in [
        [3, 3, 3, 3],
        [4, 4, 4, 4],
        [5, 5, 5, 5],
        [3, 4, 5, 0],
        [0, 0, 4, 3],
    ] {
        for forward in [150000, 150001, 450001, -450001] {
            for variant in 0..4 {
                let basis = Matrix::from_angles([0, 13000, 0]);
                let contacts = [[0, 0, 100], [0, 0, -100], [100, 0, 100], [100, 0, -100]];
                let mut v = vehicle::State {
                    entity: query::EntityId(1),
                    player: variant != 3,
                    impact_cooldown: 500,
                    ..Default::default()
                };
                v.pose_input.external = pose::ExternalBasis::Custom(basis);
                v.motion.poses.body = Pose {
                    matrix: basis,
                    position: [10, 20, 30],
                };
                for (c, p) in v.motion.contacts.iter_mut().zip(contacts) {
                    c.world = p;
                }
                v.powertrain.local_velocity[2] = forward;
                let mode = if variant == 2 { 2 } else { 0 };
                let mut input = vec![0, 2, 500, 0, 1, forward, 33, 0, 1];
                input.extend(contacts.into_iter().flatten());
                input.extend(basis.0);
                let tuning = impact::Tuning::default();
                input.extend(tuning.forward_offset);
                input.extend(tuning.reverse_offset);
                let mut ground = ground::State {
                    velocity: [250001, -777, -400001],
                    yaw: 700,
                };
                let mut points = std::array::from_fn::<_, 4, _>(|i| ground::Contact {
                    kind: kinds[i],
                    corner: i as i32,
                    normal: if i & 1 == 0 {
                        [-4096, 0, 0]
                    } else {
                        [0, 0, 4096]
                    },
                });
                for p in &points {
                    input.extend([i32::from(p.kind), p.corner]);
                    input.extend(p.normal);
                }
                input.extend(ground.velocity);
                input.extend([
                    ground.yaw,
                    mode,
                    i32::from(v.player),
                    i32::from(variant != 1),
                ]);
                input.extend(v.motion.poses.body.position);
                let mut mesh = Geometry {
                    vertices: vec![[4096, -17, -4096], [0, 300, 20], [300, 5, 0]],
                    triangles: vec![triangle([0, 1, 2])],
                };
                let mut damage = damage::State::default();
                let mut state = impact::State::default();
                let mut pool = effect_pool::Pool::default();
                let mut seed = 7;
                let before = ground.velocity;
                let status = ground
                    .step(
                        &mut points,
                        ground::Environment {
                            mode,
                            player: v.player,
                            forward_velocity: forward,
                            position: [10, 20, 30],
                            external_basis: Some(basis),
                        },
                        &mut Services {
                            vehicle: &mut v,
                            impacts: &mut state,
                            context: &mut impact::Context {
                                frame: 17,
                                random: &mut seed,
                                pool: &mut pool,
                                tuning,
                            },
                            damage: &mut damage,
                            mesh: &mut mesh,
                            present: variant != 1,
                        },
                    )
                    .map_or_else(
                        |e| match e {
                            ground::Error::Arithmetic(_) => 2,
                            ground::Error::Effect(c) => -c,
                        },
                        |()| 0,
                    );
                let mut expected =
                    impact_snapshot([status, -99, -99, -99], &state, &v, &pool, seed);
                expected.extend(ground.velocity);
                expected.push(ground.yaw);
                for p in points {
                    expected.extend(p.normal);
                }
                expected.extend(if status == 0 {
                    std::array::from_fn(|i| ground.velocity[i].wrapping_sub(before[i]))
                } else {
                    [99; 3]
                });
                expected.extend(damage.body.map(i32::from));
                expected.extend(damage.turret.map(i32::from));
                expected.extend(damage.tracks.map(i32::from));
                expected.extend(mesh.vertices.iter().flatten().copied().map(i32::from));
                expected.extend(mesh.triangles[0].normal.map(i32::from));
                assert_eq!(input.len(), 66);
                assert_eq!(expected.len(), 1329);
                cases.push(json!({"kind":16,"input":input,"status":0,"expected":expected}));
            }
        }
    }
}
