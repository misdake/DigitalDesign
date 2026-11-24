//! All-stage Bloxorz with reusable static geometry commands and cached RTT transitions.
use crate::dsl_rt::*;
mod bloxorz_core;
use bloxorz_core::TileKind;
mod gpu_api;
use gpu_api::*;
mod scene_resources;
use scene_resources::*;
mod ui_text;
use ui_text::*;
mod game_assets;
use game_assets::*;
mod level_assets;
use level_assets::*;
mod static_buffer;
use static_buffer::StaticBuffer;
#[static_buffer]
const PREPARED_STORAGE: StaticBuffer = StaticBuffer::new(65536, 128);
mod gpu_board;
use gpu_board::*;
mod time;
use time::*;
mod gamepad;
use gamepad::*;
mod boot_resources;
use boot_resources::*;
mod session;
use session::*;
mod shadow;
use shadow::*;
mod texture_views;
use texture_views::*;
mod render_stats;
use render_stats::*;
mod ground_batches;
use ground_batches::*;
mod fragile_batches;
use fragile_batches::*;
mod pad_cache;
use pad_cache::*;

struct HudState {
    enabled: u16,
    previous_draws: u16,
    valid: u16,
}
// One caller-owned block Model lives for this run; retain only its resting key.
struct RestingBlockKey {
    cell: u16,
    generation: u16,
    orientation: u16,
}
struct BridgeCache {
    generation: u16,
    mask: u16,
    opaque_draws: u16,
    button_storage: u16,
    transparent_draws: u16,
}

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum BackgroundMode {
    Solid = 0,
    Dithered = 1,
}

struct PresentationState {
    hud: HudState,
    background: BackgroundMode,
    resting_block: RestingBlockKey,
    bridges: BridgeCache,
}
static RENDER_STATE: PresentationState = PresentationState {
    hud: HudState {
        enabled: 0,
        previous_draws: 0,
        valid: 0,
    },
    background: BackgroundMode::Dithered,
    resting_block: RestingBlockKey {
        cell: 0,
        generation: 0,
        orientation: 0,
    },
    bridges: BridgeCache {
        generation: 0,
        mask: 65535,
        opaque_draws: 0,
        button_storage: 0,
        transparent_draws: 0,
    },
};
fn scene_color_load(
    commands: &mut CommandBuffer,
    source: &ColorBuffer,
    target: ColorBufferPtr,
) -> ColorLoad {
    if RENDER_STATE.background == BackgroundMode::Dithered {
        commands.copy_color(source.ptr(), target);
        return ColorLoad::load();
    }
    ColorLoad::clear(Rgb565::from_raw(0x2924))
}
fn channel_color(channel: u16, shade: u16) -> Rgba8 {
    let rg = blox_palette_word(channel, shade, 0);
    Rgba8 {
        r: rg as u8,
        g: (rg >> 8) as u8,
        b: blox_palette_word(channel, shade, 1) as u8,
        a: 255,
    }
}
fn bit_channel(mut bit: u16) -> u16 {
    let mut channel: u16 = 0;
    while bit > 1 {
        bit >>= 1;
        channel += 1;
    }
    channel
}
fn tile_pose(index: u16, y: fix32) -> Transform {
    let half = fix!(0.5);
    Transform::new(
        Vec3::new(
            fix32::from_int((index % GAME_SESSION.level.width) as i16)
                - fix32::from_int(GAME_SESSION.level.width as i16) * half
                + half,
            y,
            fix32::from_int(div_u16(index, GAME_SESSION.level.width) as i16)
                - fix32::from_int(GAME_SESSION.level.height as i16) * half
                + half,
        ),
        Vec3::zero(),
        Vec3::new(half, half, half),
    )
}

