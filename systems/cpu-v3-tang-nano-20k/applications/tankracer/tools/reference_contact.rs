//! Qualification access to the source sampler's synchronous game-owned writes.
use super::*;
pub fn sample(
    state: &mut Sampling,
    world: &World<'_>,
    env: &Environment,
    seed: &mut u32,
    tails: &mut [u32; 2],
    enabled: bool,
) -> Result<(), Error> {
    state.sample_with_effects(world, env, seed, None, |slot| {
        if !enabled {
            return true;
        }
        if let Some(tail) = tails.get_mut(usize::from(slot)) {
            *tail = 0;
            true
        } else {
            false
        }
    })
}
