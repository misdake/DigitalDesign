//! Capture canonical impact state for source-oracle comparison.
use crate::dsl_rt::*;
use crate::{attitude, impact, integer};
pub fn write(
    state: &mut impact::State,
    vehicle: &impact::Vehicle,
    context: &mut impact::Context,
    out: Array<u16>,
) {
    let mut i = 0u16;
    attitude::put(out, 4, attitude::get(context.seed, 0));
    attitude::put(out, 5, state.next);
    attitude::put(out, 6, attitude::get(vehicle.cooldown, 0));
    attitude::put(out, 7, context.pool.count);
    let mut dest = out;
    let mut j = 0u16;
    while j < 1600 {
        dest[16 + j] = context.pool.lines[j];
        j += 1;
    }
    let overflow = unsafe { view_of(&mut context.pool.overflow).as_ptr().as_u16_array() };
    j = 0;
    while j < 16 {
        dest[1616 + j] = overflow[j];
        dest[1632 + j] = context.pool.exhausted[j];
        j += 1;
    }
    let mut cursor = 824u16;
    i = 0;
    while i < 4 {
        let batch = &mut state.batches[i];
        attitude::put(out, cursor, batch.kind);
        attitude::put(out, cursor + 1, batch.corner);
        attitude::put(out, cursor + 2, batch.owner);
        let mut active = fix32::zero();
        if batch.active {
            active = integer::from_i16(1);
        }
        attitude::put(out, cursor + 3, active);
        attitude::put(out, cursor + 4, batch.age);
        attitude::put(out, cursor + 5, batch.frame);
        cursor += 6;
        j = 0;
        while j < 10 {
            let particle = &mut batch.particles[j];
            attitude::put(out, cursor, integer::from_i16(particle.distance[0u16]));
            attitude::put(out, cursor + 1, integer::from_i16(particle.distance[1u16]));
            let mut k = 0u16;
            while k < 2 {
                attitude::put(
                    out,
                    cursor + 2 + k,
                    attitude::get(particle.speed.as_array(), k),
                );
                attitude::put(
                    out,
                    cursor + 4 + k,
                    attitude::get(particle.pitch.as_array(), k),
                );
                attitude::put(
                    out,
                    cursor + 6 + k,
                    attitude::get(particle.yaw.as_array(), k),
                );
                attitude::put(
                    out,
                    cursor + 9 + k,
                    fix32::from_words(particle.colors[k * 2], particle.colors[k * 2 + 1]),
                );
                k += 1;
            }
            attitude::put(out, cursor + 8, particle.line);
            cursor += 11;
            j += 1;
        }
        i += 1;
    }
}
