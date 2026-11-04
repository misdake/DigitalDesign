//! Stable per-part pad Models; only geometry changes invalidate a world pose.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
use gpu_demo_api::*;
mod boot_resources;
use boot_resources::*;
mod session;
use session::*;
mod render_stats;
use render_stats::*;

// Plain caller-owned cache data, never resource handles. Resource Models remain
// independent lexical owners in run; this value only carries their role/key.
#[derive(Copy, Clone)]
pub struct PadPart {
    pub cache: Array<u16>,
    pub role: u16,
}

pub fn goal_carrier_enabled() -> bool {
    blox_level_meta(MOTION[6u16], 14) != 0
}
fn opening(start: bool, mode: u16, t: u16) -> fix32 {
    let one = fix32::from_int(1);
    let mut value = fix32::from_words(56361, 0);
    if (start && mode == ENTRY_OPEN) || (!start && (mode == GOAL_OPEN || mode == GOAL_CLOSE)) {
        let f = fraction(t, 200);
        let eased = f * f * (fix32::from_int(3) - fix32::from_int(2) * f);
        if mode == ENTRY_OPEN {
            value = eased;
        } else if mode == GOAL_OPEN {
            value = fix32::from_words(56361, 0) + fix32::from_words(5243, 0) * eased;
        } else {
            value = fix32::from_words(61604, 0) * (one - eased);
        }
    } else if !start && (mode == GOAL_DROP || mode == GOAL_LAND) {
        value = fix32::from_words(61604, 0);
    }
    value
}
fn cached_opening(cache: Array<u16>, start: bool) -> fix32 {
    let at = if start { 7u16 } else { 9u16 };
    fix32::from_words(cache[at], cache[at + 1])
}
// generation, entry/goal normalized animation keys, initialized/dirty/visible
// masks, goal mesh index, then the two exact Q16 opening values.
pub fn pad_cache_update(mut cache: Array<u16>) {
    let mode = visual_mode();
    let t = visual_time();
    if cache[0u16] != LEVEL_GENERATION[0u16] {
        cache[0u16] = LEVEL_GENERATION[0u16];
        cache[1u16] = 65535;
        cache[2u16] = 65535;
        cache[3u16] = 0;
    }
    cache[4u16] = 0;
    let entry_key = if mode == ENTRY_OPEN { t + 1 } else { 0u16 };
    let mut goal_key: u16 = 0;
    if mode == GOAL_OPEN {
        goal_key = t + 1;
    } else if mode == GOAL_CLOSE {
        goal_key = t + 202;
    } else if mode == GOAL_DROP || mode == GOAL_LAND {
        goal_key = 403;
    }
    if cache[1u16] != entry_key {
        let value = opening(true, mode, t);
        cache[1u16] = entry_key;
        cache[7u16] = value.lo_bits();
        cache[8u16] = value.hi_bits();
        cache[4u16] |= 120; // entrance strips3..6
    }
    if cache[2u16] != goal_key {
        let value = opening(false, mode, t);
        cache[2u16] = goal_key;
        cache[9u16] = value.lo_bits();
        cache[10u16] = value.hi_bits();
        cache[6u16] = (value * fix32::from_int(50) + fix32::from_words(32768, 0)).to_int() as u16;
        cache[4u16] |= 1920; // fallback exit strips7..10
    }
    let one = fix32::from_int(1);
    let half = fix32::from_words(32768, 0);
    let mut visible: u16 = 0;
    if mode != ENTRY_OPEN && mode != ENTRY_RISE {
        visible |= 1;
    }
    let entry = cached_opening(cache, true);
    if mode == ENTRY_OPEN && (one - entry) * half >= fix32::from_words(128, 0) {
        visible |= 24;
        if entry > fix32::from_words(6, 0) {
            visible |= 96;
        }
    }
    if SESSION[6u16] != 65535 {
        if mode <= GOAL_OPEN || mode > GOAL_CLOSE {
            visible |= 2;
        }
        if goal_carrier_enabled() {
            visible |= 4;
        } else {
            let goal = cached_opening(cache, false);
            if (one - goal) * half >= fix32::from_words(128, 0) {
                visible |= 384;
                if goal > fix32::from_words(6, 0) {
                    visible |= 1536;
                }
            }
        }
    }
    cache[5u16] = visible;
}

