//! Stable per-part pad Models; only geometry changes invalidate a world pose.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod game_assets;
use game_assets::*;

mod boot_resources;
use boot_resources::*;
mod session;
use session::*;
mod texture_views;
use texture_views::*;
mod render_stats;
use render_stats::*;

// Caller-owned aperture state and the last recorded drawing keys.
#[repr(C)]
pub struct PadCache {
    pub generation: u16,
    pub entry_key: u16,
    pub goal_key: u16,
    pub prepared_mask: u16,
    pub dirty_mask: u16,
    pub visible_mask: u16,
    pub goal_frame: u16,
    pub entry_open: fix32,
    pub goal_open: fix32,
    pub recorded_entry_key: u16,
    pub recorded_goal_key: u16,
    pub recorded_visible: u16,
    pub recorded_draws: u16,
}
impl PadCache {
    pub const fn new() -> Self {
        Self {
            generation: 0,
            entry_key: 0,
            goal_key: 0,
            prepared_mask: 0,
            dirty_mask: 0,
            visible_mask: 0,
            goal_frame: 0,
            entry_open: fix32::zero(),
            goal_open: fix32::zero(),
            recorded_entry_key: 0,
            recorded_goal_key: 0,
            recorded_visible: 0,
            recorded_draws: 0,
        }
    }
}

pub fn goal_carrier_enabled() -> bool {
    BLOX_LEVEL_HEADERS[GAME_SESSION.motion.level].carrier != 0
}
fn opening(start: bool, mode: AnimationPhase, t: u16) -> fix32 {
    let one = fix32::from_int(1);
    let mut value = fix32::from_words(56361, 0);
    if (start && mode == AnimationPhase::EntryOpen)
        || (!start && (mode == AnimationPhase::GoalOpen || mode == AnimationPhase::GoalClose))
    {
        let f = fraction(t, 200);
        let eased = f * f * (fix32::from_int(3) - fix32::from_int(2) * f);
        if mode == AnimationPhase::EntryOpen {
            value = eased;
        } else if mode == AnimationPhase::GoalOpen {
            value = fix32::from_words(56361, 0) + fix32::from_words(5243, 0) * eased;
        } else {
            value = fix32::from_words(61604, 0) * (one - eased);
        }
    } else if !start && (mode == AnimationPhase::GoalDrop || mode == AnimationPhase::GoalLand) {
        value = fix32::from_words(61604, 0);
    }
    value
}
pub fn pad_cache_update(cache: &mut PadCache) {
    let mode = GAME_SESSION.visual_mode();
    let t = GAME_SESSION.visual_time();
    if cache.generation != GAME_SESSION.generation {
        cache.generation = GAME_SESSION.generation;
        cache.entry_key = 65535;
        cache.goal_key = 65535;
        cache.prepared_mask = 0;
    }
    cache.dirty_mask = 0;
    let entry_key = if mode == AnimationPhase::EntryOpen {
        t + 1
    } else {
        0u16
    };
    let mut goal_key: u16 = 0;
    if mode == AnimationPhase::GoalOpen {
        goal_key = t + 1;
    } else if mode == AnimationPhase::GoalClose {
        goal_key = t + 202;
    } else if mode == AnimationPhase::GoalDrop || mode == AnimationPhase::GoalLand {
        goal_key = 403;
    }
    if cache.entry_key != entry_key {
        let value = opening(true, mode, t);
        cache.entry_key = entry_key;
        cache.entry_open = value;
        cache.dirty_mask |= 120; // entrance strips3..6
    }
    if cache.goal_key != goal_key {
        let value = opening(false, mode, t);
        cache.goal_key = goal_key;
        cache.goal_open = value;
        cache.goal_frame = (value * fix32::from_int(50) + fix!(0.5)).to_int() as u16;
        cache.dirty_mask |= 1920; // fallback exit strips7..10
    }
    let one = fix32::from_int(1);
    let half = fix!(0.5);
    let mut visible: u16 = 0;
    if mode != AnimationPhase::EntryOpen && mode != AnimationPhase::EntryRise {
        visible |= 1;
    }
    let entry = cache.entry_open;
    if mode == AnimationPhase::EntryOpen && (one - entry) * half >= fix32::from_words(128, 0) {
        visible |= 24;
        if entry > fix32::from_words(6, 0) {
            visible |= 96;
        }
    }
    if GAME_SESSION.animation.goal_x != 65535 {
        if !mode.goal_aperture_open() {
            visible |= 2;
        }
        if goal_carrier_enabled() {
            visible |= 4;
        } else {
            let goal = cache.goal_open;
            if (one - goal) * half >= fix32::from_words(128, 0) {
                visible |= 384;
                if goal > fix32::from_words(6, 0) {
                    visible |= 1536;
                }
            }
        }
    }
    cache.visible_mask = visible;
}