fn draw_ground_batches(pass: &mut Pass, model: &mut Model, material: &mut Material) {
    let half = fix!(0.5);
    let y_scale = if ground_global_quantization() {
        fix!(0.125)
    } else {
        half
    };
    model.set_pose(Transform::new(
        Vec3::zero(),
        Vec3::zero(),
        Vec3::new(half, y_scale, half),
    ));
    if ground_global_quantization() {
        // All source face normals are axial. This normalized inverse-transpose
        // has the original directions/magnitudes without overflowing normal18.
        let zero = fix32::zero();
        let normal = frcp(half);
        let world = model.world();
        model.set_matrices(
            world,
            Mat3x3 {
                row0: Vec3 {
                    x: normal,
                    y: zero,
                    z: zero,
                },
                row1: Vec3 {
                    x: zero,
                    y: normal,
                    z: zero,
                },
                row2: Vec3 {
                    x: zero,
                    y: zero,
                    z: normal,
                },
            },
        );
    }
    material.set_unlit(false);
    let atlas = game_texture();
    let view = clamped_texture_view(&atlas);
    let marker = marker_texture();
    let marker_view = clamped_texture_view(&marker);
    let mut group: u16 = 0;
    while group < 3 {
        if ground_count(group) != 0 {
            let mesh = ground_mesh(group);
            if group == 0 {
                material.set_uniform_color(Rgba8 {
                    r: 82,
                    g: 100,
                    b: 121,
                    a: 255,
                });
            } else if group == 1 {
                material.set_uniform_color(Rgba8::white());
            } else {
                material.set_uniform_color(Rgba8 {
                    r: 139,
                    g: 92,
                    b: 246,
                    a: 255,
                });
            }
            if group == 1 {
                counted_draw(pass, &mesh, model, material, &marker_view);
            } else {
                counted_draw(pass, &mesh, model, material, &view);
            }
        }
        group += 1;
    }
}
fn draw_floor(pass: &mut Pass, model: &mut Model, material: &mut Material) {
    if ground_enabled() {
        draw_ground_batches(pass, model, material);
        return;
    }
    material.set_uniform_color(Rgba8 {
        r: 82,
        g: 100,
        b: 121,
        a: 255,
    });
    let texture = game_texture();
    let view = clamped_texture_view(&texture);
    let marker_texture = marker_texture();
    let marker_view = clamped_texture_view(&marker_texture);
    let half = fix!(0.5);
    let carrier = goal_carrier_enabled();
    let count = level_face_count();
    let mut i: u16 = 0;
    while i < count {
        let face = level_face(i);
        let x = face & 31;
        let z = (face >> 5) & 31;
        let index = z * GAME_SESSION.level.width + x;
        let gx = GAME_SESSION.animation.goal_x;
        let gz = GAME_SESSION.animation.goal_z;
        let direction = (face >> 10) & 7;
        let inside = x + 1 >= gx && x <= gx + 1 && z + 1 >= gz && z <= gz + 1;
        if carrier
            && inside
            && (direction == 0
                || (direction == 1 && x < gx + 1)
                || (direction == 2 && z < gz + 1)
                || (direction == 3 && x + 1 > gx)
                || (direction == 4 && z + 1 > gz))
        {
            i += 1;
            continue;
        }
        let top = ((face >> 10) & 7) == 0
            && GAME_SESSION.level_field(index, 0) == TileKind::Splitter as u16;
        let mesh = game_mesh_part(
            if top {
                MARKED_SPLIT_FIRST + (face >> 13)
            } else {
                SHARED_FLOOR_FIRST + (face >> 13) * 5 + ((face >> 10) & 7)
            },
            1,
        );
        model.set_pose(Transform::new(
            Vec3::new(
                fix32::from_int(x as i16) - fix32::from_int(GAME_SESSION.level.width as i16) * half
                    + half,
                fix32::zero(),
                fix32::from_int(z as i16)
                    - fix32::from_int(GAME_SESSION.level.height as i16) * half
                    + half,
            ),
            Vec3::zero(),
            Vec3::new(half, half, half),
        ));
        material.set_unlit(false);
        if top {
            material.set_uniform_color(Rgba8::white());
            counted_draw(pass, &mesh, model, material, &marker_view);
        } else {
            if GAME_SESSION.level_field(index, 0) == TileKind::Splitter as u16 {
                material.set_uniform_color(Rgba8 {
                    r: 139,
                    g: 92,
                    b: 246,
                    a: 255,
                });
            } else {
                material.set_uniform_color(Rgba8 {
                    r: 82,
                    g: 100,
                    b: 121,
                    a: 255,
                });
            }
            counted_draw(pass, &mesh, model, material, &view);
        }
        i += 1;
    }
    model.set_pose(Transform::new(
        Vec3::zero(),
        Vec3::zero(),
        Vec3::new(half, half, half),
    ));
}

