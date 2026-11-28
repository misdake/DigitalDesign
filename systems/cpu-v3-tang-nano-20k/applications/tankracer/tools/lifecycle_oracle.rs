//! Reference ordered scheduler and queue operations over persistent short sequences.
use super::*;
use std::collections::VecDeque;
use tankracer_game::{lifecycle as life, spatial};
fn error(e: life::Error<query::EntityId>) -> i32 {
    match e {
        life::Error::MissingEntity(_) => 6,
        life::Error::InvalidCursor => 5,
        life::Error::Spatial(spatial::Error::InvalidLimit) => 2,
        life::Error::Spatial(spatial::Error::Limit) => 3,
        life::Error::Method(code) => -code,
    }
}
fn list(out: &mut Vec<i32>, values: impl IntoIterator<Item = query::EntityId>) {
    let values = values.into_iter().map(|id| id.0 as i32).collect::<Vec<_>>();
    assert!(values.len() <= 8);
    out.push(values.len() as i32);
    out.extend(&values);
    out.extend(std::iter::repeat_n(-1, 8 - values.len()));
}
pub(super) fn append(cases: &mut Vec<Value>) {
    for variant in 0..3 {
        for id in 0..8 {
            for limit in [0, 1, 2, 20] {
                let mut ticks = life::Ticks {
                    flags: vec![0, 16, 16, 16, if variant == 2 { 16 } else { 0 }, 0],
                    order: if variant == 1 {
                        vec![
                            query::EntityId(1),
                            query::EntityId(2),
                            query::EntityId(2),
                            query::EntityId(3),
                        ]
                    } else {
                        vec![query::EntityId(1), query::EntityId(2), query::EntityId(3)]
                    },
                    next: Some(query::EntityId(if variant == 2 { 4 } else { 2 })),
                };
                let mut queues = life::Queues {
                    notifications: VecDeque::from([query::EntityId(2), query::EntityId(0)]),
                    destruction: VecDeque::from([query::EntityId(4)]),
                };
                let mut input = ticks.flags.iter().map(|v| *v as i32).collect::<Vec<_>>();
                list(&mut input, ticks.order.iter().copied());
                input.push(ticks.next.map_or(-1, |id| id.0 as i32));
                list(&mut input, queues.notifications.iter().copied());
                list(&mut input, queues.destruction.iter().copied());
                let ops = [
                    [0, id, limit, 0],
                    [1, id, limit, 0],
                    [2, id, limit, 0],
                    [3, id, limit, 0],
                    [4, id, limit, 0],
                    [0, 2, 20, 0],
                    [5, 0, limit, 0],
                    [2, 3, 20, 0],
                    [3, 4, 20, 0],
                    [6, 0, limit, 0],
                    [6, 0, 20, 0],
                    [5, 0, 20, 1],
                    [2, 7, 20, 0],
                    [1, 4, 1, 0],
                ];
                input.push(ops.len() as i32);
                for op in ops {
                    input.extend(op);
                }
                assert_eq!(input.len(), 91);
                let mut expected = Vec::new();
                for [op, id, limit, fail] in ops {
                    let id = query::EntityId(id as u32);
                    let limit = limit as u32;
                    let mut visited = Vec::new();
                    let status = match op {
                        0 => ticks.enable(id).map_or_else(error, i32::from),
                        1 => ticks.disable(id, limit).map_or_else(error, i32::from),
                        2 | 3 => queues
                            .queue(&mut ticks, id, op == 3, limit)
                            .map_or_else(error, |()| 0),
                        4 => queues
                            .cancel_notification(&mut ticks, id, limit)
                            .map_or_else(error, |()| 0),
                        5 => ticks
                            .run(limit, |ticks, id| {
                                visited.push(id);
                                if fail != 0 && id.0 == 1 {
                                    return Err(-7);
                                }
                                if id.0 == 1 {
                                    ticks
                                        .disable(query::EntityId(2), limit)
                                        .map_err(|e| -error(e))?;
                                }
                                if id.0 == 3 {
                                    ticks.enable(query::EntityId(4)).map_err(|e| -error(e))?;
                                }
                                Ok(())
                            })
                            .map_or_else(error, |()| 0),
                        _ => queues
                            .drain(&mut ticks, limit, |queues, ticks, event| {
                                let (id, destroy) = match event {
                                    life::Event::Stop(id) => (id, false),
                                    life::Event::Destroy(id) => (id, true),
                                };
                                visited.push(query::EntityId(id.0 * 2 + u32::from(destroy)));
                                if !destroy {
                                    ticks.disable(id, limit).map_err(|e| -error(e))?;
                                } else if id.0 == 4 {
                                    queues
                                        .queue(ticks, query::EntityId(5), false, limit)
                                        .map_err(|e| -error(e))?;
                                }
                                Ok(())
                            })
                            .map_or_else(error, |()| 0),
                    };
                    expected.push(status);
                    expected.push(ticks.next.map_or(-1, |id| id.0 as i32));
                    expected.extend(ticks.flags.iter().map(|v| *v as i32));
                    list(&mut expected, ticks.order.iter().copied());
                    list(&mut expected, queues.notifications.iter().copied());
                    list(&mut expected, queues.destruction.iter().copied());
                    list(&mut expected, visited);
                }
                assert_eq!(expected.len(), 616);
                cases.push(json!({"kind":18,"input":input,"status":0,"expected":expected}));
            }
        }
    }
}
