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
        commands.clear_prepared_parts();
        button_cache_rebuild();
        BRIDGE_COMMANDS[0u16] = LEVEL_GENERATION[0u16];
        BRIDGE_COMMANDS[1u16] = visual_mask();
    } else if BRIDGE_COMMANDS[1u16] != visual_mask() {
        commands.invalidate_prepared_part(0);
        commands.invalidate_prepared_part(15);
        BRIDGE_COMMANDS[1u16] = visual_mask();
    }
}
// The stage owns the mapping: opaque bridges0, up to thirteen switches,
// pads14 and translucent bridge hints15.
fn prepare_bridges(commands: &mut CommandBuffer, scene: &mut SceneResources, transparent: bool) {
    let part = if transparent { 15u16 } else { 0u16 };
    if commands.prepared_part_ready(part) {
        return;
    }
    let before = DRAW_CALLS[0u16];
    let mut recording = commands.prepare_part(part);
    recording.set_camera(scene.camera.ptr());
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
    let count = if transparent { 4u16 } else { 2u16 };
    BRIDGE_COMMANDS[count] = DRAW_CALLS[0u16] - before;
    DRAW_CALLS[0u16] = before;
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
fn draw_bridges(
    recording: &mut PreparedPart,
    model: &mut Model,
    material: &mut Material,
    transparent: bool,
) {
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
    commands: &mut CommandBuffer,
    scene: &mut SceneResources,
    material: &mut Material,
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
        if !commands.prepared_part_ready(part) || cache[2 + j * 3] != key {
            let before = DRAW_CALLS[0u16];
            let mut recording = commands.prepare_part(part);
            recording.set_camera(scene.camera.ptr());
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
            cache[3 + j * 3] = DRAW_CALLS[0u16] - before;
            DRAW_CALLS[0u16] = before;
        }
        j += 1;
    }
}

