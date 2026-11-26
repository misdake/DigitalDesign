//! Shared temporary effects and all line-pool aliases after each original call.
use super::{ints, wide};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{effect_pool, vehicle_scene::temporary};

fn line(out: &mut Vec<u16>, v: &effect_pool::Line) {
    ints(out, v.points.into_iter().flatten());
    for color in v.colors {
        wide(out, u32::from_le_bytes(color) as i32);
    }
}
pub fn snapshot(
    state: &temporary::State,
    pool: &effect_pool::Pool,
    slot: i16,
    seed: u32,
) -> Vec<u16> {
    let mut out = Vec::new();
    wide(&mut out, state.available as i32);
    ints(&mut out, state.timestamps.map(|v| v as i32));
    ints(&mut out, state.lines.iter().flatten().map(|v| v.0 as i32));
    for row in state.rows.iter().flatten() {
        ints(&mut out, row.first);
        ints(&mut out, row.last);
        out.extend([
            u16::from(row.colors[0]) | (u16::from(row.colors[1]) << 8),
            u16::from(row.colors[2]) | (u16::from(row.colors[3]) << 8),
        ]);
        out.extend(row.phases);
    }
    assert_eq!(out.len(), 274);
    ints(
        &mut out,
        [i32::from(slot), seed as i32, pool.line_count as i32],
    );
    for v in &pool.lines {
        line(&mut out, v);
    }
    line(&mut out, &pool.overflow);
    let q = &pool.quads[159];
    ints(
        &mut out,
        [q.points[3][1], q.points[3][2], q.material as i32],
    );
    for b in q.style.compat_bytes().as_chunks::<4>().0 {
        wide(&mut out, u32::from_le_bytes(*b) as i32);
    }
    for color in pool.exhausted_colors {
        wide(&mut out, u32::from_le_bytes(color) as i32);
    }
    assert_eq!(out.len(), 1912);
    out
}
pub fn export(out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..24 {
        let mut state = temporary::State::default();
        let mut pool = effect_pool::Pool::default();
        let mut slot = 0i16;
        let mut seed = 0x19876543;
        let position = [12345, -23456, 34567];
        let mut controls = vec![
            [0, 0, 100, 12345, -23456, 34567],
            [1, 6000, 101, 12345, -23456, 34567],
            [0, 6000, 102, -1000, 2000, -3000],
            [1, 1, 103, -1000, 2000, -3000],
            [2, 0, 104, 0, 0, 0],
            [3, 0, 105, 0, 0, 0],
            [0, 0, 106, 12345, -23456, 34567],
        ];
        match branch {
            0 => {}
            1 => pool.line_count = 100,
            2 => {
                for l in &mut pool.lines {
                    l.points[0][1] = 0;
                }
                pool.line_count = 0;
            }
            3 => {
                for _ in 0..97 {
                    pool.allocate([[0; 3]; 2], [[0; 4]; 2]);
                }
            }
            4 => {
                state.available = u32::MAX;
                state.timestamps = [u32::MAX, 0x80000000, 0x7fffffff, 1];
            }
            5 => state.available = 4,
            6 => state.available = u32::MAX - 1,
            7 | 8 => {
                slot = if branch == 7 { -1 } else { 4 };
                controls = vec![
                    [0, 1, 100, 0, 0, 0],
                    [1, 1, 101, 0, 0, 0],
                    [2, 0, 102, 0, 0, 0],
                ];
            }
            9..=17 => {
                state
                    .start(
                        0,
                        &mut slot,
                        position,
                        1,
                        temporary::Context {
                            pool: &mut pool,
                            random: &mut seed,
                        },
                    )
                    .unwrap();
                state.lines[slot as usize][(branch - 9) % 9] = effect_pool::LineHandle(999);
                controls = vec![[1, 1, 2, 123, 234, 345], [2, 0, 3, 0, 0, 0]];
            }
            18 => {
                state.available = u32::MAX;
                state.timestamps = [1; 4];
                state.lines = [[effect_pool::LineHandle::EXHAUSTED; 9]; 4];
                pool.line_count = 100;
            }
            19 => {
                controls = (0..6)
                    .map(|time| [0, 0, time, 12345, -23456, 34567])
                    .collect();
            }
            20 => {
                controls = (1..7)
                    .map(|time| [0, 0, time, 12345, -23456, 34567])
                    .collect();
            }
            21 => {
                slot = -1;
                controls = vec![[1, 0, 1, 0, 0, 0], [0, 0, 2, i32::MIN, i32::MAX, -100]];
            }
            22 => {
                state.timestamps = [0x80000000, u32::MAX, 1, 0x7fffffff];
                state.available = u32::MAX;
            }
            _ => {
                pool.line_count = u32::MAX;
                state.available = u32::MAX;
                state.timestamps = [0; 4];
            }
        }
        let input = snapshot(&state, &pool, slot, seed);
        let mut frames = Vec::new();
        for c in &controls {
            let [op, timer, time, x, y, z] = *c;
            let status = match op {
                0 => state.start(
                    timer as i16,
                    &mut slot,
                    [x, y, z],
                    time as u32,
                    temporary::Context {
                        pool: &mut pool,
                        random: &mut seed,
                    },
                ),
                1 => state.tick(
                    timer as i16,
                    slot,
                    [x, y, z],
                    temporary::Context {
                        pool: &mut pool,
                        random: &mut seed,
                    },
                ),
                2 => state.expire(slot, &mut pool),
                3 => {
                    state.reset_timestamps();
                    Ok(())
                }
                _ => panic!("temporary operation"),
            }
            .map_or_else(|v| v, |()| 0);
            let mut answer = Vec::new();
            wide(&mut answer, status);
            answer.extend(snapshot(&state, &pool, slot, seed));
            frames.push(answer);
        }
        cases.push(json!({"tag":format!("temporary-{branch}"),"input":input,"controls":controls,"frames":frames}));
    }
    fs::write(out.join("temporary.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original shared temporary start/tick/expire/reset, all four rows, complete line pool and quad-tail aliases, seed and error prefixes"})).unwrap()).unwrap();
}
