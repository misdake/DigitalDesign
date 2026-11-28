//! Ordered vehicle effects on the same particle, pool, owner and random state.
use crate::dsl_rt::*;
use crate::{
    impact, integer, pickup_light, quad_pool, temporary, track_effects, vehicle_debris,
    vehicle_shield, vehicle_smoke,
};
pub struct State {
    pub spray: track_effects::Spray,
    pub marks: Array<u16>,
    pub smoke: Array<u16>,
    pub lights: Array<u16>,
}
pub struct Shared {
    pub pickup: pickup_light::State,
    pub impacts: Array<impact::State>,
    pub debris: Array<u16>,
    pub shield: Array<u16>,
    pub quads: Array<quad_pool::Pool>,
    pub temporary: temporary::State,
    pub owners: vehicle_debris::Owners,
    pub own: u16,
    pub owner_slots: u16,
    pub impact_accumulator: Array<u16>,
    pub sound: Array<u16>,
}
pub struct Input {
    pub special: bool,
    pub spray: track_effects::SprayInput,
    pub marks: track_effects::MarksInput,
    pub smoke: vehicle_smoke::Input,
    pub lights: track_effects::LightInput,
    pub debris: vehicle_debris::Input,
    pub shield: vehicle_shield::Input,
    pub base_position: Array<u16>,
    pub number: fix32,
    pub temporary_timer: i16,
    pub temporary_slot: i16,
}
pub struct Tuning {
    pub spray: track_effects::SprayTuning,
    pub smoke_sockets: Array<u16>,
    pub pickup: Array<u16>,
}
fn n(x: i16) -> fix32 {
    integer::from_i16(x)
}
pub fn impact_status(status: u16) -> fix32 {
    if status == 0 {
        return n(0);
    }
    if status == 2 {
        return n(-2);
    }
    if status == 3 {
        return n(-1);
    }
    n(-4)
}
// Inputs are borrowed views of the current car. No persistent state is copied
// between calls; the context's line pool is also the temporary effect's pool.
pub fn tick(
    state: &State,
    shared: &mut Shared,
    input: &Input,
    tuning: &Tuning,
    vehicle: &mut impact::Vehicle,
    context: &mut impact::Context,
) -> fix32 {
    let mut impact_states = shared.impacts;
    let impacts = &mut impact_states[0u16];
    let mut quad_states = shared.quads;
    let quads = &mut quad_states[0u16];
    if !input.special || !input.smoke.player {
        let status = track_effects::spray(&state.spray, &input.spray, &tuning.spray);
        if status != n(0) {
            return status;
        }
        let status = track_effects::marks(state.marks, &input.marks, quads, &context.pool);
        if status != n(0) {
            return status;
        }
    }
    let status = pickup_light::fade(
        &shared.pickup,
        &pickup_light::Frame {
            target: vehicle.entity,
            position: input.base_position,
            number: input.number,
            dt: input.smoke.dt,
        },
        tuning.pickup,
        quads,
        &context.pool,
    );
    if status != n(0) {
        return status;
    }
    let status = impact_status(impact::tick(impacts, vehicle, input.smoke.dt, context));
    if status != n(0) {
        return status;
    }
    let status = vehicle_smoke::step(
        state.smoke,
        &input.smoke,
        tuning.smoke_sockets,
        impacts,
        vehicle,
        context,
    );
    if status != n(0) {
        return status;
    }
    if shared.owner_slots > 9 {
        return n(-1);
    }
    let status = vehicle_debris::step(
        shared.debris,
        &shared.owners,
        shared.own,
        &input.debris,
        &vehicle_debris::Context {
            kinds: tuning.spray.kinds,
            seed: context.seed,
            impact_accumulator: shared.impact_accumulator,
            sound: shared.sound,
        },
    );
    if status != n(0) {
        return status;
    }
    let status = track_effects::lights(state.lights, &input.lights, quads, &context.pool);
    if status != n(0) {
        return status;
    }
    let status = temporary::tick(
        &shared.temporary,
        input.temporary_timer,
        input.temporary_slot,
        input.base_position,
        &temporary::Context {
            pool: view_of(&mut context.pool),
            seed: context.seed,
            sine: context.sine,
        },
    );
    if status != n(0) {
        return status;
    }
    vehicle_shield::tick(shared.shield, &input.shield, context.seed, context.sine)
}
