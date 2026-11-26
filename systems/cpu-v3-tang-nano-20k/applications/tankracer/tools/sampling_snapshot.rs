//! Stable semantic fields for terrain sampling qualification.
use tankracer_assets::terrain::TerrainData;
use tankracer_game::contact_pipeline as ground;
pub fn values(s: &ground::Sampling, data: &TerrainData) -> Vec<i32> {
    let mut out = Vec::new();
    for (slot, p) in s.primary.points.iter().enumerate() {
        out.extend(p.local);
        out.extend(p.world);
        out.extend([
            p.kind,
            p.gap,
            p.previous_gap,
            p.raw_height,
            p.travel,
            p.flags as i32,
        ]);
        out.extend(p.normal);
        // Surface and source-node tags follow the corresponding point.
        out.push(s.primary.surfaces[slot]);
        out.push(s.primary.nodes[slot].map_or(-1, |id| data.node(id).source_offset as i32));
    }
    for p in &s.auxiliary {
        out.extend(p.world);
        out.extend([p.kind, p.surface, p.cached]);
    }
    let e = &s.effect;
    out.extend([
        e.timer,
        e.restore,
        e.active,
        e.phase,
        e.impulse[0],
        e.impulse[1],
        e.pose,
        i32::from(e.clear_player_slot),
    ]);
    let p = &s.primary;
    out.extend([
        p.load,
        p.airborne_count,
        p.special_count,
        p.flagged_count,
        i32::from(p.has_extra),
    ]);
    out.extend(p.extra);
    out.push(p.flagged_ground_count);
    out.extend(s.materials);
    out.extend([
        s.fallback_mode,
        i32::from(s.feedback_required),
        s.clear_player_slot.map_or(-1, i32::from),
    ]);
    assert_eq!(out.len(), 103);
    out
}
