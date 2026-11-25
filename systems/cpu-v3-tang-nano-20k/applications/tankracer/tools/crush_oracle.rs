//! Reference record animation and retirement through the real shared queues.
use super::*;
use tankracer_game::{crush, lifecycle as life, scene_collision, scene_meshes};
pub(super) fn append(cases: &mut Vec<Value>) {
    for variant in 0..6 {
        for dt in [-1, 16, 33, 1500] {
            for limit in [0, 1, 20] {
                let records = vec![
                    deform::Record {
                        timer: 20,
                        state: if variant >= 3 { 3 } else { 0 },
                        color: [127, 3, 9],
                        ..Default::default()
                    },
                    deform::Record {
                        timer: -7,
                        state: if variant >= 3 { 3 } else { 1 },
                        color: [255, 2, 3],
                        ..Default::default()
                    },
                    deform::Record {
                        timer: 10,
                        state: 2,
                        color: [if variant >= 4 { 255 } else { 127 }, 77, 12],
                        ..Default::default()
                    },
                ];
                let mut meshes = scene_meshes::Meshes::default();
                let mesh = meshes.insert(Geometry {
                    vertices: Vec::new(),
                    triangles: Vec::new(),
                });
                let mut surface = scene_collision::Deformable {
                    config: deform_hit::Config {
                        floor: 50,
                        compression: 1,
                        drift_shift: 1,
                        material: 0,
                        has_sound: false,
                    },
                    state: deform_hit::State::default(),
                    mesh,
                    vertex_flags: Vec::new(),
                    sampling: scene_collision::Sampling {
                        bounds: [[0; 2]; 2],
                        edge_cap: 0,
                        state: deform::State {
                            pose: deform::Pose::default(),
                            records,
                            records_attached: variant != 1,
                            dirty: 1,
                        },
                    },
                    runtime: crush::State {
                        siren_timer: if variant == 2 { 11026 } else { 0 },
                        sound_channel: 99,
                        highlighted: true,
                        private_mesh: true,
                    },
                };
                let mut ticks = life::Ticks {
                    flags: vec![0; 2],
                    order: Vec::new(),
                    next: None,
                };
                let mut queues = life::Queues::default();
                let id = query::EntityId(if variant == 5 { 7 } else { 1 });
                let mut input = vec![
                    dt,
                    limit,
                    id.0 as i32,
                    i32::from(surface.sampling.state.records_attached),
                    i32::from(surface.sampling.state.dirty),
                    surface.runtime.siren_timer,
                    surface.runtime.sound_channel as i32,
                ];
                for r in &surface.sampling.state.records {
                    input.extend(record_row(r));
                }
                assert_eq!(input.len(), 40);
                let mut expected = Vec::new();
                for _ in 0..3 {
                    let status =
                        crush::tick(&mut surface, dt, id, &mut ticks, &mut queues, limit as u32)
                            .map_or_else(
                                |e| match e {
                                    crush::Error::InvalidLimit => 2,
                                    crush::Error::Limit => 3,
                                    crush::Error::DetachedRecords => 4,
                                    crush::Error::Lifecycle(life::Error::MissingEntity(_)) => 6,
                                    crush::Error::Lifecycle(life::Error::Spatial(
                                        tankracer_game::spatial::Error::InvalidLimit,
                                    )) => 2,
                                    crush::Error::Lifecycle(life::Error::Spatial(
                                        tankracer_game::spatial::Error::Limit,
                                    )) => 3,
                                    other => panic!("unexpected {other:?}"),
                                },
                                |()| 0,
                            );
                    expected.extend([
                        status,
                        i32::from(surface.sampling.state.records_attached),
                        surface.runtime.siren_timer,
                        surface.runtime.sound_channel as i32,
                        ticks.flags[0] as i32,
                        ticks.flags[1] as i32,
                        queues.notifications.len() as i32,
                        queues.notifications.front().map_or(-1, |id| id.0 as i32),
                    ]);
                    for r in &surface.sampling.state.records {
                        expected.extend(record_row(r));
                    }
                }
                assert_eq!(expected.len(), 123);
                cases.push(json!({"kind":19,"input":input,"status":0,"expected":expected}));
            }
        }
    }
}