pub fn pad_part_material(role: u16) -> Material {
    let mut color = demo_white();
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
    demo_material(color, unlit)
}
pub fn pad_carrier_texture() -> Texture {
    let phase = (SESSION[6u16] * 3 + SESSION[7u16] * 5) & 3;
    Texture::from_native(TextureDesc {
        memory: unsafe { GpuMemory::from_sectors(60416 + phase * 256, 256) },
        size_log2: 7,
        full_mip: false,
    })
}
pub fn draw_pad_part(
    pass: &mut Pass,
    box_mesh: &Mesh,
    model: &mut Model,
    material: &Material,
    view: &TextureView,
    part: PadPart,
) {
    let mut cache = part.cache;
    let role = part.role;
    let bit = 1 << role;
    if cache[5u16] & bit == 0 {
        return;
    }
    if cache[3u16] & bit == 0 || cache[4u16] & bit != 0 {
        let start = role == 0 || (role >= 3 && role <= 6);
        let x = if start { LEVEL[2u16] } else { SESSION[6u16] };
        let z = if start { LEVEL[3u16] } else { SESSION[7u16] };
        let one = fix32::from_int(1);
        let half = fix32::from_words(32768, 0);
        let cx = fix32::from_int(x as i16) - fix32::from_int(LEVEL[0u16] as i16) * half + half;
        let cz = fix32::from_int(z as i16) - fix32::from_int(LEVEL[1u16] as i16) * half + half;
        if role < 3 {
            let scale = if role == 2 || (role == 1 && goal_carrier_enabled()) {
                half
            } else {
                fix32::from_words(32604, 0)
            };
            model.set_pose(demo_pose(
                demo_point(cx, fix32::zero(), cz),
                demo_zero(),
                demo_point(scale, half, scale),
            ));
        } else {
            let panel = if start { role - 3 } else { role - 7 };
            let value = cached_opening(cache, start);
            let along_x = panel < 2;
            let sign = if panel & 1 == 0 { -one } else { one };
            let offset = sign * (one + value) * fix32::from_words(16384, 0);
            let width = (one - value) * half;
            model.set_pose(demo_pose(
                demo_point(
                    cx + if along_x { offset } else { fix32::zero() },
                    fix32::from_words(524, 0),
                    cz + if along_x { fix32::zero() } else { offset },
                ),
                demo_zero(),
                demo_point(
                    if along_x { width } else { value },
                    fix32::from_words(1966, 0),
                    if along_x { one } else { width },
                ),
            ));
        }
        cache[3u16] |= bit;
    }
    if role >= 3 {
        counted_draw(pass, box_mesh, model, material, view);
    } else {
        let mut first = TILE0_FIRST;
        if role == 2 {
            first = GOAL_CARRIER_FIRST + cache[6u16];
        } else if role == 1 && goal_carrier_enabled() {
            first = GOAL_HATCH_FIRST + cache[6u16];
        }
        let mesh = demo_mesh_part(first, 1);
        counted_draw(pass, &mesh, model, material, view);
    }
}

pub fn draw_pad_well(pass: &mut Pass, model: &mut Model, material: &mut Material, start: bool) {
    let x = if start { LEVEL[2u16] } else { SESSION[6u16] };
    let z = if start { LEVEL[3u16] } else { SESSION[7u16] };
    if x == 65535 {
        return;
    }
    let half = fix32::from_words(32768, 0);
    let texture = demo_texture();
    let view = texture.view();
    let mesh = demo_mesh_part(WELL_FIRST, WELL_COUNT);
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
    model.set_pose(demo_pose(
        demo_point(
            fix32::from_int(x as i16) - fix32::from_int(LEVEL[0u16] as i16) * half + half,
            fix32::zero(),
            fix32::from_int(z as i16) - fix32::from_int(LEVEL[1u16] as i16) * half + half,
        ),
        demo_zero(),
        demo_scale(),
    ));
    counted_draw(pass, &mesh, model, material, &view);
}