fn draw_switches(pass: &mut Pass) {
    let cache = button_cache_data();
    let mut j: u16 = 0;
    while j < cache[0u16] {
        pass.draw_prepared(j + 1);
        DRAW_CALLS[0u16] += cache[3 + j * 3];
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

#[inline(never)]
fn finish_frame(gpu: &mut Gpu, commands: CommandBuffer, started: bool) -> CommandBuffer {
    gpu.submit(commands);
    let completed = gpu.wait();
    let elapsed = time_microseconds();
    render_time_complete(elapsed, started);
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
fn advance_gameplay(
    clock: &mut FrameClock,
    automatic_capture: bool,
    capture_brightness: u16,
) -> u16 {
    gamepad_capture();
    let input = gamepad_take_frame();
    if !fading() && input.pressed & 8192 != 0 {
        HUD[0u16] ^= 1;
    }
    if !fading() && input.pressed & 1 != 0 {
        BACKGROUND[0u16] ^= 1;
    }
    let now = time_milliseconds();
    let elapsed = now - clock.previous;
    clock.previous = now;
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
    input.connected
}

struct StaticScene {
    color: ColorBuffer,
    depth: DepthBuffer,
    background: ColorBuffer,
    generation: u16,
    background_mode: u16,
    hidden: u16,
    valid: bool,
}
impl StaticScene {
    fn new() -> Self {
        let background = ColorBuffer::from_memory(unsafe {
            GpuMemory::from_sectors(BLOX_BACKGROUND_SECTOR, 1600)
        });
        let color = ColorBuffer::from_memory(unsafe {
            GpuMemory::from_sectors(BLOX_STATIC_COLOR_SECTOR, 2048)
        });
        let depth = DepthBuffer::from_memory(unsafe {
            GpuMemory::from_sectors(BLOX_STATIC_DEPTH_SECTOR, 2048)
        });
        Self {
            color: color,
            depth: depth,
            background: background,
            generation: 0,
            background_mode: 2,
            hidden: 65535,
            valid: false,
        }
    }
}
fn refresh_static_scene(
    cache: &mut StaticScene,
    gpu: &mut Gpu,
    mut commands: CommandBuffer,
    dynamic_commands: &mut CommandBuffer,
    camera: &Camera,
    fragile_owner: Ptr,
) -> CommandBuffer {
    if cache.generation != LEVEL_GENERATION[0u16] {
        cache.valid = false;
        commands.reset();
        if ground_enabled() && !ground_matches_level() {
            dynamic_commands.reset();
            ground_rebuild();
        }
        if !fragile_matches_level(fragile_owner) {
            dynamic_commands.reset();
            fragile_rebuild(fragile_owner);
        }
    }
    // Source indices can change only after both preceding CBs retired.
    // Their changed coverage invalidates pixels as well as geometry.
    fragile_sync(fragile_owner);
    if !cache.valid
        || cache.background_mode != BACKGROUND[0u16]
        || cache.hidden != fragile_hidden(fragile_owner)
    {
        cache.valid = false;
        commands.reset();
        record_static_scene(
            &mut commands,
            &cache.color,
            &cache.depth,
            &cache.background,
            camera,
            fragile_owner,
        );

        gpu.submit(commands);
        commands = gpu.wait();
        cache.generation = LEVEL_GENERATION[0u16];
        cache.background_mode = BACKGROUND[0u16];
        cache.hidden = fragile_hidden(fragile_owner);
        cache.valid = true;
    }
    commands
}
fn record_static_copy(
    commands: &mut CommandBuffer,
    cache: &StaticScene,
    target: &ColorBuffer,
    depth: &DepthBuffer,
) {
    commands.reset();
    commands.copy_color(cache.color.ptr(), target.ptr());
    commands.copy_depth(cache.depth.ptr(), depth.ptr());
    let pass = commands.pass(
        target.ptr(),
        depth.ptr(),
        ColorLoad::load(),
        DepthLoad::load(),
    );
}

fn prepare_pads(
    commands: &mut CommandBuffer,
    scene: &mut SceneResources,
    mut pad_cache: Array<u16>,
    pad_view: &TextureView,
    carrier_view: &TextureView,
) {
    if commands.prepared_part_ready(14)
        && pad_cache[11u16] == pad_cache[1u16]
        && pad_cache[12u16] == pad_cache[2u16]
        && pad_cache[13u16] == pad_cache[5u16]
    {
        return;
    }
    let before = DRAW_CALLS[0u16];
    let mut recording = commands.prepare_part(14);
    recording.set_camera(scene.camera.ptr());
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
    pad_cache[11u16] = pad_cache[1u16];
    pad_cache[12u16] = pad_cache[2u16];
    pad_cache[13u16] = pad_cache[5u16];
    pad_cache[14u16] = DRAW_CALLS[0u16] - before;
    DRAW_CALLS[0u16] = before;
}

fn record_dynamic_scene(pass: &mut Pass, scene: &mut SceneResources, pad_cache: Array<u16>) {
    prepare_block(&mut scene.block_model);
    pass.draw_prepared(0);
    DRAW_CALLS[0u16] += BRIDGE_COMMANDS[2u16];
    draw_switches(pass);
    pass.draw_prepared(14);
    DRAW_CALLS[0u16] += pad_cache[14u16];

    pass.set_camera(scene.camera.ptr());
    pass.set_state(scene.shadow_state.ptr());
    draw_shadow(
        pass,
        &scene.shadow_mesh,
        &mut scene.shadow_model,
        &scene.shadow_material,
        &scene.block_model,
    );
    if split_visual() {
        idle_cube_pose(&mut scene.idle_model);
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
    if split_visual() {
        draw_block(
            pass,
            &scene.block,
            &mut scene.idle_model,
            &scene.idle_material,
        );
    }
    // Opaque geometry and receiver shadows are complete. Inactive
    // bridge hints now test opaque depth without writing it.
    pass.draw_prepared(15);
    DRAW_CALLS[0u16] += BRIDGE_COMMANDS[4u16];
}

struct FrameOverlay {
    offscreen_view: TextureView,
    tint_material: Material,
    previous_tint: u16,
    automatic_capture: bool,
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
        if overlay.automatic_capture {
            counted_ui(
                pass,
                grid,
                CellRect {
                    x: 0,
                    y: 0,
                    width: 4,
                    height: 1,
                },
            );
        }
    } else {
        if overlay.automatic_capture {
            counted_ui(
                pass,
                grid,
                CellRect {
                    x: 0,
                    y: 0,
                    width: 7,
                    height: 1,
                },
            );
        }
    }
    if !overlay.automatic_capture {
        draw_hud(pass, grid);
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
    let mut overlay = FrameOverlay {
        offscreen_view: offscreen_view,
        tint_material: game_material(Rgba8::white(), true),
        previous_tint: 65535,
        automatic_capture: automatic_capture,
    };
    let mut grid = game_ui_grid();
    session_init();
    pad_cache_update(pad_cache);
    let mut carrier_texture = pad_carrier_texture();
    let mut carrier_view = clamped_texture_view(&carrier_texture);
    shadow_invalidate();
    BLOCK_POSE_KEY[0u16] = 0;
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
        if automatic_capture {
            capture_report(65, number);
        }
        let connected = advance_gameplay(&mut clock, automatic_capture, capture_brightness);
        if camera_level != MOTION[6u16] {
            static_scene.valid = false;
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
            text_u16_field(&mut grid, 6, 0, 1, connected);
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
            scene_commands = refresh_static_scene(
                &mut static_scene,
                &mut gpu,
                scene_commands,
                &mut commands,
                &scene.camera,
                fragile_owner,
            );
            record_static_copy(&mut scene_commands, &static_scene, target, &depth);
            gpu.submit(scene_commands);
            if automatic_capture && fade {
                capture_report(66, number);
            }
            commands.reset();
            prepare_bridges(&mut commands, &mut scene, false);
            prepare_switches(&mut commands, &mut scene, &mut hatch_material);
            prepare_pads(
                &mut commands,
                &mut scene,
                pad_cache,
                &pad_view,
                &carrier_view,
            );
            prepare_bridges(&mut commands, &mut scene, true);
            {
                let mut pass = commands.pass(
                    target.ptr(),
                    depth.ptr(),
                    ColorLoad::load(),
                    DepthLoad::load(),
                );
                record_dynamic_scene(&mut pass, &mut scene, pad_cache);
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
            scene_commands = gpu.wait();
            if fade {
                gpu.submit(commands);
                commands = gpu.wait();
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
            record_overlay(&mut pass, &scene, &mut overlay, &mut grid, fade, color);
        }
        if automatic_capture {
            capture_report(68, color);
        }
        commands = finish_frame(&mut gpu, commands, render_started);
        if automatic_capture {
            capture_report(69, color);
        }
        HUD[1u16] = DRAW_CALLS[0u16];
        HUD[2u16] = 1;
        display.set_next_swap(back.ptr());
        display.wait_vblank();
        time_clear_microseconds();
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