// Generation, bridge mask, opaque count, button-list owner, transparent count.
fn button_cache_begin() {
    let owner = malloc(47);
    RENDER_STATE.bridges.button_storage = owner.addr();
    let mut cache = unsafe { owner.as_u16_array() };
    cache[0u16] = 0;
}
fn button_cache_end() {
    free(Ptr::from_addr(RENDER_STATE.bridges.button_storage));
    RENDER_STATE.bridges.button_storage = 0;
}
fn button_cache_data() -> Array<u16> {
    unsafe { Ptr::from_addr(RENDER_STATE.bridges.button_storage).as_u16_array() }
}
fn button_cache_rebuild() {
    let mut cache = button_cache_data();
    let mut count: u16 = 0;
    let mut i: u16 = 0;
    while i < GAME_SESSION.level.cell_count {
        let kind = GAME_SESSION.level_field(i, 0);
        if kind == TileKind::SoftSwitch as u16 || kind == TileKind::HeavySwitch as u16 {
            if count >= 13 {
                halt(0xbad);
            }
            cache[1 + count * 3] = i;
            cache[2 + count * 3] = 65535;
            cache[3 + count * 3] = 0;
            count += 1;
        }
        i += 1;
    }
    cache[0u16] = count;
}
fn sync_bridge_group(parts: Array<PreparedDrawsPtr>) {
    if RENDER_STATE.bridges.generation != GAME_SESSION.generation {
        let mut i: u16 = 0;
        while i < 16 {
            let part = parts[i];
            part.invalidate();
            i += 1;
        }
        button_cache_rebuild();
        RENDER_STATE.bridges.generation = GAME_SESSION.generation;
        RENDER_STATE.bridges.mask = GAME_SESSION.visual_mask();
    } else if RENDER_STATE.bridges.mask != GAME_SESSION.visual_mask() {
        let opaque = parts[0u16];
        opaque.invalidate();
        let hints = parts[15u16];
        hints.invalidate();
        RENDER_STATE.bridges.mask = GAME_SESSION.visual_mask();
    }
}
// The stage owns the mapping: opaque bridges0, up to thirteen switches,
// pads14 and translucent bridge hints15.
fn prepare_bridges(parts: Array<PreparedDrawsPtr>, scene: &mut SceneResources, transparent: bool) {
    let part = if transparent { 15u16 } else { 0u16 };
    if parts[part].ready() {
        return;
    }
    let before = DRAW_STATS.draw_calls;
    let mut recording = parts[part].record();
    if transparent {
        recording.set_state(scene.hint_state.ptr());
    } else {
        recording.set_state(scene.state.ptr());
    }
    draw_bridges(
        &mut recording,
        &mut scene.floor_model,
        &mut scene.floor_material,
        transparent,
    );
    recording.finish();
    let count = DRAW_STATS.draw_calls - before;
    if transparent {
        RENDER_STATE.bridges.transparent_draws = count;
    } else {
        RENDER_STATE.bridges.opaque_draws = count;
    }
    DRAW_STATS.draw_calls = before;
}

fn visible_tile_side(i: u16) -> u16 {
    let eye = BLOX_LEVEL_HEADERS[GAME_SESSION.motion.level].eye.x;
    let half = fix!(0.5);
    let center = fix32::from_int((i % GAME_SESSION.level.width) as i16)
        - fix32::from_int(GAME_SESSION.level.width as i16) * half
        + half;
    if eye < center {
        1u16
    } else {
        0u16
    }
}
fn draw_bridges(
    recording: &mut PreparedPart,
    model: &mut Model,
    material: &mut Material,
    transparent: bool,
) {
    let texture = game_texture();
    let view = clamped_texture_view(&texture);
    let mut i: u16 = 0;
    while i < GAME_SESSION.level.cell_count {
        let selected = if transparent {
            GAME_SESSION.visual_mask() & GAME_SESSION.level_field(i, 1) == 0
        } else {
            GAME_SESSION.visual_mask() & GAME_SESSION.level_field(i, 1) != 0
        };
        if GAME_SESSION.level_field(i, 0) == TileKind::Bridge as u16 && selected {
            let bit = GAME_SESSION.level_field(i, 1);
            let active = GAME_SESSION.visual_mask() & bit != 0;
            let variant =
                ((i % GAME_SESSION.level.width) * 3 + div_u16(i, GAME_SESSION.level.width) * 5) % 4;
            let mesh = game_mesh_part(
                if active {
                    BRIDGE_TILE_FIRST + variant + visible_tile_side(i) * 4
                } else {
                    HINT_TILE_FIRST
                },
                1,
            );
            let mut color = channel_color(bit_channel(bit), 1);
            if !active {
                color.a = 60;
            }
            material.set_uniform_color(color);
            model.set_pose(tile_pose(i, fix32::zero()));
            counted_part_draw(recording, &mesh, model, material, &view);
        }
        i += 1;
    }
}
fn marker_texture() -> Texture {
    Texture::from_native(TextureDesc {
        format: TextureFormat::Rgb565,
        memory: unsafe { GpuMemory::from_sectors(BLOX_MARKER_SECTOR, BLOX_MARKER_SECTORS) },
        size_log2: BLOX_MARKER_LOG2,
        full_mip: false,
    })
}
fn draw_fragile_batches(pass: &mut Pass, model: &mut Model, material: &mut Material, owner: Ptr) {
    if fragile_count(owner) == 0 {
        return;
    }
    let texture = marker_texture();
    let view = clamped_texture_view(&texture);
    material.set_unlit(false);
    material.set_uniform_color(Rgba8::white());
    let half = fix!(0.5);
    model.set_pose(Transform::new(
        Vec3::zero(),
        Vec3::zero(),
        Vec3::new(half, half, half),
    ));
    let mesh = fragile_mesh(owner);
    counted_draw(pass, &mesh, model, material, &view);
}
fn draw_switch_tile(
    recording: &mut PreparedPart,
    model: &mut Model,
    material: &mut Material,
    i: u16,
    kind: u16,
    channel: u16,
) {
    let marked = marker_texture();
    let marked_view = clamped_texture_view(&marked);
    material.set_unlit(false);
    let one = fix32::from_int(1);
    if kind == TileKind::SoftSwitch as u16 || kind == TileKind::HeavySwitch as u16 {
        let bit = 1u16 << channel;
        let active = GAME_SESSION.visual_mask() & bit != 0;
        let mut sink = fix32::zero();
        if kind == TileKind::SoftSwitch as u16 && active {
            sink = fix32::from_words(3277, 0);
            let f = GAME_SESSION.switch_fraction(channel);
            if f < one {
                let sc = fsincos(f * fix32::from_words(9280, 3));
                let wave = sc.x();
                let remain = one - f;
                sink = fix32::from_words(3277, 0) * (one - remain * remain * remain)
                    + fix32::from_words(1311, 0) * wave;
            }
        }
        if kind == TileKind::SoftSwitch as u16 {
            let mesh = game_mesh_part(
                MARKED_SOFT_FIRST
                    + (channel & 7) * 2
                    + if active { 1u16 } else { 0u16 }
                    + visible_tile_side(i) * 16,
                1,
            );
            let mut y = fix32::from_words(2621, 0) - sink;
            if y < fix32::from_words(983, 0) {
                y = fix32::from_words(983, 0);
            }
            material.set_uniform_color(Rgba8::white());
            model.set_pose(tile_pose(i, y));
            counted_part_draw(recording, &mesh, model, material, &marked_view);
        } else {
            let base = game_mesh_part(
                MARKED_HEAVY_FIRST + (channel & 7) * 2 + if active { 1u16 } else { 0u16 },
                1,
            );
            material.set_uniform_color(Rgba8::white());
            model.set_pose(tile_pose(i, fix32::zero()));
            counted_part_draw(recording, &base, model, material, &marked_view);
        }
    }
}
fn prepare_switches(
    parts: Array<PreparedDrawsPtr>,
    scene: &mut SceneResources,
    material: &mut Material,
) {
    let mut cache = button_cache_data();
    let mask = GAME_SESSION.visual_mask();
    let mut j: u16 = 0;
    while j < cache[0u16] {
        let i = cache[1 + j * 3];
        let kind = GAME_SESSION.level_field(i, 0);
        let bit = GAME_SESSION.level_field(i, 1);
        let channel = bit_channel(bit);
        let key = if mask & bit != 0 {
            32768
                + if kind == TileKind::HeavySwitch as u16 {
                    0u16
                } else {
                    GAME_SESSION.switch_age(channel)
                }
        } else {
            0u16
        };
        let part = j + 1;
        if !parts[part].ready() || cache[2 + j * 3] != key {
            let before = DRAW_STATS.draw_calls;
            let mut recording = parts[part].record();
            recording.set_state(scene.state.ptr());
            draw_switch_tile(
                &mut recording,
                &mut scene.pad_model,
                material,
                i,
                kind,
                channel,
            );
            recording.finish();
            cache[2 + j * 3] = key;
            cache[3 + j * 3] = DRAW_STATS.draw_calls - before;
            DRAW_STATS.draw_calls = before;
        }
        j += 1;
    }
}

