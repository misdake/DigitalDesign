//! Persistent live geometry/membership changes followed by original queries.
use super::*;
use tankracer_game::{obstacle_collision::Grid, spatial};
pub(super) fn append(cases: &mut Vec<Value>) {
    for external in 0..3 {
        for invalid in [0, 7, 65536, u32::MAX] {
            for deformable in [false, true] {
                for own in [u32::MAX, 10] {
                    let mut colliders = [0, 200].map(|x| query::Collider {
                        entity: query::EntityId(if x == 0 { 10 } else { 11 }),
                        pose: Pose {
                            position: [x, 0, 0],
                            matrix: Matrix::IDENTITY,
                        },
                        external_basis: match external {
                            0 => None,
                            1 => Some(Matrix::IDENTITY),
                            _ => Some(Matrix::from_angles([1000, 9000, 3333])),
                        },
                        bounds: geometry::Bounds {
                            min: [-100; 3],
                            max: [100; 3],
                        },
                        radius: 20,
                    });
                    let mut grid = Grid {
                        origin: [-4096; 2],
                        shifts: [12; 2],
                        cells: std::array::from_fn(|_| {
                            vec![query::ObstacleId(invalid as usize), query::ObstacleId(1)]
                        }),
                    };
                    let mut input = collider_row(&colliders[0]);
                    input.extend(collider_row(&colliders[1]));
                    input.extend([
                        500,
                        0,
                        0,
                        -500,
                        0,
                        0,
                        own as i32,
                        i32::from(deformable),
                        invalid as i32,
                    ]);
                    assert_eq!(input.len(), 69);
                    input.extend(Matrix::from_angles([3000, 2000, 1000]).0);
                    let mut expected = Vec::new();
                    for step in 0..6 {
                        if step == 1 {
                            colliders[0].pose.position[1] = 10000;
                        }
                        if step == 2 {
                            colliders[0].pose.position[1] = 0;
                            colliders[0]
                                .pose
                                .matrix
                                .rotate(tankracer_game::math::Axis::Y, 8192);
                            spatial::remove(
                                &mut grid,
                                query::ObstacleId(1),
                                Some(spatial::Cell { x: 1, z: 1 }),
                                1000,
                            )
                            .unwrap();
                        }
                        if step == 3 {
                            spatial::remove(
                                &mut grid,
                                query::ObstacleId(0),
                                Some(spatial::Cell { x: 1, z: 1 }),
                                1000,
                            )
                            .unwrap();
                        }
                        if step == 4 {
                            spatial::insert(
                                &mut grid,
                                query::ObstacleId(1),
                                &colliders[1],
                                Some(20),
                            );
                        }
                        if step == 5 {
                            colliders[1].external_basis =
                                Some(Matrix::from_angles([3000, 2000, 1000]));
                        }
                        let mut hit = query::Hit {
                            point: [11, 22, 33],
                            normal: [-44, 55, -66],
                            ..Default::default()
                        };
                        let result = if deformable {
                            query::deformables(
                                &[query::ObstacleId(0), query::ObstacleId(1)],
                                &colliders,
                                [500, 0, 0],
                                [-500, 0, 0],
                                &mut hit,
                            )
                        } else {
                            query::obstacles(
                                &query::Grid {
                                    origin: grid.origin,
                                    shifts: grid.shifts,
                                    cells: grid.cells.each_ref().map(|x| x.as_slice()),
                                },
                                &colliders,
                                query::EntityId(own),
                                [500, 0, 0],
                                [-500, 0, 0],
                                &mut hit,
                            )
                        };
                        expected.push(result.map_or_else(
                            |e| if e == query::Error::Arithmetic { 3 } else { 4 },
                            i32::from,
                        ));
                        expected.extend(obstacle_hit(&hit));
                        let row = &grid.cells[9];
                        assert!(row.len() <= 3);
                        expected.push(row.len() as i32);
                        expected.extend(row.iter().map(|id| id.0 as i32));
                        expected.extend(std::iter::repeat_n(-1, 3 - row.len()));
                    }
                    assert_eq!(expected.len(), 84);
                    cases.push(json!({"kind":23,"input":input,"status":0,"expected":expected}));
                }
            }
        }
    }
}
