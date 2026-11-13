//! Explicit application fixture inputs; labels only identify output directories.
use super::harness::{ApplicationCase, SourcePatch};
use super::*;

fn edited(entry: &'static str, edit: impl FnOnce(String) -> String + 'static) -> ApplicationCase {
    ApplicationCase {
        entry,
        edit: Box::new(edit),
        ..ApplicationCase::default()
    }
}

mod parts;
pub(super) use parts::{button_parts, cache_level, static_cache};
mod timing;
pub(super) use timing::{cpu_cost, cpu_shadow, moving, render_time};
mod fragile;
pub(super) use fragile::{
    atlas_sizes, fragile_atlas, fragile_flow, fragile_ordinary, fragile_pad, fragile_play,
    fragile_representative,
};
mod rendering;
pub(super) use rendering::{
    cached_veil, gallery, goal_close, goal_mid, hint_order, orientation, rerecord, stage,
};
mod shadow;
use rendering::stage_source;
pub(super) use shadow::{frame_schedule, shadow_cache, shadow_diagnostic, web_shadow};