fn draw_switches(pass: &mut Pass, parts: Array<PreparedDrawsPtr>) {
    let cache = button_cache_data();
    let mut j: u16 = 0;
    while j < cache[0u16] {
        pass.draw_prepared(parts[j + 1]);
        DRAW_STATS.draw_calls += cache[3 + j * 3];
        j += 1;
    }
}
fn prepare_block(model: &mut Model) {
    if !GAME_SESSION.block_visible() {
        RENDER_STATE.resting_block.cell = 0;
        return;
    }
    let resting =
        GAME_SESSION.visual_mode() == AnimationPhase::Play && GAME_SESSION.motion.moving == 0;
    let key = (GAME_SESSION.rules.x & 31)
        | ((GAME_SESSION.rules.z & 31) << 5)
        | ((GAME_SESSION.rules.orientation & 3) << 10)
        | 32768;
    if GAME_SESSION.split_visual()
        || !resting
        || RENDER_STATE.resting_block.cell != key
        || RENDER_STATE.resting_block.generation != GAME_SESSION.generation
        || RENDER_STATE.resting_block.orientation != GAME_SESSION.block_orientation()
    {
        GAME_SESSION.block_pose(model);
    }
    RENDER_STATE.resting_block.cell = if resting && !GAME_SESSION.split_visual() {
        key
    } else {
        0u16
    };
    RENDER_STATE.resting_block.generation = GAME_SESSION.generation;
    RENDER_STATE.resting_block.orientation = GAME_SESSION.block_orientation();
}

