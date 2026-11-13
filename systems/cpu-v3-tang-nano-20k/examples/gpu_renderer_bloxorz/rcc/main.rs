//! All-stage Bloxorz with static color/depth restoration and cached RTT transitions.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_api;
use gpu_api::*;
mod scene_resources;
use scene_resources::*;
mod ui_text;
use ui_text::*;
mod game_assets;
use game_assets::*;
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
pub static HUD: Buf<u16, 3> = Buf::new([0; 3]);

pub static BACKGROUND: Buf<u16, 1> = Buf::new([1]);
// One caller-owned block Model lives for this run; retain only its resting key.
static BLOCK_POSE_KEY: Buf<u16, 3> = Buf::new([0; 3]);
fn scene_color_load(
    commands: &mut CommandBuffer,
    source: &ColorBuffer,
    target: &ColorBuffer,
) -> ColorLoad {
    if BACKGROUND[0u16] != 0 {
        commands.copy_color(source.ptr(), target.ptr());
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
            fix32::from_int((index % LEVEL[0u16]) as i16)
                - fix32::from_int(LEVEL[0u16] as i16) * half
                + half,
            y,
            fix32::from_int(div_u16(index, LEVEL[0u16]) as i16)
                - fix32::from_int(LEVEL[1u16] as i16) * half
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
    let count = blox_scene_face_count(MOTION[6u16]);
    let mut i: u16 = 0;
    while i < count {
        let face = blox_scene_face(MOTION[6u16], i);
        let x = face & 31;
        let z = (face >> 5) & 31;
        let index = z * LEVEL[0u16] + x;
        let gx = SESSION[6u16];
        let gz = SESSION[7u16];
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
        let top = ((face >> 10) & 7) == 0 && level_field(index, 0) == 7;
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
                fix32::from_int(x as i16) - fix32::from_int(LEVEL[0u16] as i16) * half + half,
                fix32::zero(),
                fix32::from_int(z as i16) - fix32::from_int(LEVEL[1u16] as i16) * half + half,
            ),
            Vec3::zero(),
            Vec3::new(half, half, half),
        ));
        material.set_unlit(false);
        if top {
            material.set_uniform_color(Rgba8::white());
            counted_draw(pass, &mesh, model, material, &marker_view);
        } else {
            if level_field(index, 0) == 7 {
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
static BRIDGE_COMMANDS: Buf<u16, 5> = Buf::new([0, 65535, 0, 0, 0]);
fn button_cache_begin() {
    let owner = malloc(47);
    BRIDGE_COMMANDS[3u16] = owner.addr();
    let mut cache = unsafe { owner.as_u16_array() };
    cache[0u16] = 0;
}
fn button_cache_end() {
    free(Ptr::from_addr(BRIDGE_COMMANDS[3u16]));
    BRIDGE_COMMANDS[3u16] = 0;
}
fn button_cache_data() -> Array<u16> {
    unsafe { Ptr::from_addr(BRIDGE_COMMANDS[3u16]).as_u16_array() }
}
fn button_cache_rebuild() {
    let mut cache = button_cache_data();
    let mut count: u16 = 0;
    let mut i: u16 = 0;
    while i < LEVEL[6u16] {
        let kind = level_field(i, 0);
        if kind == 3 || kind == 4 {
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
fn sync_bridge_group(commands: &mut CommandBuffer) {
    if BRIDGE_COMMANDS[0u16] != LEVEL_GENERATION[0u16] {
        commands.clear_prepared_group();
        button_cache_rebuild();
        BRIDGE_COMMANDS[0u16] = LEVEL_GENERATION[0u16];
        BRIDGE_COMMANDS[1u16] = visual_mask();
    } else if BRIDGE_COMMANDS[1u16] != visual_mask() {
        commands.invalidate_prepared_part(0);
        commands.invalidate_prepared_part(15);
        BRIDGE_COMMANDS[1u16] = visual_mask();
    }
}
// Part14 is available in every shipped stage (at most13 buttons).
fn begin_pad_group(pass: &mut Pass, mut cache: Array<u16>) -> bool {
    if pass.prepared_part_ready(14)
        && cache[11u16] == cache[1u16]
        && cache[12u16] == cache[2u16]
        && cache[13u16] == cache[5u16]
    {
        pass.append_prepared_part(14);
        DRAW_CALLS[0u16] += cache[14u16];
        return false;
    }
    pass.begin_prepared_part(14);
    cache[11u16] = cache[1u16];
    cache[12u16] = cache[2u16];
    cache[13u16] = cache[5u16];
    cache[14u16] = DRAW_CALLS[0u16];
    true
}
fn end_pad_group(pass: &mut Pass, mut cache: Array<u16>) {
    pass.end_prepared_group();
    cache[14u16] = DRAW_CALLS[0u16] - cache[14u16];
}
// Two independent command parts keep opaque depth ahead of alpha blending.
// Shipped stages use at most13 buttons: parts1..13, pads14, hints15.
fn draw_bridge_group(
    pass: &mut Pass,
    model: &mut Model,
    material: &mut Material,
    state: &RenderState,
    hint_state: &RenderState,
    transparent: bool,
) {
    let part = if transparent { 15u16 } else { 0u16 };
    let count = if transparent { 4u16 } else { 2u16 };
    if pass.prepared_part_ready(part) {
        pass.append_prepared_part(part);
        DRAW_CALLS[0u16] += BRIDGE_COMMANDS[count];
    } else {
        let before = DRAW_CALLS[0u16];
        pass.begin_prepared_part(part);
        if transparent {
            pass.set_state(hint_state.ptr());
        } else {
            pass.set_state(state.ptr());
        }
        draw_bridges(pass, model, material, transparent);
        pass.set_state(state.ptr());
        pass.end_prepared_group();
        BRIDGE_COMMANDS[count] = DRAW_CALLS[0u16] - before;
    }
}

fn visible_tile_side(i: u16) -> u16 {
    let eye = fix32::from_words(
        blox_level_meta(MOTION[6u16], 4),
        blox_level_meta(MOTION[6u16], 5),
    );
    let half = fix!(0.5);
    let center = fix32::from_int((i % LEVEL[0u16]) as i16)
        - fix32::from_int(LEVEL[0u16] as i16) * half
        + half;
    if eye < center {
        1u16
    } else {
        0u16
    }
}
fn draw_bridges(pass: &mut Pass, model: &mut Model, material: &mut Material, transparent: bool) {
    let texture = game_texture();
    let view = clamped_texture_view(&texture);
    let mut i: u16 = 0;
    while i < LEVEL[6u16] {
        let selected = if transparent {
            visual_mask() & level_field(i, 1) == 0
        } else {
            visual_mask() & level_field(i, 1) != 0
        };
        if level_field(i, 0) == 5 && selected {
            let bit = level_field(i, 1);
            let active = visual_mask() & bit != 0;
            let variant = ((i % LEVEL[0u16]) * 3 + div_u16(i, LEVEL[0u16]) * 5) % 4;
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
            counted_draw(pass, &mesh, model, material, &view);
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
    pass: &mut Pass,
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
    if kind == 3 || kind == 4 {
        let bit = 1u16 << channel;
        let active = visual_mask() & bit != 0;
        let mut sink = fix32::zero();
        if kind == 3 && active {
            sink = fix32::from_words(3277, 0);
            let f = switch_fraction(channel);
            if f < one {
                let sc = fsincos(f * fix32::from_words(9280, 3));
                let wave = sc.x();
                let remain = one - f;
                sink = fix32::from_words(3277, 0) * (one - remain * remain * remain)
                    + fix32::from_words(1311, 0) * wave;
            }
        }
        if kind == 3 {
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
            counted_draw(pass, &mesh, model, material, &marked_view);
        } else {
            let base = game_mesh_part(
                MARKED_HEAVY_FIRST + (channel & 7) * 2 + if active { 1u16 } else { 0u16 },
                1,
            );
            material.set_uniform_color(Rgba8::white());
            model.set_pose(tile_pose(i, fix32::zero()));
            counted_draw(pass, &base, model, material, &marked_view);
        }
    }
}
fn draw_switches(
    pass: &mut Pass,
    model: &mut Model,
    material: &mut Material,
    camera: &Camera,
    state: &RenderState,
) {
    let mut cache = button_cache_data();
    let mask = visual_mask();
    let mut j: u16 = 0;
    while j < cache[0u16] {
        let i = cache[1 + j * 3];
        let kind = level_field(i, 0);
        let bit = level_field(i, 1);
        let channel = bit_channel(bit);
        let key = if mask & bit != 0 {
            32768 + if kind == 4 { 0u16 } else { switch_age(channel) }
        } else {
            0u16
        };
        let part = j + 1;
        if pass.prepared_part_ready(part) && cache[2 + j * 3] == key {
            pass.append_prepared_part(part);
            DRAW_CALLS[0u16] += cache[3 + j * 3];
        } else {
            let before = DRAW_CALLS[0u16];
            pass.begin_prepared_part(part);
            draw_switch_tile(pass, model, material, i, kind, channel);
            pass.end_prepared_group();
            cache[2 + j * 3] = key;
            cache[3 + j * 3] = DRAW_CALLS[0u16] - before;
        }
        pass.set_camera(camera.ptr());
        pass.set_state(state.ptr());
        j += 1;
    }
}
fn prepare_block(model: &mut Model) {
    if !block_visible() {
        BLOCK_POSE_KEY[0u16] = 0;
        return;
    }
    let resting = visual_mode() == PLAY && MOTION[5u16] == 0;
    let key = (GAME[0u16] & 31) | ((GAME[1u16] & 31) << 5) | ((GAME[2u16] & 3) << 10) | 32768;
    if split_visual()
        || !resting
        || BLOCK_POSE_KEY[0u16] != key
        || BLOCK_POSE_KEY[1u16] != LEVEL_GENERATION[0u16]
        || BLOCK_POSE_KEY[2u16] != block_orientation()
    {
        block_pose(model);
    }
    BLOCK_POSE_KEY[0u16] = if resting && !split_visual() {
        key
    } else {
        0u16
    };
    BLOCK_POSE_KEY[1u16] = LEVEL_GENERATION[0u16];
    BLOCK_POSE_KEY[2u16] = block_orientation();
}

fn draw_block(pass: &mut Pass, block: &Mesh, model: &mut Model, material: &Material) {
    if block_visible() {
        let texture = game_texture();
        let view = clamped_texture_view(&texture);
        counted_draw(pass, block, model, material, &view);
    }
}
fn level_eye(which: u16) -> Vec3 {
    Vec3::new(
        fix32::from_words(blox_level_meta(which, 4), blox_level_meta(which, 5)),
        fix32::from_words(blox_level_meta(which, 6), blox_level_meta(which, 7)),
        fix32::from_words(blox_level_meta(which, 8), blox_level_meta(which, 9)),
    )
}
fn level_far(which: u16) -> fix32 {
    fix32::from_words(blox_level_meta(which, 12), blox_level_meta(which, 13))
}
fn level_projection(which: u16) -> Projection {
    let far = level_far(which);
    Projection::from_focal_pixels(
        fix32::from_words(blox_level_meta(which, 10), blox_level_meta(which, 11)),
        far,
    )
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
// Capture-only progress: ASCII stage, four hexadecimal digits and LF.
// A=loop, B=source begin, C=source retired, D=consumer submit,
// E=consumer wait, F=presented, H=selected-brightness halt.
fn capture_report(stage: u16, value: u16) {
    let mut i: u16 = 0;
    while i < 6 {
        let mut byte = stage;
        if i == 5 {
            byte = 10;
        }
        if i > 0 && i < 5 {
            let nibble = (value >> ((4 - i) * 4)) & 15;
            byte = if nibble < 10 {
                nibble + 48
            } else {
                nibble + 55
            };
        }
        while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 {}
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, byte);
        i += 1;
    }
}
// Explicit capture-card diagnostic only: hold the initial scene, make one real
// rule move, hold that result, then stop after an opaque selected-brightness RTT frame.
static CAPTURE_STIMULUS: Buf<u16, 2> = Buf::new([0; 2]);
fn capture_buttons(elapsed: u16) -> u16 {
    if SESSION[0u16] != PLAY {
        return 0;
    }
    let phase = CAPTURE_STIMULUS[0u16];
    let dt = if elapsed > 50 { 50u16 } else { elapsed };
    if phase == 1 && GAME[5u16] == 1 {
        CAPTURE_STIMULUS[0u16] = 2;
        CAPTURE_STIMULUS[1u16] = 0;
        return 0;
    }
    if phase == 0 || phase == 2 {
        CAPTURE_STIMULUS[1u16] += dt;
        if CAPTURE_STIMULUS[1u16] >= 2000 {
            CAPTURE_STIMULUS[1u16] = 0;
            CAPTURE_STIMULUS[0u16] = phase + 1;
            return if phase == 0 { 32u16 } else { 8u16 };
        }
    }
    0
}

fn submit_game(
    gpu: &mut Gpu,
    commands: CommandBuffer,
    finish: bool,
    begin: u16,
    started: bool,
) -> CommandBuffer {
    gpu.submit(commands);
    wait_game(gpu, finish, begin, started)
}
fn wait_game(gpu: &mut Gpu, finish: bool, begin: u16, started: bool) -> CommandBuffer {
    let completed = gpu.wait();
    if finish {
        render_time_complete(begin, time_milliseconds(), started);
    }
    completed
}
fn record_static_scene(
    commands: &mut CommandBuffer,
    color: &ColorBuffer,
    depth: &DepthBuffer,
    background: &ColorBuffer,
    camera: &Camera,
    fragile: Ptr,
) {
    let state = game_solid_state();
    let mut floor_model = game_floor_model();
    let mut floor_material = game_material(Rgba8::white(), false);
    let mut pad_model = game_plain_model();
    let mut pad_material = game_material(Rgba8::white(), false);
    let load = scene_color_load(commands, background, color);
    let mut pass = commands.pass(color.ptr(), depth.ptr(), load, DepthLoad::clear(65535));
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
    text_u16_field(grid, 6, 0, 2, MOTION[6u16] + 1);
    if HUD[0u16] != 0 {
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
        if HUD[2u16] != 0 {
            text_u16_field(grid, 5, 1, 4, HUD[1u16]);
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
        if FRAME_RENDER_MS[1u16] != 0 {
            text_u16_field(grid, 3, 2, 5, FRAME_RENDER_MS[0u16]);
        } else {
            grid.write(
                CellRect {
                    x: 3,
                    y: 2,
                    width: 5,
                    height: 1,
                },
                b"-----",
            );
        }
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

fn run(limit: u16, automatic_capture: bool, capture_brightness: u16) {
    let mut gpu = Gpu::init(board_gpu_config());
    let mut commands = gpu.command_buffer();
    commands.enable_prepared_parts(PreparedPartsLayout::SplitWide);
    let mut scene_commands = gpu.command_buffer();
    button_cache_begin();
    BRIDGE_COMMANDS[0u16] = 0;
    BRIDGE_COMMANDS[1u16] = 65535;
    let ground_owner = ground_begin();
    let shadow_owner = shadow_batch_begin();
    let fragile_owner = fragile_begin();
    let mut scene_generation: u16 = 0;
    let mut scene_background: u16 = 2;
    let mut scene_hidden: u16 = 65535;
    let mut scene_valid = false;
    let mut snapshot_generation: u16 = 0;
    let mut snapshot_valid = false;
    let mut previous_tint: u16 = 65535;
    let offscreen = ColorBuffer::from_memory(unsafe { GpuMemory::from_sectors(0xc000, 2048) });
    let background =
        ColorBuffer::from_memory(unsafe { GpuMemory::from_sectors(BLOX_BACKGROUND_SECTOR, 1600) });
    let offscreen_view = offscreen.texture_view();
    let color_a = ColorBuffer::from_surface(BOARD_COLOR_A);
    let color_b = ColorBuffer::from_surface(BOARD_COLOR_B);
    let depth = board_depth();
    let static_color = ColorBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(BLOX_STATIC_COLOR_SECTOR, 2048)
    });
    let static_depth = DepthBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(BLOX_STATIC_DEPTH_SECTOR, 2048)
    });
    let mut display = Display::open();
    let camera = game_camera();
    let mut camera_level: u16 = 0;
    let mut scene = SceneResources::new(camera);
    let pad_owner = malloc(15);
    let mut pad_cache = unsafe { pad_owner.as_u16_array() };
    let mut cache_word: u16 = 0;
    while cache_word < 15 {
        pad_cache[cache_word] = 0;
        cache_word += 1;
    }
    let pad_texture = game_texture();
    let pad_view = clamped_texture_view(&pad_texture);
    let mut hatch_material = game_material(Rgba8::white(), false);
    let mut tint_material = game_material(Rgba8::white(), true);
    let mut grid = game_ui_grid();
    session_init();
    pad_cache_update(pad_cache);
    let mut carrier_texture = pad_carrier_texture();
    let mut carrier_view = clamped_texture_view(&carrier_texture);
    shadow_invalidate();
    BLOCK_POSE_KEY[0u16] = 0;
    gamepad_init();
    let mut previous = time_milliseconds();
    let mut render_start: u16 = 0;
    let mut render_started = false;
    let mut number: u16 = 0;
    let mut back_b = true;
    loop {
        shadow_batch_frame();
        signal(4, number);
        if automatic_capture {
            capture_report(65, number);
        }
        gamepad_capture();
        let input = gamepad_take_frame();
        if !fading() && input.pressed & 8192 != 0 {
            HUD[0u16] ^= 1;
        }
        if !fading() && input.pressed & 1 != 0 {
            BACKGROUND[0u16] ^= 1;
        }
        let now = time_milliseconds();
        let elapsed = now - previous;
        previous = now;
        let buttons = if automatic_capture {
            capture_buttons(elapsed)
        } else {
            input.pressed
        };
        let held = if automatic_capture {
            buttons & 240
        } else {
            input.buttons & 240
        };
        session_tick(buttons, held, elapsed);
        if automatic_capture && CAPTURE_STIMULUS[0u16] == 3 && TRANSITION[0u16] == FADE_OUT {
            let stop = if capture_brightness == 255 {
                0u16
            } else {
                160u16
            };
            if TRANSITION[1u16] > stop {
                TRANSITION[1u16] = stop;
            }
        }
        if camera_level != MOTION[6u16] {
            scene_valid = false;
            camera_level = MOTION[6u16];
            scene.camera.set_look_at_y_up(
                level_eye(camera_level),
                Vec3::new(fix32::zero(), fix!(0.6), fix32::zero()),
            );
            scene.camera.set_projection(level_projection(camera_level));
            web_camera_axes(&mut scene.camera);
        }
        let fade = fading();
        let color = if fade { tint() } else { 255u16 };
        let back = if back_b { &color_b } else { &color_a };
        let target = if fade { &offscreen } else { back };
        let capture = fade && (!snapshot_valid || snapshot_generation != LEVEL_GENERATION[0u16]);
        let prepare_scene = !fade || capture;
        if !fade {
            snapshot_valid = false;
        }
        DRAW_CALLS[0u16] = 0;
        if automatic_capture {
            text_u16_field(&mut grid, 0, 0, 4, GAME[5u16]);
            text_u16_field(&mut grid, 6, 0, 1, input.connected);
        }
        if prepare_scene {
            sync_bridge_group(&mut commands);
            if pad_cache[0u16] != LEVEL_GENERATION[0u16] {
                drop(carrier_view);
                drop(carrier_texture);
                carrier_texture = pad_carrier_texture();
                carrier_view = clamped_texture_view(&carrier_texture);
            }
            pad_cache_update(pad_cache);
            if scene_generation != LEVEL_GENERATION[0u16] {
                scene_valid = false;
                scene_commands.reset();
                if ground_enabled() && !ground_matches_level() {
                    commands.reset();
                    ground_rebuild();
                }
                if !fragile_matches_level(fragile_owner) {
                    commands.reset();
                    fragile_rebuild(fragile_owner);
                }
            }
            // Source indices can change only after both preceding CBs retired.
            // Their changed coverage invalidates pixels as well as geometry.
            fragile_sync(fragile_owner);
            if !scene_valid
                || scene_background != BACKGROUND[0u16]
                || scene_hidden != fragile_hidden(fragile_owner)
            {
                scene_valid = false;
                scene_commands.reset();
                record_static_scene(
                    &mut scene_commands,
                    &static_color,
                    &static_depth,
                    &background,
                    &scene.camera,
                    fragile_owner,
                );

                gpu.submit(scene_commands);
                scene_commands = wait_game(&mut gpu, false, 0, false);
                scene_generation = LEVEL_GENERATION[0u16];
                scene_background = BACKGROUND[0u16];
                scene_hidden = fragile_hidden(fragile_owner);
                scene_valid = true;
            }
            scene_commands.reset();
            scene_commands.copy_color(static_color.ptr(), target.ptr());
            scene_commands.copy_depth(static_depth.ptr(), depth.ptr());
            {
                let pass = scene_commands.pass(
                    target.ptr(),
                    depth.ptr(),
                    ColorLoad::load(),
                    DepthLoad::load(),
                );
            }

            gpu.submit(scene_commands);
            if automatic_capture && fade {
                capture_report(66, number);
            }
            commands.reset();
            {
                let mut pass = commands.pass(
                    target.ptr(),
                    depth.ptr(),
                    ColorLoad::load(),
                    DepthLoad::load(),
                );
                pass.set_camera(scene.camera.ptr());
                pass.set_state(scene.state.ptr());
                prepare_block(&mut scene.block_model);
                draw_bridge_group(
                    &mut pass,
                    &mut scene.floor_model,
                    &mut scene.floor_material,
                    &scene.state,
                    &scene.hint_state,
                    false,
                );
                pass.set_camera(scene.camera.ptr());
                pass.set_state(scene.state.ptr());
                draw_switches(
                    &mut pass,
                    &mut scene.pad_model,
                    &mut hatch_material,
                    &scene.camera,
                    &scene.state,
                );
                if begin_pad_group(&mut pass, pad_cache) {
                    draw_pad_part(
                        &mut pass,
                        &scene.pad,
                        &mut scene.entry_hatch_model,
                        &scene.entry_hatch_material,
                        &pad_view,
                        PadPart {
                            cache: pad_cache,
                            role: 0,
                        },
                    );
                    draw_pad_part(
                        &mut pass,
                        &scene.pad,
                        &mut scene.entry0,
                        &scene.entry_ring_material,
                        &pad_view,
                        PadPart {
                            cache: pad_cache,
                            role: 3,
                        },
                    );
                    draw_pad_part(
                        &mut pass,
                        &scene.pad,
                        &mut scene.entry1,
                        &scene.entry_ring_material,
                        &pad_view,
                        PadPart {
                            cache: pad_cache,
                            role: 4,
                        },
                    );
                    draw_pad_part(
                        &mut pass,
                        &scene.pad,
                        &mut scene.entry2,
                        &scene.entry_ring_material,
                        &pad_view,
                        PadPart {
                            cache: pad_cache,
                            role: 5,
                        },
                    );
                    draw_pad_part(
                        &mut pass,
                        &scene.pad,
                        &mut scene.entry3,
                        &scene.entry_ring_material,
                        &pad_view,
                        PadPart {
                            cache: pad_cache,
                            role: 6,
                        },
                    );
                    draw_pad_part(
                        &mut pass,
                        &scene.pad,
                        &mut scene.goal_hatch_model,
                        &scene.goal_hatch_material,
                        &pad_view,
                        PadPart {
                            cache: pad_cache,
                            role: 1,
                        },
                    );
                    draw_pad_part(
                        &mut pass,
                        &scene.pad,
                        &mut scene.carrier_model,
                        &scene.carrier_material,
                        &carrier_view,
                        PadPart {
                            cache: pad_cache,
                            role: 2,
                        },
                    );
                    draw_pad_part(
                        &mut pass,
                        &scene.pad,
                        &mut scene.goal0,
                        &scene.goal_ring_material,
                        &pad_view,
                        PadPart {
                            cache: pad_cache,
                            role: 7,
                        },
                    );
                    draw_pad_part(
                        &mut pass,
                        &scene.pad,
                        &mut scene.goal1,
                        &scene.goal_ring_material,
                        &pad_view,
                        PadPart {
                            cache: pad_cache,
                            role: 8,
                        },
                    );
                    draw_pad_part(
                        &mut pass,
                        &scene.pad,
                        &mut scene.goal2,
                        &scene.goal_ring_material,
                        &pad_view,
                        PadPart {
                            cache: pad_cache,
                            role: 9,
                        },
                    );
                    draw_pad_part(
                        &mut pass,
                        &scene.pad,
                        &mut scene.goal3,
                        &scene.goal_ring_material,
                        &pad_view,
                        PadPart {
                            cache: pad_cache,
                            role: 10,
                        },
                    );
                    end_pad_group(&mut pass, pad_cache);
                }
                pass.set_camera(scene.camera.ptr());
                pass.set_state(scene.shadow_state.ptr());
                draw_shadow(
                    &mut pass,
                    &scene.shadow_mesh,
                    &mut scene.shadow_model,
                    &scene.shadow_material,
                    &scene.block_model,
                );
                if split_visual() {
                    idle_cube_pose(&mut scene.idle_model);
                    draw_shadow(
                        &mut pass,
                        &scene.shadow_mesh,
                        &mut scene.shadow_model,
                        &scene.shadow_material,
                        &scene.idle_model,
                    );
                }
                pass.set_state(scene.state.ptr());
                draw_block(
                    &mut pass,
                    &scene.block,
                    &mut scene.block_model,
                    &scene.material,
                );
                if split_visual() {
                    draw_block(
                        &mut pass,
                        &scene.block,
                        &mut scene.idle_model,
                        &scene.idle_material,
                    );
                }
                // Opaque geometry and receiver shadows are complete. Inactive
                // bridge hints now test opaque depth without writing it.
                draw_bridge_group(
                    &mut pass,
                    &mut scene.floor_model,
                    &mut scene.floor_material,
                    &scene.state,
                    &scene.hint_state,
                    true,
                );
                if automatic_capture {
                    counted_ui(
                        &mut pass,
                        &grid,
                        CellRect {
                            x: 0,
                            y: 0,
                            width: 7,
                            height: 1,
                        },
                    );
                }
            }
            // The other CB is CPU-owned while the immutable static CB runs.
            // Its Load dependency is checked only after static completion,
            // when it is submitted; recording does not reclaim that producer.
            scene_commands = wait_game(&mut gpu, false, 0, false);
            if fade {
                commands = submit_game(&mut gpu, commands, false, 0, false);
                snapshot_valid = true;
                snapshot_generation = LEVEL_GENERATION[0u16];
                session_captured();
                signal(11, SESSION[4u16]);
                if automatic_capture {
                    capture_report(67, number);
                }
            }
        }
        if fade {
            if automatic_capture {
                text_u16_field(&mut grid, 0, 0, 4, 1000 + color);
            }
            commands.reset();
            if capture {
                commands.invalidate_textures();
            }
        }
        let mut color_load = ColorLoad::load();
        if fade {
            color_load = ColorLoad::clear(Rgb565::from_raw(0x1107));
        }
        {
            let mut pass = commands.pass(back.ptr(), depth.ptr(), color_load, DepthLoad::load());
            if fade {
                if previous_tint != color {
                    tint_material.set_uniform_color(Rgba8 {
                        r: 255,
                        g: 255,
                        b: 255,
                        a: color as u8,
                    });
                    previous_tint = color;
                }
                pass.set_camera(scene.screen_camera.ptr());
                pass.set_state(scene.screen_state.ptr());
                counted_draw(
                    &mut pass,
                    &scene.screen,
                    &scene.screen_model,
                    &tint_material,
                    &offscreen_view,
                );
                if automatic_capture {
                    counted_ui(
                        &mut pass,
                        &grid,
                        CellRect {
                            x: 0,
                            y: 0,
                            width: 4,
                            height: 1,
                        },
                    );
                }
            } else {
                if automatic_capture {
                    counted_ui(
                        &mut pass,
                        &grid,
                        CellRect {
                            x: 0,
                            y: 0,
                            width: 7,
                            height: 1,
                        },
                    );
                }
            }
            if !automatic_capture {
                draw_hud(&mut pass, &mut grid);
            }
        }
        if automatic_capture {
            capture_report(68, color);
        }
        commands = submit_game(&mut gpu, commands, true, render_start, render_started);
        if automatic_capture {
            capture_report(69, color);
        }
        HUD[1u16] = DRAW_CALLS[0u16];
        HUD[2u16] = 1;
        display.set_next_swap(back.ptr());
        display.wait_vblank();
        render_start = time_milliseconds();
        render_started = true;
        gamepad_capture();
        signal(12, SESSION[0u16]);
        if automatic_capture {
            capture_report(70, SESSION[0u16]);
        }
        if automatic_capture
            && CAPTURE_STIMULUS[0u16] == 3
            && TRANSITION[0u16] == FADE_OUT
            && color == capture_brightness
        {
            capture_report(72, color);
            capture_report(76, dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW));
            capture_report(85, dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH));
            capture_report(77, dev_recv(DISPLAY_DEVICE, DISPLAY_COLOR_FORMAT));
            capture_report(80, dev_recv(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND));
            capture_report(71, dev_recv(GPU_DEVICE, GPU_STATUS));
            halt(0x600d);
        }
        session_presented();
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
    free(pad_owner);
    button_cache_end();
    session_end();
}

pub fn renderer_game_frames() {
    run(120, false, 128);
    halt(0x600d);
}
fn main() {
    run(0, false, 128);
}
pub fn renderer_game_capture() {
    run(0, true, 128);
    halt(0x600d);
}
pub fn renderer_game_capture_full() {
    run(0, true, 255);
    halt(0x600d);
}

pub fn renderer_game_three_frames() {
    run(3, false, 128);
    halt(0x600d);
}
