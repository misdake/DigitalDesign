//! All-stage Bloxorz renderer with live colored RTT transitions.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
use gpu_demo_api::*;
mod profiler_clock;
use profiler_clock::*;
mod gamepad;
use gamepad::*;
mod boot_resources;
use boot_resources::*;
mod session;
use session::*;
mod shadow;
use shadow::*;
mod render_stats;
use render_stats::*;
mod ground_batches;
use ground_batches::*;
pub static HUD: Buf<u16, 3> = Buf::new([0; 3]);

pub static BACKGROUND: Buf<u16, 1> = Buf::new([1]);
// One caller-owned block Model lives for this run; retain only its resting key.
static BLOCK_POSE_KEY: Buf<u16, 2> = Buf::new([0; 2]);
fn scene_color_load(
    commands: &mut CommandBuffer,
    source: &ColorBuffer,
    target: &ColorBuffer,
) -> ColorLoad {
    if BACKGROUND[0u16] != 0 {
        commands.copy_color(source, target);
        return ColorLoad::load();
    }
    ColorLoad::clear(Rgb565::from_raw(0x2924))
}
fn pad_box(
    pass: &mut Pass,
    mesh: &Mesh,
    model: &mut Model,
    material: &mut Material,
    center: Vec3,
    size: Vec3,
) {
    model.set_pose(demo_pose(center, demo_zero(), size));
    let texture = demo_texture();
    let view = texture.view();
    counted_draw(pass, mesh, model, material, &view);
}
fn goal_carrier_enabled() -> bool {
    blox_level_meta(MOTION[6u16], 14) != 0
}
fn pad_opening(start: bool, mode: u16, t: u16) -> fix32 {
    let one = fix32::from_int(1);
    let mut opening = fix32::from_words(56361, 0); // exit idle0.86
    if (start && mode == ENTRY_OPEN) || (!start && (mode == GOAL_OPEN || mode == GOAL_CLOSE)) {
        let f = fraction(t, 200);
        let eased = f * f * (fix32::from_int(3) - fix32::from_int(2) * f);
        if mode == ENTRY_OPEN {
            opening = eased;
        } else if mode == GOAL_OPEN {
            opening = fix32::from_words(56361, 0) + fix32::from_words(5243, 0) * eased;
        } else {
            opening = fix32::from_words(61604, 0) * (one - eased);
        }
    } else if !start && (mode == GOAL_DROP || mode == GOAL_LAND) {
        opening = fix32::from_words(61604, 0);
    }
    opening
}
fn goal_aperture(opening: fix32) -> u16 {
    ((opening * fix32::from_int(50) + fix32::from_words(32768, 0)).to_int() as u16)
}
fn draw_goal_carrier(
    pass: &mut Pass,
    model: &mut Model,
    material: &mut Material,
    cx: fix32,
    cz: fix32,
    aperture: u16,
) {
    let phase = (SESSION[6u16] * 3 + SESSION[7u16] * 5) & 3;
    let texture = Texture::from_native(TextureDesc {
        memory: unsafe { GpuMemory::from_sectors(60416 + phase * 256, 256) },
        size_log2: 7,
        full_mip: false,
    });
    let view = texture.view();
    let mesh = demo_mesh_part(GOAL_CARRIER_FIRST + aperture, 1);
    let half = fix32::from_words(32768, 0);
    material.set_unlit(true);
    material.set_uniform_color(demo_white());
    model.set_pose(demo_pose(
        demo_point(cx, fix32::zero(), cz),
        demo_zero(),
        demo_point(half, half, half),
    ));
    counted_draw(pass, &mesh, model, material, &view);
}
fn draw_pad(
    pass: &mut Pass,
    box_mesh: &Mesh,
    model: &mut Model,
    material: &mut Material,
    hatch_material: &mut Material,
    selector: u16,
) {
    let start = selector & 1 != 0;
    let static_only = selector & 2 != 0;
    let x = if start { LEVEL[2u16] } else { SESSION[6u16] };
    let z = if start { LEVEL[3u16] } else { SESSION[7u16] };
    if x == 65535 {
        return;
    }
    let mode = visual_mode();
    let t = visual_time();
    let one = fix32::from_int(1);
    let half = fix32::from_words(32768, 0);
    let cx = fix32::from_int(x as i16) - fix32::from_int(LEVEL[0u16] as i16) * half + half;
    let cz = fix32::from_int(z as i16) - fix32::from_int(LEVEL[1u16] as i16) * half + half;
    let texture = demo_texture();
    let view = texture.view();
    if static_only {
        let well = demo_mesh_part(WELL_FIRST, WELL_COUNT);
        material.set_unlit(false);
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
            demo_point(cx, fix32::zero(), cz),
            demo_zero(),
            demo_scale(),
        ));
        counted_draw(pass, &well, model, material, &view);
        return;
    }
    let carrier = !start && goal_carrier_enabled();
    let opening = pad_opening(start, mode, t);
    let aperture = goal_aperture(opening);
    let hatch_visible = if start {
        mode != ENTRY_OPEN && mode != ENTRY_RISE
    } else {
        mode <= GOAL_OPEN || mode > GOAL_CLOSE
    };
    if hatch_visible {
        let hatch = demo_mesh_part(
            if carrier {
                GOAL_HATCH_FIRST + aperture
            } else {
                TILE0_FIRST
            },
            1,
        );
        if start {
            hatch_material.set_uniform_color(Rgba8 {
                r: 67,
                g: 99,
                b: 79,
                a: 255,
            });
        } else {
            hatch_material.set_uniform_color(Rgba8 {
                r: 87,
                g: 50,
                b: 47,
                a: 255,
            });
        }
        model.set_pose(demo_pose(
            demo_point(cx, fix32::zero(), cz),
            demo_zero(),
            demo_point(
                if carrier {
                    half
                } else {
                    fix32::from_words(32604, 0)
                },
                half,
                if carrier {
                    half
                } else {
                    fix32::from_words(32604, 0)
                },
            ),
        ));
        counted_draw(pass, &hatch, model, hatch_material, &view);
    }
    // The entrance ring vanishes at the rim and stays hidden after entry.
    if start && mode != ENTRY_OPEN {
        return;
    }
    if carrier {
        draw_goal_carrier(pass, model, material, cx, cz, aperture);
        return;
    }
    material.set_unlit(true);
    if start {
        material.set_uniform_color(Rgba8 {
            r: 58,
            g: 165,
            b: 115,
            a: 255,
        });
    } else {
        material.set_uniform_color(Rgba8 {
            r: 224,
            g: 88,
            b: 88,
            a: 255,
        });
    }
    let width = (one - opening) * half;
    if width < fix32::from_words(128, 0) {
        return;
    }
    let mut panel: u16 = 0;
    while panel < 4 {
        let along_x = panel < 2;
        let sign = if panel & 1 == 0 { -one } else { one };
        let offset = sign * (one + opening) * fix32::from_words(16384, 0);
        // Match the reference visibility threshold for transverse strips.
        if along_x || opening > fix32::from_words(6, 0) {
            pad_box(
                pass,
                box_mesh,
                model,
                material,
                demo_point(
                    cx + if along_x { offset } else { fix32::zero() },
                    fix32::from_words(524, 0),
                    cz + if along_x { fix32::zero() } else { offset },
                ),
                demo_point(
                    if along_x { width } else { opening },
                    fix32::from_words(1966, 0),
                    if along_x { one } else { width },
                ),
            );
        }
        panel += 1;
    }
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
    let half = fix32::from_words(32768, 0);
    demo_pose(
        demo_point(
            fix32::from_int((index % LEVEL[0u16]) as i16)
                - fix32::from_int(LEVEL[0u16] as i16) * half
                + half,
            y,
            fix32::from_int(div_u16(index, LEVEL[0u16]) as i16)
                - fix32::from_int(LEVEL[1u16] as i16) * half
                + half,
        ),
        demo_zero(),
        demo_point(half, half, half),
    )
}