fn draw_block(pass: &mut Pass, block: &Mesh, model: &mut Model, material: &Material) {
    if GAME_SESSION.block_visible() {
        let texture = game_texture();
        let view = clamped_texture_view(&texture);
        counted_draw(pass, block, model, material, &view);
    }
}
fn level_eye(which: u16) -> Vec3 {
    BLOX_LEVEL_HEADERS[which].eye
}
fn level_far(which: u16) -> fix32 {
    BLOX_LEVEL_HEADERS[which].far
}
fn level_projection(which: u16) -> Projection {
    Projection::from_focal_pixels(BLOX_LEVEL_HEADERS[which].focal, level_far(which))
}
fn game_camera() -> Camera {
    let mut camera = Camera::look_at(
        level_eye(0),
        Vec3::new(fix32::zero(), fix!(0.6), fix32::zero()),
        Vec3::new(fix32::zero(), fix32::from_int(1), fix32::zero()),
        level_projection(0),
    );
    web_camera_axes(&mut camera);
    camera
}
fn web_camera_axes(camera: &mut Camera) {
    // Native scene preparation reflects horizontal view coordinates. Undo that
    // convention here so source geometry uses Three's +X screen-right basis.
    let v = camera.view();
    camera.set_view(Mat3x4 {
        row0: Vec4 {
            x: -v.row0.x,
            y: -v.row0.y,
            z: -v.row0.z,
            w: -v.row0.w,
        },
        row1: v.row1,
        row2: v.row2,
    });
}
#[inline(never)]
fn finish_frame(gpu: &mut Gpu, commands: CommandBuffer, started: bool) -> CommandBuffer {
    gpu.submit(commands);
    let completed = gpu.wait();
    let elapsed = time_microseconds();
    render_time_complete(elapsed, started);
    completed
}
struct SceneTarget {
    color: ColorBufferPtr,
    depth: DepthBufferPtr,
}
fn record_static_scene(
    commands: &mut CommandBuffer,
    target: &SceneTarget,
    background: &ColorBuffer,
    camera: &Camera,
    fragile: Ptr,
) {
    let state = game_solid_state();
    let mut floor_model = game_floor_model();
    let mut floor_material = game_material(Rgba8::white(), false);
    let mut pad_model = game_plain_model();
    let mut pad_material = game_material(Rgba8::white(), false);
    let load = scene_color_load(commands, background, target.color);
    let mut pass = commands.pass(target.color, target.depth, load, DepthLoad::clear(65535));
    pass.set_camera(camera.ptr());
    pass.set_state(state.ptr());
    draw_floor(&mut pass, &mut floor_model, &mut floor_material);
    draw_pad_well(&mut pass, &mut pad_model, &mut pad_material, true);
    draw_pad_well(&mut pass, &mut pad_model, &mut pad_material, false);
    draw_fragile_batches(&mut pass, &mut floor_model, &mut floor_material, fragile);
}

fn draw_hud(pass: &mut Pass, grid: &mut UiGrid) {
    grid.write(
        CellRect {
            x: 0,
            y: 0,
            width: 6,
            height: 1,
        },
        b"LEVEL ",
    );
    text_u16_field(grid, 6, 0, 2, GAME_SESSION.motion.level + 1);
    if RENDER_STATE.hud.enabled != 0 {
        grid.write(
            CellRect {
                x: 0,
                y: 1,
                width: 5,
                height: 1,
            },
            b"DRAW ",
        );
        grid.write(
            CellRect {
                x: 0,
                y: 2,
                width: 3,
                height: 1,
            },
            b"MS ",
        );
        if RENDER_STATE.hud.valid != 0 {
            text_u16_field(grid, 5, 1, 4, RENDER_STATE.hud.previous_draws);
        } else {
            grid.write(
                CellRect {
                    x: 5,
                    y: 1,
                    width: 4,
                    height: 1,
                },
                b"----",
            );
        }
        draw_render_time(grid, 3, 2);
        counted_ui(
            pass,
            grid,
            CellRect {
                x: 0,
                y: 0,
                width: 9,
                height: 3,
            },
        );
    } else {
        counted_ui(
            pass,
            grid,
            CellRect {
                x: 0,
                y: 0,
                width: 8,
                height: 1,
            },
        );
    }
}

struct FrameClock {
    previous: u16,
}
fn advance_gameplay(clock: &mut FrameClock) {
    gamepad_capture();
    let input = gamepad_take_frame();
    if !GAME_SESSION.fading() && input.pressed & 8192 != 0 {
        RENDER_STATE.hud.enabled ^= 1;
    }
    if !GAME_SESSION.fading() && input.pressed & 1 != 0 {
        RENDER_STATE.background = if RENDER_STATE.background == BackgroundMode::Dithered {
            BackgroundMode::Solid
        } else {
            BackgroundMode::Dithered
        };
    }
    let now = time_milliseconds();
    let elapsed = now - clock.previous;
    clock.previous = now;
    let buttons = input.pressed;
    let held = input.buttons & 240;
    GAME_SESSION.tick(buttons, held, elapsed);
}