pub fn pad_part_material(role: u16) -> Material {
    let mut color = Rgba8::white();
    let mut unlit = true;
    if role == 0 {
        color = Rgba8 {
            r: 67,
            g: 99,
            b: 79,
            a: 255,
        };
        unlit = false;
    } else if role == 1 {
        color = Rgba8 {
            r: 87,
            g: 50,
            b: 47,
            a: 255,
        };
        unlit = false;
    } else if role == 3 {
        color = Rgba8 {
            r: 58,
            g: 165,
            b: 115,
            a: 255,
        };
    } else if role == 7 {
        color = Rgba8 {
            r: 224,
            g: 88,
            b: 88,
            a: 255,
        };
    }
    game_material(color, unlit)
}
pub fn pad_carrier_texture() -> Texture {
    let phase = (GAME_SESSION.animation.goal_x * 3 + GAME_SESSION.animation.goal_z * 5) & 3;
    Texture::from_native(TextureDesc {
        format: TextureFormat::Rgb565,
        memory: unsafe { GpuMemory::from_sectors(60416 + phase * 256, 256) },
        size_log2: 7,
        full_mip: false,
    })
}
// Keep the borrowed view and role together within the six-parameter ABI.
#[derive(Copy, Clone)]
pub struct PadPart {
    pub cache: Array<PadCache>,
    pub role: u16,
}

pub fn draw_pad_part(
    recording: &mut PreparedPart,
    box_mesh: &Mesh,
    model: &mut Model,
    material: &Material,
    view: &TextureView,
    part: PadPart,
) {
    let mut cache = part.cache;
    let role = part.role;
    let bit = 1 << role;
    if cache[0u16].visible_mask & bit == 0 {
        return;
    }
    if cache[0u16].prepared_mask & bit == 0 || cache[0u16].dirty_mask & bit != 0 {
        let start = role == 0 || (role >= 3 && role <= 6);
        let x = if start {
            GAME_SESSION.level.start_x
        } else {
            GAME_SESSION.animation.goal_x
        };
        let z = if start {
            GAME_SESSION.level.start_z
        } else {
            GAME_SESSION.animation.goal_z
        };
        let one = fix32::from_int(1);
        let half = fix!(0.5);
        let cx = fix32::from_int(x as i16)
            - fix32::from_int(GAME_SESSION.level.width as i16) * half
            + half;
        let cz = fix32::from_int(z as i16)
            - fix32::from_int(GAME_SESSION.level.height as i16) * half
            + half;
        if role < 3 {
            let scale = if role == 2 || (role == 1 && goal_carrier_enabled()) {
                half
            } else {
                fix32::from_words(32604, 0)
            };
            model.set_pose(Transform::new(
                Vec3::new(cx, fix32::zero(), cz),
                Vec3::zero(),
                Vec3::new(scale, half, scale),
            ));
        } else {
            let panel = if start { role - 3 } else { role - 7 };
            let value = if start {
                cache[0u16].entry_open
            } else {
                cache[0u16].goal_open
            };
            let along_x = panel < 2;
            let sign = if panel & 1 == 0 { -one } else { one };
            let offset = sign * (one + value) * fix32::from_words(16384, 0);
            let width = (one - value) * half;
            model.set_pose(Transform::new(
                Vec3::new(
                    cx + if along_x { offset } else { fix32::zero() },
                    fix32::from_words(524, 0),
                    cz + if along_x { fix32::zero() } else { offset },
                ),
                Vec3::zero(),
                Vec3::new(
                    if along_x { width } else { value },
                    fix32::from_words(1966, 0),
                    if along_x { one } else { width },
                ),
            ));
        }
        cache[0u16].prepared_mask |= bit;
    }
    if role >= 3 {
        counted_part_draw(recording, box_mesh, model, material, view);
    } else {
        let mut first = TILE0_FIRST;
        if role == 2 {
            first = GOAL_CARRIER_FIRST + cache[0u16].goal_frame;
        } else if role == 1 && goal_carrier_enabled() {
            first = GOAL_HATCH_FIRST + cache[0u16].goal_frame;
        }
        let mesh = game_mesh_part(first, 1);
        counted_part_draw(recording, &mesh, model, material, view);
    }
}

pub fn draw_pad_well(pass: &mut Pass, model: &mut Model, material: &mut Material, start: bool) {
    let x = if start {
        GAME_SESSION.level.start_x
    } else {
        GAME_SESSION.animation.goal_x
    };
    let z = if start {
        GAME_SESSION.level.start_z
    } else {
        GAME_SESSION.animation.goal_z
    };
    if x == 65535 {
        return;
    }
    let half = fix!(0.5);
    let texture = game_texture();
    let view = clamped_texture_view(&texture);
    let mesh = game_mesh_part(WELL_FIRST, WELL_COUNT);
    if start {
        material.set_uniform_color(Rgba8 {
            r: 36,
            g: 71,
            b: 58,
            a: 255,
        });
    } else {
        material.set_uniform_color(Rgba8 {
            r: 87,
            g: 50,
            b: 47,
            a: 255,
        });
    }
    model.set_pose(Transform::new(
        Vec3::new(
            fix32::from_int(x as i16) - fix32::from_int(GAME_SESSION.level.width as i16) * half
                + half,
            fix32::zero(),
            fix32::from_int(z as i16) - fix32::from_int(GAME_SESSION.level.height as i16) * half
                + half,
        ),
        Vec3::zero(),
        Vec3::one(),
    ));
    counted_draw(pass, &mesh, model, material, &view);
}
