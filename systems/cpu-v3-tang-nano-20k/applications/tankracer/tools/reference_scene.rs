//! Qualification-only snapshots of original component pool ownership.
use super::*;
pub fn clear_terrain(components: &mut Components) {
    components.terrain = None;
}
pub fn lists(components: &Components) -> [Vec<i32>; 5] {
    [
        components.free_slots.iter().map(|id| id.0 as i32).collect(),
        components
            .lap_objects
            .iter()
            .map(|id| id.0 as i32)
            .collect(),
        components.signs.iter().map(|id| id.0 as i32).collect(),
        components
            .lap_events
            .enter
            .iter()
            .map(|id| id.0 as i32)
            .collect(),
        components
            .lap_events
            .leave
            .iter()
            .map(|id| id.0 as i32)
            .collect(),
    ]
}