struct StaticScene {
    background: ColorBuffer,
    valid: bool,
    generation: u16,
    background_mode: BackgroundMode,
    hidden: u16,
    draws: u16,
}
impl StaticScene {
    fn new() -> Self {
        let background = ColorBuffer::from_memory(unsafe {
            GpuMemory::from_sectors(BLOX_BACKGROUND_SECTOR, 1600)
        });
        Self {
            background: background,
            valid: false,
            generation: 0,
            background_mode: BackgroundMode::Dithered,
            hidden: 65535,
            draws: 0,
        }
    }
}
fn refresh_static_scene(
    cache: &mut StaticScene,
    mut commands: CommandBuffer,
    dynamic_commands: &mut CommandBuffer,
    camera: &Camera,
    fragile_owner: Ptr,
    target: &SceneTarget,
) -> CommandBuffer {
    if cache.generation != GAME_SESSION.generation {
        commands.reset();
        cache.valid = false;
        level_geometry_load(GAME_SESSION.motion.level);
        if ground_enabled() && !ground_matches_level() {
            dynamic_commands.reset();
            ground_rebuild();
        }
        if !fragile_matches_level(fragile_owner) {
            dynamic_commands.reset();
            fragile_rebuild(fragile_owner);
        }
    }
    // Geometry changes only after both preceding CBs retired. Visible fragile
    // coverage is part of the recording key, not a per-frame command patch.
    fragile_sync(fragile_owner);
    if cache.background_mode != RENDER_STATE.background
        || cache.hidden != fragile_hidden(fragile_owner)
    {
        cache.valid = false;
    }
    if cache.valid {
        let binding = commands.target_binding();
        commands.retarget_target(binding, target.color, target.depth);
        DRAW_STATS.draw_calls += cache.draws;
    } else {
        commands.reset();
        let previous_draws = DRAW_STATS.draw_calls;
        record_static_scene(
            &mut commands,
            target,
            &cache.background,
            camera,
            fragile_owner,
        );
        cache.valid = true;
        cache.draws = DRAW_STATS.draw_calls - previous_draws;
        cache.generation = GAME_SESSION.generation;
        cache.background_mode = RENDER_STATE.background;
        cache.hidden = fragile_hidden(fragile_owner);
    }
    commands
}

fn prepare_pads(
    parts: Array<PreparedDrawsPtr>,
    scene: &mut SceneResources,
    mut pad_cache: Array<PadCache>,
    pad_view: &TextureView,
    carrier_view: &TextureView,
) {
    if parts[14u16].ready()
        && pad_cache[0u16].recorded_entry_key == pad_cache[0u16].entry_key
        && pad_cache[0u16].recorded_goal_key == pad_cache[0u16].goal_key
        && pad_cache[0u16].recorded_visible == pad_cache[0u16].visible_mask
    {
        return;
    }
    let before = DRAW_STATS.draw_calls;
    let mut recording = parts[14u16].record();
    recording.set_state(scene.state.ptr());
    draw_pad_part(
        &mut recording,
        &scene.pad,
        &mut scene.entry_hatch_model,
        &scene.entry_hatch_material,
        pad_view,
        PadPart {
            cache: pad_cache,
            role: 0,
        },
    );
    draw_pad_part(
        &mut recording,
        &scene.pad,
        &mut scene.entry0,
        &scene.entry_ring_material,
        pad_view,
        PadPart {
            cache: pad_cache,
            role: 3,
        },
    );
    draw_pad_part(
        &mut recording,
        &scene.pad,
        &mut scene.entry1,
        &scene.entry_ring_material,
        pad_view,
        PadPart {
            cache: pad_cache,
            role: 4,
        },
    );
    draw_pad_part(
        &mut recording,
        &scene.pad,
        &mut scene.entry2,
        &scene.entry_ring_material,
        pad_view,
        PadPart {
            cache: pad_cache,
            role: 5,
        },
    );
    draw_pad_part(
        &mut recording,
        &scene.pad,
        &mut scene.entry3,
        &scene.entry_ring_material,
        pad_view,
        PadPart {
            cache: pad_cache,
            role: 6,
        },
    );
    draw_pad_part(
        &mut recording,
        &scene.pad,
        &mut scene.goal_hatch_model,
        &scene.goal_hatch_material,
        pad_view,
        PadPart {
            cache: pad_cache,
            role: 1,
        },
    );
    draw_pad_part(
        &mut recording,
        &scene.pad,
        &mut scene.carrier_model,
        &scene.carrier_material,
        carrier_view,
        PadPart {
            cache: pad_cache,
            role: 2,
        },
    );
    draw_pad_part(
        &mut recording,
        &scene.pad,
        &mut scene.goal0,
        &scene.goal_ring_material,
        pad_view,
        PadPart {
            cache: pad_cache,
            role: 7,
        },
    );
    draw_pad_part(
        &mut recording,
        &scene.pad,
        &mut scene.goal1,
        &scene.goal_ring_material,
        pad_view,
        PadPart {
            cache: pad_cache,
            role: 8,
        },
    );
    draw_pad_part(
        &mut recording,
        &scene.pad,
        &mut scene.goal2,
        &scene.goal_ring_material,
        pad_view,
        PadPart {
            cache: pad_cache,
            role: 9,
        },
    );
    draw_pad_part(
        &mut recording,
        &scene.pad,
        &mut scene.goal3,
        &scene.goal_ring_material,
        pad_view,
        PadPart {
            cache: pad_cache,
            role: 10,
        },
    );
    recording.finish();
    pad_cache[0u16].recorded_entry_key = pad_cache[0u16].entry_key;
    pad_cache[0u16].recorded_goal_key = pad_cache[0u16].goal_key;
    pad_cache[0u16].recorded_visible = pad_cache[0u16].visible_mask;
    pad_cache[0u16].recorded_draws = DRAW_STATS.draw_calls - before;
    DRAW_STATS.draw_calls = before;
}

