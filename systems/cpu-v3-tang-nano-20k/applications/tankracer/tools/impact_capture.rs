//! Capture canonical impact state for source-oracle comparison.
use crate::dsl_rt::*;
use crate::{attitude, effect_pool, impact, integer};
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
    let saved = effect_pool::select(&context.pool);
    while j < 1600 {
        dest[16 + j] = context.pool.lines[j];
        j += 1;
    }
    effect_pool::restore(saved);
    let overflow = unsafe { view_of(&mut context.pool.overflow).as_ptr().as_u16_array() };
    j = 0;
    while j < 16 {
        dest[1616 + j] = overflow[j];
        dest[1632 + j] = context.pool.exhausted[j];
        j += 1;
    }
    let mut cursor = 824u16;
    let batch_page = impact::select(state);
    i = 0;
    while i < 4 {
        let batch = &mut state.batches[i];
        write_batch(batch, unsafe {
            out.as_ptr().add((cursor * 2) as i16).as_u16_array()
        });
        cursor += 6;
        j = 0;
        while j < 10 {
            let particle = &mut batch.particles[j];
            write_particle(particle, unsafe {
                out.as_ptr().add((cursor * 2) as i16).as_u16_array()
            });
            cursor += 11;
            j += 1;
        }
        i += 1;
    }
    effect_pool::restore(batch_page);
}

// Row writers also support bounded streaming comparison without a full image.
pub fn write_batch(batch: &mut impact::Batch, out: Array<u16>) {
    attitude::put(out, 0, batch.kind);
    attitude::put(out, 1, batch.corner);
    attitude::put(out, 2, batch.owner);
    let mut active = fix32::zero();
    if batch.active {
        active = integer::from_i16(1);
    }
    attitude::put(out, 3, active);
    attitude::put(out, 4, batch.age);
    attitude::put(out, 5, batch.frame);
}
pub fn write_particle(particle: &mut impact::Particle, out: Array<u16>) {
    attitude::put(out, 0, integer::from_i16(particle.distance[0u16]));
    attitude::put(out, 1, integer::from_i16(particle.distance[1u16]));
    let mut k = 0u16;
    while k < 2 {
        attitude::put(out, 2 + k, attitude::get(particle.speed.as_array(), k));
        attitude::put(out, 4 + k, attitude::get(particle.pitch.as_array(), k));
        attitude::put(out, 6 + k, attitude::get(particle.yaw.as_array(), k));
        attitude::put(
            out,
            9 + k,
            fix32::from_words(particle.colors[k * 2], particle.colors[k * 2 + 1]),
        );
        k += 1;
    }
    attitude::put(out, 8, particle.line);
}