fn draw_ground_batches(pass: &mut Pass, model: &mut Model, material: &mut Material) {
    let half = fix32::from_words(32768, 0);
    model.set_pose(demo_pose(
        demo_zero(),
        demo_zero(),
        demo_point(half, half, half),
    ));
    material.set_unlit(false);
    let atlas = demo_texture();
    let view = atlas.view();
    let marker = marker_texture();
    let marker_view = marker.view();
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
                material.set_uniform_color(demo_white());
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
    let texture = demo_texture();
    let view = texture.view();
    let marker_texture = marker_texture();
    let marker_view = marker_texture.view();
    let half = fix32::from_words(32768, 0);
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
        let mesh = demo_mesh_part(
            if top {
                MARKED_SPLIT_FIRST + (face >> 13)
            } else {
                SHARED_FLOOR_FIRST + (face >> 13) * 5 + ((face >> 10) & 7)
            },
            1,
        );
        model.set_pose(demo_pose(
            demo_point(
                fix32::from_int(x as i16) - fix32::from_int(LEVEL[0u16] as i16) * half + half,
                fix32::zero(),
                fix32::from_int(z as i16) - fix32::from_int(LEVEL[1u16] as i16) * half + half,
            ),
            demo_zero(),
            demo_point(half, half, half),
        ));
        material.set_unlit(false);
        if top {
            material.set_uniform_color(demo_white());
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
    model.set_pose(demo_pose(
        demo_zero(),
        demo_zero(),
        demo_point(half, half, half),
    ));
}

fn draw_bridges(
    pass: &mut Pass,
    model: &mut Model,
    material: &mut Material,
    state: &RenderState,
    hint_state: &RenderState,
) {
    let texture = demo_texture();
    let view = texture.view();
    let mut i: u16 = 0;
    while i < LEVEL[6u16] {
        if level_field(i, 0) == 5 {
            let bit = level_field(i, 1);
            let active = visual_mask() & bit != 0;
            let variant = ((i % LEVEL[0u16]) * 3 + div_u16(i, LEVEL[0u16]) * 5) % 4;
            let mesh = demo_mesh_part(
                if active {
                    BRIDGE_TILE_FIRST + variant
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
            if !active {
                pass.set_state(hint_state);
            }
            counted_draw(pass, &mesh, model, material, &view);
            if !active {
                pass.set_state(state);
            }
        }
        i += 1;
    }
}
fn marker_texture() -> Texture {
    Texture::from_native(TextureDesc {
        memory: unsafe { GpuMemory::from_sectors(BLOX_MARKER_SECTOR, 1024) },
        size_log2: 8,
        full_mip: false,
    })
}
fn draw_fragile(pass: &mut Pass, model: &mut Model, material: &mut Material) {
    let texture = marker_texture();
    let view = texture.view();
    material.set_unlit(false);
    material.set_uniform_color(demo_white());
    let mut i: u16 = 0;
    while i < LEVEL[6u16] {
        if level_field(i, 0) == 2 {
            let variant = ((i % LEVEL[0u16]) * 3 + div_u16(i, LEVEL[0u16]) * 5) % 4;
            let mesh = demo_mesh_part(MARKED_FRAGILE_FIRST + variant, 1);
            model.set_pose(tile_pose(i, fragile_height(i)));
            counted_draw(pass, &mesh, model, material, &view);
        }
        i += 1;
    }
}
fn draw_switches(pass: &mut Pass, model: &mut Model, material: &mut Material) {
    let texture = demo_texture();
    let view = texture.view();
    let marked = marker_texture();
    let marked_view = marked.view();
    material.set_unlit(false);
    let one = fix32::from_int(1);
    let mut i: u16 = 0;
    while i < LEVEL[6u16] {
        let kind = level_field(i, 0);
        if kind == 3 || kind == 4 {
            let bit = level_field(i, 1);
            let channel = bit_channel(bit);
            let active = visual_mask() & bit != 0;
            let mut pulse = one;
            let mut sink = fix32::zero();
            if active {
                sink = fix32::from_words(3277, 0);
                let f = switch_fraction(channel);
                if f < one {
                    let sc = fsincos(f * fix32::from_words(9280, 3));
                    let wave = sc.x();
                    let remain = one - f;
                    pulse = one + fix32::from_words(16384, 0) * wave;
                    sink = fix32::from_words(3277, 0) * (one - remain * remain * remain)
                        + fix32::from_words(1311, 0) * wave;
                }
            }
            if kind == 3 {
                let mesh = demo_mesh_part(
                    MARKED_SOFT_FIRST + (channel & 7) * 2 + if active { 1u16 } else { 0u16 },
                    1,
                );
                let mut y = fix32::from_words(2621, 0) - sink;
                if y < fix32::from_words(983, 0) {
                    y = fix32::from_words(983, 0);
                }
                material.set_uniform_color(demo_white());
                model.set_pose(tile_pose(i, y));
                counted_draw(pass, &mesh, model, material, &marked_view);
            } else {
                let tile = demo_mesh_part(TILE1_FIRST, TILE1_COUNT);
                material
                    .set_uniform_color(channel_color(channel, if active { 1u16 } else { 0u16 }));
                let tile_transform = tile_pose(i, fix32::zero());
                model.set_pose(tile_transform);
                counted_draw(pass, &tile, model, material, &view);
                let base = demo_mesh_part(MARKED_HEAVY_FIRST + (channel & 7), 1);
                let mut y = fix32::from_words(2294, 0) - sink;
                if y < fix32::from_words(983, 0) {
                    y = fix32::from_words(983, 0);
                }
                material.set_uniform_color(demo_white());
                model.set_pose(demo_pose(
                    demo_point(tile_transform.position.x, y, tile_transform.position.z),
                    demo_zero(),
                    demo_point(pulse, pulse, pulse),
                ));
                counted_draw(pass, &base, model, material, &marked_view);
            }
        }
        i += 1;
    }
}
fn draw_pads(
    pass: &mut Pass,
    block: &Mesh,
    model: &mut Model,
    material: &mut Material,
    hatch: &mut Material,
) {
    draw_pad(pass, block, model, material, hatch, 1);
    draw_pad(pass, block, model, material, hatch, 0);
}
fn prepare_block(model: &mut Model) {
    let resting = visual_mode() == PLAY && MOTION[5u16] == 0;
    let key = (GAME[0u16] & 31) | ((GAME[1u16] & 31) << 5) | ((GAME[2u16] & 3) << 10) | 32768;
    if split_visual()
        || !resting
        || BLOCK_POSE_KEY[0u16] != key
        || BLOCK_POSE_KEY[1u16] != LEVEL_GENERATION[0u16]
    {
        block_pose(model);
    }
    BLOCK_POSE_KEY[0u16] = if resting && !split_visual() {
        key
    } else {
        0u16
    };
    BLOCK_POSE_KEY[1u16] = LEVEL_GENERATION[0u16];
}

fn draw_block(pass: &mut Pass, block: &Mesh, model: &mut Model, material: &Material) {
    if block_visible() {
        let texture = demo_texture();
        let view = texture.view();
        counted_draw(pass, block, model, material, &view);
    }
}
fn level_eye(which: u16) -> Vec3 {
    demo_point(
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
        demo_point(fix32::zero(), fix32::from_words(39322, 0), fix32::zero()),
        demo_point(fix32::zero(), fix32::from_int(1), fix32::zero()),
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
fn game_floor_model() -> Model {
    let half = fix32::from_words(32768, 0);
    Model::new(demo_pose(
        demo_zero(),
        demo_zero(),
        demo_point(half, half, half),
    ))
}
fn game_plain_model() -> Model {
    Model::new(demo_pose(demo_zero(), demo_zero(), demo_scale()))
}
fn game_screen_model() -> Model {
    let one = fix32::from_int(1);
    Model::new(demo_pose(
        demo_point(fix32::zero(), fix32::zero(), -one),
        demo_zero(),
        demo_point(one, fix32::from_words(39322, 0), one),
    ))
}

fn game_screen_state() -> RenderState {
    let screen_state = RenderState::new(RenderStateDesc {
        depth: DepthFunc::Always,
        depth_write: false,
        blend: BlendMode::SrcOver,
        filter: TextureFilter::Nearest,
        lighting: demo_lighting(demo_point(fix32::zero(), fix32::zero(), fix32::from_int(1))),
    });
    screen_state
}
fn game_solid_state() -> RenderState {
    let state = RenderState::new(RenderStateDesc {
        depth: DepthFunc::Less,
        depth_write: true,
        blend: BlendMode::Replace,
        filter: TextureFilter::Trilinear,
        lighting: demo_lighting(demo_point(
            fix32::from_words(16711, 0),
            fix32::from_words(33422, 0),
            fix32::from_words(16711, 0),
        )),
    });
    state
}
fn game_hint_state() -> RenderState {
    let hint_state = RenderState::new(RenderStateDesc {
        depth: DepthFunc::Less,
        depth_write: false,
        blend: BlendMode::SrcOver,
        filter: TextureFilter::Trilinear,
        lighting: demo_lighting(demo_point(
            fix32::from_words(16711, 0),
            fix32::from_words(33422, 0),
            fix32::from_words(16711, 0),
        )),
    });
    hint_state
}
fn game_shadow_state() -> RenderState {
    let shadow_state = RenderState::new(RenderStateDesc {
        depth: DepthFunc::Always,
        depth_write: false,
        blend: BlendMode::SrcOver,
        filter: TextureFilter::Nearest,
        lighting: demo_lighting(demo_point(fix32::zero(), fix32::zero(), fix32::from_int(1))),
    });
    shadow_state
}

fn submit_game(
    gpu: &mut Gpu,
    commands: CommandBuffer,
    finish: bool,
    begin: u16,
    started: bool,
) -> CommandBuffer {
    let resume_before = profiler_resume_epoch();
    gpu.submit(commands);
    let completed = gpu.wait();
    if finish {
        render_time_complete(begin, profiler_active_milliseconds(), started);
    }
    if profiler_resume_epoch() != resume_before {
        gamepad_capture();
        gamepad_discard_edges();
    }
    completed
}
fn record_static_scene(
    commands: &mut CommandBuffer,
    color: &ColorBuffer,
    depth: &DepthBuffer,
    background: &ColorBuffer,
    camera: &Camera,
    state: &RenderState,
) {
    let mut floor_model = game_floor_model();
    let mut floor_material = demo_material(demo_white(), false);
    let pad = demo_mesh_part(PAD_FIRST, PAD_COUNT);
    let mut pad_model = game_plain_model();
    let mut pad_material = demo_material(demo_white(), true);
    let mut hatch = demo_material(demo_white(), false);

    let load = scene_color_load(commands, background, color);
    let mut pass = commands.pass(color, depth, load, DepthLoad::clear(65535));
    pass.set_camera(camera);
    pass.set_state(state);
    draw_floor(&mut pass, &mut floor_model, &mut floor_material);
    draw_pad(
        &mut pass,
        &pad,
        &mut pad_model,
        &mut pad_material,
        &mut hatch,
        3,
    );
    draw_pad(
        &mut pass,
        &pad,
        &mut pad_model,
        &mut pad_material,
        &mut hatch,
        2,
    );
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
    demo_number(grid, 6, 0, 2, MOTION[6u16] + 1);
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
            demo_number(grid, 5, 1, 4, HUD[1u16]);
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
            demo_number(grid, 3, 2, 5, FRAME_RENDER_MS[0u16]);
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
    let mut gpu = Gpu::init(demo_config());
    let mut commands = gpu.command_buffer();
    let mut scene_commands = gpu.command_buffer();
    let ground_owner = ground_begin();
    let mut scene_generation: u16 = 0;
    let mut scene_background: u16 = 2;
    let mut static_draws: u16 = 0;
    let offscreen = ColorBuffer::from_memory(unsafe { GpuMemory::from_sectors(0xc000, 2048) });
    let background =
        ColorBuffer::from_memory(unsafe { GpuMemory::from_sectors(BLOX_BACKGROUND_SECTOR, 1600) });
    let color_a = demo_color_a();
    let color_b = demo_color_b();
    let depth = demo_depth();
    let mut display = Display::open();
    let mut camera = game_camera();
    let mut camera_level: u16 = 0;
    let screen_camera = demo_camera();
    let screen_state = game_screen_state();
    let state = game_solid_state();
    let hint_state = game_hint_state();
    let block = demo_mesh_part(BLOCK_FIRST, BLOCK_COUNT);
    let pad = demo_mesh_part(PAD_FIRST, PAD_COUNT);
    let shadow_mesh = demo_mesh_part(SHADOW_FIRST, SHADOW_COUNT);
    let mut shadow_model = game_plain_model();
    let shadow_material = demo_material(
        Rgba8 {
            r: 10,
            g: 13,
            b: 18,
            a: 87,
        },
        true,
    );
    let shadow_state = game_shadow_state();
    let screen = demo_mesh_part(SCREEN_FIRST, SCREEN_COUNT);
    let mut floor_model = game_floor_model();
    let mut block_model = game_plain_model();
    let mut idle_model = game_plain_model();
    let idle_material = demo_material(
        Rgba8 {
            r: 154,
            g: 147,
            b: 140,
            a: 255,
        },
        false,
    );
    let mut pad_model = game_plain_model();
    let screen_model = game_screen_model();
    let material = demo_material(demo_white(), false);
    let mut floor_material = demo_material(
        Rgba8 {
            r: 82,
            g: 100,
            b: 121,
            a: 255,
        },
        false,
    );
    let mut pad_material = demo_material(demo_white(), true);
    let mut hatch_material = demo_material(demo_white(), false);
    let mut tint_material = demo_material(demo_white(), true);
    let mut grid = demo_grid(false);
    session_init();
    shadow_invalidate();
    BLOCK_POSE_KEY[0u16] = 0;
    gamepad_init();
    let mut previous = profiler_active_milliseconds();
    let mut render_start: u16 = 0;
    let mut render_started = false;
    let mut number: u16 = 0;
    let mut back_b = true;
    loop {
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
        let now = profiler_active_milliseconds();
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
            camera_level = MOTION[6u16];
            camera.set_look_at_y_up(
                level_eye(camera_level),
                demo_point(fix32::zero(), fix32::from_words(39322, 0), fix32::zero()),
            );
            camera.set_projection(level_projection(camera_level));
            web_camera_axes(&mut camera);
        }
        DRAW_CALLS[0u16] = 0;
        if scene_generation != LEVEL_GENERATION[0u16] || scene_background != BACKGROUND[0u16] {
            scene_commands.reset();
            if ground_enabled() && !ground_matches_level() {
                commands.reset();
                ground_rebuild();
            }
            record_static_scene(
                &mut scene_commands,
                &offscreen,
                &depth,
                &background,
                &camera,
                &state,
            );
            static_draws = DRAW_CALLS[0u16];
            scene_generation = LEVEL_GENERATION[0u16];
            scene_background = BACKGROUND[0u16];
        }
        DRAW_CALLS[0u16] = static_draws;
        scene_commands = submit_game(&mut gpu, scene_commands, false, 0, false);
        let fade = fading();
        let color = tint();
        if automatic_capture {
            demo_number(&mut grid, 0, 0, 4, GAME[5u16]);
            demo_number(&mut grid, 6, 0, 1, input.connected);
        }
        {
            if automatic_capture && fade {
                capture_report(66, number);
            }
            commands.reset();
            {
                let mut pass =
                    commands.pass(&offscreen, &depth, ColorLoad::load(), DepthLoad::load());
                pass.set_camera(&camera);
                pass.set_state(&state);
                prepare_block(&mut block_model);
                draw_bridges(
                    &mut pass,
                    &mut floor_model,
                    &mut floor_material,
                    &state,
                    &hint_state,
                );
                draw_fragile(&mut pass, &mut pad_model, &mut hatch_material);
                draw_switches(&mut pass, &mut pad_model, &mut hatch_material);
                draw_pads(
                    &mut pass,
                    &pad,
                    &mut pad_model,
                    &mut pad_material,
                    &mut hatch_material,
                );
                pass.set_state(&shadow_state);
                draw_shadow(
                    &mut pass,
                    &shadow_mesh,
                    &mut shadow_model,
                    &shadow_material,
                    &block_model,
                );
                if split_visual() {
                    idle_model.set_pose(idle_cube_transform());
                    draw_shadow(
                        &mut pass,
                        &shadow_mesh,
                        &mut shadow_model,
                        &shadow_material,
                        &idle_model,
                    );
                }
                pass.set_state(&state);
                draw_block(&mut pass, &block, &mut block_model, &material);
                if split_visual() {
                    draw_block(&mut pass, &block, &mut idle_model, &idle_material);
                }
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
            if fade {
                commands = submit_game(&mut gpu, commands, false, 0, false);
                session_captured();
                signal(11, SESSION[4u16]);
                if automatic_capture {
                    capture_report(67, number);
                    demo_number(&mut grid, 0, 0, 4, 1000 + color);
                }
            }
        }
        if fade {
            commands.reset();
        }
        let back = if back_b { &color_b } else { &color_a };
        let mut color_load = ColorLoad::clear(Rgb565::from_raw(0x1107));
        if fade {
            commands.invalidate_textures();
        } else {
            commands.copy_color(&offscreen, back);
            color_load = ColorLoad::load();
        }
        {
            let mut pass = commands.pass(back, &depth, color_load, DepthLoad::load());
            if fade {
                tint_material.set_uniform_color(Rgba8 {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: color as u8,
                });
                let view = offscreen.texture_view();
                pass.set_camera(&screen_camera);
                pass.set_state(&screen_state);
                counted_draw(&mut pass, &screen, &screen_model, &tint_material, &view);
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
        display.set_next_swap(back);
        display.wait_vblank();
        render_start = profiler_active_milliseconds();
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