fn record_dynamic_scene(
    pass: &mut Pass,
    scene: &mut SceneResources,
    pad_cache: &PadCache,
    parts: Array<PreparedDrawsPtr>,
) {
    pass.set_camera(scene.camera.ptr());
    pass.set_state(scene.state.ptr());
    prepare_block(&mut scene.block_model);
    pass.draw_prepared(parts[0u16]);
    DRAW_STATS.draw_calls += RENDER_STATE.bridges.opaque_draws;
    draw_switches(pass, parts);
    pass.draw_prepared(parts[14u16]);
    DRAW_STATS.draw_calls += pad_cache.recorded_draws;

    pass.set_state(scene.shadow_state.ptr());
    draw_shadow(
        pass,
        &scene.shadow_mesh,
        &mut scene.shadow_model,
        &scene.shadow_material,
        &scene.block_model,
    );
    if GAME_SESSION.split_visual() {
        GAME_SESSION.idle_cube_pose(&mut scene.idle_model);
        draw_shadow(
            pass,
            &scene.shadow_mesh,
            &mut scene.shadow_model,
            &scene.shadow_material,
            &scene.idle_model,
        );
    }
    pass.set_state(scene.state.ptr());
    draw_block(pass, &scene.block, &mut scene.block_model, &scene.material);
    if GAME_SESSION.split_visual() {
        draw_block(
            pass,
            &scene.block,
            &mut scene.idle_model,
            &scene.idle_material,
        );
    }
    // Opaque geometry and receiver shadows are complete. Inactive
    // bridge hints now test opaque depth without writing it.
    pass.draw_prepared(parts[15u16]);
    DRAW_STATS.draw_calls += RENDER_STATE.bridges.transparent_draws;
}

struct FrameOverlay {
    offscreen_view: TextureView,
    tint_material: Material,
    previous_tint: u16,
}
fn record_overlay(
    pass: &mut Pass,
    scene: &SceneResources,
    overlay: &mut FrameOverlay,
    grid: &mut UiGrid,
    fade: bool,
    color: u16,
) {
    if fade {
        if overlay.previous_tint != color {
            overlay.tint_material.set_uniform_color(Rgba8 {
                r: 255,
                g: 255,
                b: 255,
                a: color as u8,
            });
            overlay.previous_tint = color;
        }
        pass.set_camera(scene.screen_camera.ptr());
        pass.set_state(scene.screen_state.ptr());
        counted_draw(
            pass,
            &scene.screen,
            &scene.screen_model,
            &overlay.tint_material,
            &overlay.offscreen_view,
        );
    }
    draw_hud(pass, grid);
}

