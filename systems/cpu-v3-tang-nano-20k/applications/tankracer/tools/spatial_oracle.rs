//! Reference insertion/removal on the persistent original 8x8 grid.
use super::*;
use tankracer_game::{obstacle_collision::Grid, spatial};
fn snapshot(out: &mut Vec<i32>, grid: &Grid, status: i32, cell: Option<spatial::Cell>) {
    out.extend([
        status,
        cell.map_or(0, |c| i32::from(c.x)),
        cell.map_or(0, |c| i32::from(c.z)),
        i32::from(cell.is_some()),
    ]);
    for ids in &grid.cells {
        assert!(ids.len() <= 4);
        out.push(ids.len() as i32);
        out.extend(ids.iter().map(|id| id.0 as i32));
        out.extend(std::iter::repeat_n(-1, 4 - ids.len()));
    }
}
fn status(result: Result<(), spatial::Error>) -> i32 {
    result.map_or_else(
        |e| match e {
            spatial::Error::InvalidLimit => 2,
            spatial::Error::Limit => 3,
        },
        |()| 0,
    )
}
pub(super) fn append(cases: &mut Vec<Value>) {
    for shift in [0, 12, 31] {
        for position in [
            [0; 3],
            [4095, 7, 4096],
            [-1, -77, -1],
            [28672, 0, 28672],
            [i32::MIN, 0, i32::MAX],
            [100000, 0, -100000],
        ] {
            for radius in [None, Some(0), Some(20), Some(65535)] {
                for limit in [0, 1, 20] {
                    let removal = if position[0] == -1 {
                        None
                    } else if position[0] == i32::MIN {
                        Some(spatial::Cell {
                            x: i16::MIN,
                            z: u16::MAX,
                        })
                    } else {
                        Some(spatial::Cell { x: 0, z: 0 })
                    };
                    let mut grid = Grid {
                        origin: [0; 2],
                        shifts: [shift; 2],
                        cells: std::array::from_fn(|_| Vec::new()),
                    };
                    grid.cells[0] = vec![query::ObstacleId(0), query::ObstacleId(0)];
                    grid.cells[1] = vec![query::ObstacleId(1), query::ObstacleId(0)];
                    grid.cells[63] = vec![query::ObstacleId(0)];
                    let collider = query::Collider {
                        entity: query::EntityId(2),
                        pose: Pose {
                            position,
                            matrix: Matrix::IDENTITY,
                        },
                        external_basis: None,
                        bounds: geometry::Bounds {
                            min: [0; 3],
                            max: [0; 3],
                        },
                        radius: 20,
                    };
                    let input = position
                        .into_iter()
                        .chain([
                            0,
                            0,
                            shift as i32,
                            shift as i32,
                            i32::from(radius.is_some()),
                            i32::from(radius.unwrap_or(0)),
                            limit,
                            removal.map_or(0, |c| i32::from(c.x)),
                            removal.map_or(0, |c| i32::from(c.z)),
                            i32::from(removal.is_some()),
                        ])
                        .collect::<Vec<_>>();
                    assert_eq!(input.len(), 13);
                    let mut expected = Vec::new();
                    let cell = spatial::insert(&mut grid, query::ObstacleId(2), &collider, radius);
                    snapshot(&mut expected, &grid, 0, Some(cell));
                    let result =
                        spatial::remove(&mut grid, query::ObstacleId(2), Some(cell), limit as u32);
                    snapshot(&mut expected, &grid, status(result), Some(cell));
                    let new_cell =
                        spatial::insert(&mut grid, query::ObstacleId(3), &collider, radius);
                    snapshot(&mut expected, &grid, 0, Some(new_cell));
                    let result =
                        spatial::remove(&mut grid, query::ObstacleId(0), removal, limit as u32);
                    snapshot(&mut expected, &grid, status(result), removal);
                    assert_eq!(expected.len(), 1296);
                    cases.push(json!({"kind":20,"input":input,"status":0,"expected":expected}));
                }
            }
        }
    }
}
