//! Semantic pickup banks and complete shared effect pool snapshots.
use tankracer_game::{
    effect_pool::{self, Pool, Quad},
    vehicle_scene::pickup,
};
fn quad(out: &mut Vec<i32>, q: Quad) {
    out.extend(q.points.into_iter().flatten());
    out.push(q.material as i32);
    for b in q.style.compat_bytes().chunks_exact(4) {
        out.push(i32::from_le_bytes(b.try_into().unwrap()));
    }
}
fn line(out: &mut Vec<i32>, l: effect_pool::Line) {
    out.extend(l.points.into_iter().flatten());
    for c in l.colors {
        out.push(i32::from_le_bytes(c));
    }
}
pub fn values(s: &pickup::State, p: &Pool) -> Vec<i32> {
    let mut out = Vec::new();
    for b in &s.banks {
        out.extend([
            i32::from(b.active),
            i32::from(b.projected),
            i32::from(b.follow[0]),
            i32::from(b.follow[1]),
            i32::from(b.target.is_some()),
            b.target.map_or(0, |t| t.0 as i32),
            b.life,
            b.frame as i32,
        ]);
        out.extend(b.origin);
        out.extend(b.position);
        out.extend([b.speed, b.inner_radius, b.outer_radius, b.flare_radius]);
        for r in b.rays {
            out.extend(r.angles.map(i32::from));
            out.extend(r.angular_velocity);
            out.extend([r.palette_index as i32, r.handle.0 as i32]);
            quad(&mut out, r.quad);
        }
        for r in b.rings {
            out.push(r.handle.0 as i32);
            quad(&mut out, r.quad);
        }
    }
    for q in &p.quads[..159] {
        quad(&mut out, *q);
    }
    let mut last = Vec::new();
    quad(&mut last, p.quads[159]);
    out.extend(&last[..10]);
    quad(&mut out, p.exhausted_quad);
    out.extend([p.quad_count as i32, p.line_count as i32]);
    for l in p.lines {
        line(&mut out, l);
    }
    line(&mut out, p.overflow);
    out.extend(&last[10..]);
    for c in p.exhausted_colors {
        out.push(i32::from_le_bytes(c));
    }
    assert_eq!(out.len(), 4204);
    out
}