fn run(limit: u16) {
    let mut gpu = Gpu::init(board_gpu_config());
    let mut commands = gpu.command_buffer();
    let mut prepared_pool = PreparedPool::new(GpuMemory::from_buffer(PREPARED_STORAGE));
    let first = prepared_pool.allocate(1024, 8192);
    let mut parts: Buf<PreparedDrawsPtr, 16> = Buf::new([first; 16]);
    let mut index: u16 = 1;
    while index < 16 {
        let mut push_bytes: u16 = 2048;
        if index == 15 {
            push_bytes = 8192;
        }
        parts[index] = prepared_pool.allocate(1024, push_bytes);
        index += 1;
    }
    let mut scene_commands = gpu.command_buffer();
    button_cache_begin();
    RENDER_STATE.bridges.generation = 0;
    RENDER_STATE.bridges.mask = 65535;
    let ground_owner = ground_begin();
    let shadow_owner = shadow_batch_begin();
    let fragile_owner = fragile_begin();
    let mut snapshot_generation: u16 = 0;
    let mut snapshot_valid = false;
    let offscreen = ColorBuffer::from_memory(unsafe { GpuMemory::from_sectors(0xc000, 2048) });
    let offscreen_view = offscreen.texture_view();
    let color_a = ColorBuffer::from_surface(BOARD_COLOR_A);
    let color_b = ColorBuffer::from_surface(BOARD_COLOR_B);
    let depth = board_depth();
    let mut static_scene = StaticScene::new();
    let mut display = Display::open();
    let camera = game_camera();
    let mut camera_level: u16 = 0;
    let mut scene = SceneResources::new(camera);
    let mut pad_cache = PadCache::new();
    let pad_texture = game_texture();
    let pad_view = clamped_texture_view(&pad_texture);
    let mut hatch_material = game_material(Rgba8::white(), false);
    let mut overlay = FrameOverlay {
        offscreen_view: offscreen_view,
        tint_material: game_material(Rgba8::white(), true),
        previous_tint: 65535,
    };
    let mut grid = game_ui_grid();
    GAME_SESSION.begin();
    pad_cache_update(&mut pad_cache);
    let mut carrier_texture = pad_carrier_texture();
    let mut carrier_view = clamped_texture_view(&carrier_texture);
    shadow_invalidate();
    RENDER_STATE.resting_block.cell = 0;
    gamepad_init();
    let mut clock = FrameClock {
        previous: time_milliseconds(),
    };
    let mut render_started = false;
    let mut number: u16 = 0;
    let mut back_b = true;
    loop {
        shadow_batch_frame();
        signal(4, number);
        advance_gameplay(&mut clock);
        if camera_level != GAME_SESSION.motion.level {
            static_scene.valid = false;
            camera_level = GAME_SESSION.motion.level;
            scene.camera.set_look_at_y_up(
                level_eye(camera_level),
                Vec3::new(fix32::zero(), fix!(0.6), fix32::zero()),
            );
            scene.camera.set_projection(level_projection(camera_level));
            web_camera_axes(&mut scene.camera);
        }
        let fade = GAME_SESSION.fading();
        let color = if fade { GAME_SESSION.tint() } else { 255u16 };
        let back = if back_b { &color_b } else { &color_a };
        let target = if fade { &offscreen } else { back };
        let refresh_snapshot =
            fade && (!snapshot_valid || snapshot_generation != GAME_SESSION.generation);
        let prepare_scene = !fade || refresh_snapshot;
        if !fade {
            snapshot_valid = false;
        }
        DRAW_STATS.draw_calls = 0;
        if prepare_scene {
            commands.reset();
            sync_bridge_group(parts.as_array());
            if pad_cache.generation != GAME_SESSION.generation {
                drop(carrier_view);
                drop(carrier_texture);
                carrier_texture = pad_carrier_texture();
                carrier_view = clamped_texture_view(&carrier_texture);
            }
            pad_cache_update(&mut pad_cache);
            let scene_target = SceneTarget {
                color: target.ptr(),
                depth: depth.ptr(),
            };
            scene_commands = refresh_static_scene(
                &mut static_scene,
                scene_commands,
                &mut commands,
                &scene.camera,
                fragile_owner,
                &scene_target,
            );
            gpu.submit(scene_commands);
            commands.reset();
            prepare_bridges(parts.as_array(), &mut scene, false);
            prepare_switches(parts.as_array(), &mut scene, &mut hatch_material);
            prepare_pads(
                parts.as_array(),
                &mut scene,
                view_of(&mut pad_cache),
                &pad_view,
                &carrier_view,
            );
            prepare_bridges(parts.as_array(), &mut scene, true);
            {
                let mut pass = commands.pass(
                    target.ptr(),
                    depth.ptr(),
                    ColorLoad::load(),
                    DepthLoad::load(),
                );
                record_dynamic_scene(&mut pass, &mut scene, &pad_cache, parts.as_array());
            }
            // The other CB is CPU-owned while the immutable static CB runs.
            // Its Load dependency is checked only after static completion,
            // when it is submitted; recording does not reclaim that producer.
            scene_commands = gpu.wait();
            if fade {
                gpu.submit(commands);
                commands = gpu.wait();
                snapshot_valid = true;
                snapshot_generation = GAME_SESSION.generation;
                GAME_SESSION.captured();
                signal(11, GAME_SESSION.animation.captured_frames);
            }
        }
        if fade {
            commands.reset();
            if refresh_snapshot {
                commands.invalidate_textures();
            }
        }
        let color_load: ColorLoad = if fade {
            ColorLoad::clear(Rgb565::from_raw(0x1107))
        } else {
            ColorLoad::load()
        };
        {
            let mut pass = commands.pass(back.ptr(), depth.ptr(), color_load, DepthLoad::load());
            record_overlay(&mut pass, &scene, &mut overlay, &mut grid, fade, color);
        }
        commands = finish_frame(&mut gpu, commands, render_started);
        RENDER_STATE.hud.previous_draws = DRAW_STATS.draw_calls;
        RENDER_STATE.hud.valid = 1;
        display.set_next_swap(back.ptr());
        display.wait_vblank();
        time_clear_microseconds();
        render_started = true;
        gamepad_capture();
        signal(12, GAME_SESSION.animation.phase as u16);
        GAME_SESSION.presented();
        back_b = !back_b;
        number += 1;
        if limit != 0 && number == limit {
            break;
        }
    }
    drop(scene_commands);
    drop(commands);
    ground_end(ground_owner);
    shadow_batch_end(shadow_owner);
    fragile_end(fragile_owner);
    button_cache_end();
    GAME_SESSION.end();
}

pub fn renderer_game_frames() {
    run(120);
    halt(0x600d);
}
fn main() {
    run(0);
}
pub fn renderer_game_three_frames() {
    run(3);
    halt(0x600d);
}
