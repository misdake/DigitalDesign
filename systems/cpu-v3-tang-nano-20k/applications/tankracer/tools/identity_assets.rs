//! Canonical semantic identity rows for loading and differential snapshots.
use tankracer_game::{
    entities::Entities,
    object_query::EntityId,
    scene_entity::{Handle, SessionObject},
};
pub fn encode(entities: &Entities<Handle>) -> Vec<u16> {
    let mut out = Vec::new();
    for v in [
        entities.capacity(),
        entities.free_head().0,
        entities.high_water().0,
        entities.record_count(),
        u32::from(entities.sequential),
        entities.next_requested.0,
    ] {
        out.extend([v as u16, (v >> 16) as u16]);
    }
    for i in 0..entities.capacity() {
        let record = entities.record(EntityId(i));
        let class = record.and_then(|r| r.class);
        let (kind, index) = match record.and_then(|r| r.object) {
            None => (0, 0),
            Some(Handle::Obstacle(id)) => (1, id.0 as u32),
            Some(Handle::Vehicle(id)) => (2, id.0 as u32),
            Some(Handle::Session(SessionObject::Camera)) => (3, 0),
            Some(Handle::Session(SessionObject::FinishOwner)) => (4, 0),
        };
        let flags = u16::from(record.is_some()) | (u16::from(class.is_some()) << 1) | (kind << 2);
        let class = class.map_or(0, |c| u16::try_from(c.0).unwrap());
        let next = entities.next_free(EntityId(i)).unwrap().0;
        out.extend([
            flags,
            class,
            index as u16,
            (index >> 16) as u16,
            next as u16,
            (next >> 16) as u16,
        ]);
    }
    out
}
