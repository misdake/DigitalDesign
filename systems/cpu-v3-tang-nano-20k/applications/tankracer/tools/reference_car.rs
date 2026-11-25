//! Qualification-only access to the original private ground response.
use super::*;
pub fn ground(world: &mut SinglePlayer, id: TankId, dt: i32, number: u32) -> Result<(), i32> {
    let player = world
        .vehicles
        .entries
        .get_mut(id.0)
        .and_then(Option::as_mut)
        .ok_or(-1)?;
    super::ground(
        player,
        &mut world.shared,
        &vehicle::Frame {
            dt,
            number,
            mode: 0,
            input: Default::default(),
            drive: Default::default(),
        },
    )
}
