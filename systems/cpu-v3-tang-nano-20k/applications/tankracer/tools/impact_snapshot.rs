//! Stable diagnostic capture of the original shared impact state.
use tankracer_game::{effect_pool, impact_particles as impact};
fn packed_color(c: [u8; 4]) -> i32 {
    u32::from_le_bytes(c) as i32
}
fn line_row(line: &effect_pool::Line) -> Vec<i32> {
    line.points
        .into_iter()
        .flatten()
        .chain(line.colors.map(packed_color))
        .collect()
}
pub fn snapshot(
    phases: [i32; 4],
    state: &impact::State,
    v: &tankracer_game::vehicle::State,
    pool: &effect_pool::Pool,
    seed: u32,
) -> Vec<i32> {
    let mut expected = phases.to_vec();
    expected.extend([
        seed as i32,
        state.next as i32,
        v.impact_cooldown as i32,
        pool.line_count as i32,
    ]);
    for l in &pool.lines {
        expected.extend(line_row(l));
    }
    expected.extend(line_row(&pool.overflow));
    let q = &pool.quads[159];
    let style = q.style.compat_bytes();
    expected.extend([q.points[3][1], q.points[3][2], q.material as i32]);
    for b in style.as_chunks::<4>().0 {
        expected.push(u32::from_le_bytes(*b) as i32);
    }
    expected.extend(pool.exhausted_colors.map(packed_color));
    for batch in &state.batches {
        expected.extend([
            batch.kind,
            batch.corner as i32,
            batch.owner.map_or(-1, |e| e.0 as i32),
            i32::from(batch.active),
            batch.age,
            batch.frame as i32,
        ]);
        for p in &batch.particles {
            expected.extend(p.distance.map(i32::from));
            expected.extend(p.speed);
            expected.extend(p.pitch);
            expected.extend(p.yaw);
            expected.push(p.line.0 as i32);
            expected.extend(
                p.colors
                    .map(|rgb| packed_color([rgb[0], rgb[1], rgb[2], 0])),
            );
        }
    }
    expected
}
