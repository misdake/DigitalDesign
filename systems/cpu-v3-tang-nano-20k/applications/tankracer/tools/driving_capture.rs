//! Canonical diagnostic fields shared by real-map driving owners.
use super::reference;
use tankracer_assets::terrain::TerrainData;
use tankracer_game::{contact_pipeline as ground, motion, pose, powertrain, vehicle_prefix::boost};
fn sampling(s: &ground::Sampling, data: &TerrainData) -> Vec<i32> {
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
pub fn state(
    m: &motion::State,
    p: &powertrain::State,
    b: &boost::State,
    g: &ground::Pipeline,
    data: &TerrainData,
) -> Vec<i32> {
    let mut out = reference::attitude_state(m);
    out.extend(reference::spring(
        &g.response,
        g.sampling.primary.load,
        g.pivot,
    ));
    out.extend(reference::drive_fixture::flatten(&p.drive_state(m)));
    out.extend(reference::motion_state(m));
    out.extend(reference::boost_state(b));
    for t in m.poses.tracks {
        out.extend(t.matrix.0);
        out.extend(t.position);
    }
    out.extend(sampling(&g.sampling, data));
    assert_eq!(out.len(), 236);
    out
}
pub fn body_state(m: &motion::State, pose: &pose::Config, gun: &[[i16; 3]]) -> Vec<i32> {
    let a = &m.articulation;
    let l = &m.lean;
    let h = &m.hull;
    let mut out = vec![
        a.recoil,
        a.recoil_rate,
        a.pitch,
        a.pitch_rate,
        l.lateral.position,
        l.lateral.rate,
        l.longitudinal.position,
        l.longitudinal.rate,
    ];
    out.extend(h.position);
    out.extend(h.matrix.0);
    out.extend(h.vertices.into_iter().flatten().map(i32::from));
    out.extend([
        i32::from(h.radius),
        i32::from(m.ready),
        pose.turret_offset[2],
    ]);
    out.extend(gun.iter().flatten().copied().map(i32::from));
    out
}
