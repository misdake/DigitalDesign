//! Qualification-only call to the original closing movement implementation.
use super::*;
pub fn advance(
    state: &mut crate::closer::State,
    matrix: &mut crate::math::Matrix,
    dt: i32,
    limit: u32,
    owner: &mut crate::entity_factory::State<Handle>,
    entity: crate::object_query::EntityId,
) -> Result<(), i32> {
    super::advance_closer(state, matrix, dt, limit, &mut owner.context(), entity)
}
