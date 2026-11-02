//! First-two-stage Bloxorz renderer with live colored RTT transitions.
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

pub static BACKGROUND: Buf<u16, 1> = Buf::new([1]);
// One caller-owned block Model lives for this run; retain only its resting key.
static BLOCK_POSE_KEY: Buf<u16, 1> = Buf::new([0]);
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
fn plane_floor() -> Mesh {
    let first = if MOTION[6u16] == 0 {
        FLOOR0_FIRST
    } else {
        FLOOR1_FIRST
    };
    let count = if MOTION[6u16] == 0 {
        FLOOR0_COUNT
    } else {
        FLOOR1_COUNT
    };
    return demo_mesh_part(first, count);
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
    pass.draw(mesh, model, material, &view);
}
fn draw_pad(
    pass: &mut Pass,
    box_mesh: &Mesh,
    model: &mut Model,
    material: &mut Material,
    hatch_material: &mut Material,
    start: bool,
) {
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
    pass.draw(&well, model, material, &view);
    let hatch_visible = if start {
        mode != ENTRY_OPEN && mode != ENTRY_RISE
    } else {
        mode <= GOAL_OPEN || mode > GOAL_CLOSE
    };
    if hatch_visible {
        let hatch = demo_mesh_part(TILE0_FIRST, TILE0_COUNT);
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
                fix32::from_words(32604, 0),
                half,
                fix32::from_words(32604, 0),
            ),
        ));
        pass.draw(&hatch, model, hatch_material, &view);
    }
    // The entrance ring vanishes at the rim and stays hidden after entry.
    if start && mode != ENTRY_OPEN {
        return;
    }
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
    let colors: Buf<u16, 16> = Buf::new([
        BLOX_CHANNEL0_DIM_RG,
        BLOX_CHANNEL0_DIM_B,
        BLOX_CHANNEL0_BASE_RG,
        BLOX_CHANNEL0_BASE_B,
        BLOX_CHANNEL0_GLOW_RG,
        BLOX_CHANNEL0_GLOW_B,
        BLOX_CHANNEL0_INK_RG,
        BLOX_CHANNEL0_INK_B,
        BLOX_CHANNEL1_DIM_RG,
        BLOX_CHANNEL1_DIM_B,
        BLOX_CHANNEL1_BASE_RG,
        BLOX_CHANNEL1_BASE_B,
        BLOX_CHANNEL1_GLOW_RG,
        BLOX_CHANNEL1_GLOW_B,
        BLOX_CHANNEL1_INK_RG,
        BLOX_CHANNEL1_INK_B,
    ]);
    let at = channel * 8 + shade * 2;
    Rgba8 {
        r: colors[at] as u8,
        g: (colors[at] >> 8) as u8,
        b: colors[at + 1] as u8,
        a: 255,
    }
}
fn draw_floor(pass: &mut Pass, floor: &Mesh, model: &Model, material: &mut Material) {
    let texture = demo_texture();
    let view = texture.view();
    material.set_uniform_color(Rgba8 {
        r: 82,
        g: 100,
        b: 121,
        a: 255,
    });
    pass.draw(floor, model, material, &view);
}
fn draw_bridges(
    pass: &mut Pass,
    model: &Model,
    material: &mut Material,
    state: &RenderState,
    hint_state: &RenderState,
) {
    let texture = demo_texture();
    let view = texture.view();
    if MOTION[6u16] == 1 {
        let mut channel: u16 = 0;
        while channel < 2 {
            if visual_mask() & (1 << channel) != 0 {
                material.set_uniform_color(channel_color(channel, 1));
                if channel == 0 {
                    let mesh = demo_mesh_part(BRIDGE0_FIRST, BRIDGE0_COUNT);
                    pass.draw(&mesh, model, material, &view);
                } else {
                    let mesh = demo_mesh_part(BRIDGE1_FIRST, BRIDGE1_COUNT);
                    pass.draw(&mesh, model, material, &view);
                }
            } else {
                let first = if channel == 0 {
                    HINT0_FIRST
                } else {
                    HINT1_FIRST
                };
                let count = if channel == 0 {
                    HINT0_COUNT
                } else {
                    HINT1_COUNT
                };
                let mesh = demo_mesh_part(first, count);
                let mut color = channel_color(channel, 1);
                color.a = 23;
                material.set_uniform_color(color);
                pass.set_state(hint_state);
                pass.draw(&mesh, model, material, &view);
                pass.set_state(state);
            }
            channel += 1;
        }
    }
}
fn draw_switches(
    pass: &mut Pass,
    model: &mut Model,
    tile_material: &mut Material,
    glyph_material: &mut Material,
) {
    let texture = demo_texture();
    let view = texture.view();
    let half = fix32::from_words(32768, 0);
    let mut i: u16 = 0;
    while i < LEVEL[6u16] {
        let at = 8 + i * 8;
        let kind = LEVEL[at];
        if kind == 3 || kind == 4 {
            let x = i % LEVEL[0u16];
            let z = div_u16(i, LEVEL[0u16]);
            let cx = fix32::from_int(x as i16) - fix32::from_int(LEVEL[0u16] as i16) * half + half;
            let cz = fix32::from_int(z as i16) - fix32::from_int(LEVEL[1u16] as i16) * half + half;
            let bit = LEVEL[at + 1];
            let channel = if bit == 2 { 1u16 } else { 0u16 };
            let active = visual_mask() & bit != 0;
            tile_material
                .set_uniform_color(channel_color(channel, if active { 1u16 } else { 0u16 }));
            let first = if kind == 3 { TILE0_FIRST } else { TILE1_FIRST };
            let count = if kind == 3 { TILE0_COUNT } else { TILE1_COUNT };
            let tile = demo_mesh_part(first, count);
            model.set_pose(demo_pose(
                demo_point(cx, fix32::zero(), cz),
                demo_zero(),
                demo_point(half, half, half),
            ));
            pass.draw(&tile, model, tile_material, &view);
            let one = fix32::from_int(1);
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
            if kind == 4 {
                let base = demo_mesh_part(HEAVY_BASE_FIRST, HEAVY_BASE_COUNT);
                let mut y = fix32::from_words(2294, 0) - sink;
                if y < fix32::from_words(983, 0) {
                    y = fix32::from_words(983, 0);
                }
                tile_material.set_uniform_color(channel_color(channel, 0));
                model.set_pose(demo_pose(
                    demo_point(cx, y, cz),
                    demo_zero(),
                    demo_point(pulse, pulse, pulse),
                ));
                pass.draw(&base, model, tile_material, &view);
            }
            glyph_material.set_unlit(true);
            glyph_material.set_uniform_color(channel_color(channel, 3));
            let first = if kind == 3 {
                SOFT_GLYPH_FIRST
            } else {
                HEAVY_GLYPH_FIRST
            };
            let count = if kind == 3 {
                SOFT_GLYPH_COUNT
            } else {
                HEAVY_GLYPH_COUNT
            };
            let glyph = demo_mesh_part(first, count);
            let mut y = if kind == 3 {
                fix32::from_words(2621, 0)
            } else {
                fix32::from_words(4915, 0)
            };
            y = y - sink;
            if y < fix32::from_words(983, 0) {
                y = fix32::from_words(983, 0);
            }
            model.set_pose(demo_pose(
                demo_point(cx, y, cz),
                demo_zero(),
                demo_point(pulse, pulse, pulse),
            ));
            pass.draw(&glyph, model, glyph_material, &view);
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
    draw_pad(pass, block, model, material, hatch, true);
    draw_pad(pass, block, model, material, hatch, false);
}
fn prepare_block(model: &mut Model) {
    let resting = visual_mode() == PLAY && MOTION[5u16] == 0;
    // The first-two-stage anchors/pose/index fit4+3+2+1 bits. Bit15 marks
    // validity; animated frames clear it so their final pose is refreshed.
    let key = (GAME[0u16] & 15)
        | ((GAME[1u16] & 7) << 4)
        | ((GAME[2u16] & 3) << 7)
        | ((MOTION[6u16] & 1) << 9)
        | 32768;
    if !resting || BLOCK_POSE_KEY[0u16] != key {
        block_pose(model);
    }
    BLOCK_POSE_KEY[0u16] = if resting { key } else { 0u16 };
}
fn draw_block(pass: &mut Pass, block: &Mesh, model: &mut Model, material: &Material) {
    if block_visible() {
        let texture = demo_texture();
        let view = texture.view();
        pass.draw(block, model, material, &view);
    }
}
fn game_camera() -> Camera {
    let mut camera = Camera::look_at(
        demo_point(
            fix32::from_words(BLOX_CAMERA0_EYE_X_LOW, BLOX_CAMERA0_EYE_X_HIGH),
            fix32::from_words(BLOX_CAMERA0_EYE_Y_LOW, BLOX_CAMERA0_EYE_Y_HIGH),
            fix32::from_words(BLOX_CAMERA0_EYE_Z_LOW, BLOX_CAMERA0_EYE_Z_HIGH),
        ),
        demo_point(fix32::zero(), fix32::from_words(39322, 0), fix32::zero()),
        demo_point(fix32::zero(), fix32::from_int(1), fix32::zero()),
        Projection::from_focal_pixels(
            fix32::from_words(BLOX_CAMERA0_FOCAL_LOW, BLOX_CAMERA0_FOCAL_HIGH),
            fix32::from_int(199),
        ),
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

fn run(limit: u16, automatic_capture: bool, capture_brightness: u16) {
    let mut gpu = Gpu::init(demo_config());
    let mut commands = gpu.command_buffer();
    let offscreen = ColorBuffer::from_memory(unsafe { GpuMemory::from_sectors(0xc000, 2048) });
    let offscreen_depth =
        DepthBuffer::from_memory(unsafe { GpuMemory::from_sectors(0xc800, 2048) });
    let background =
        ColorBuffer::from_memory(unsafe { GpuMemory::from_sectors(BLOX_BACKGROUND_SECTOR, 1600) });
    let color_a = demo_color_a();
    let color_b = demo_color_b();
    let depth = demo_depth();
    let mut display = Display::open();
    let mut camera = game_camera();
    let mut camera_level: u16 = 0;
    let screen_camera = demo_camera();
    let screen_state = RenderState::new(RenderStateDesc {
        depth: DepthFunc::Always,
        depth_write: false,
        blend: BlendMode::SrcOver,
        filter: TextureFilter::Nearest,
        lighting: demo_lighting(demo_point(fix32::zero(), fix32::zero(), fix32::from_int(1))),
    });
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
    let shadow_state = RenderState::new(RenderStateDesc {
        depth: DepthFunc::LessEqual,
        depth_write: false,
        blend: BlendMode::SrcOver,
        filter: TextureFilter::Nearest,
        lighting: demo_lighting(demo_point(fix32::zero(), fix32::zero(), fix32::from_int(1))),
    });
    let screen = demo_mesh_part(SCREEN_FIRST, SCREEN_COUNT);
    let floor_model = game_floor_model();
    let mut block_model = game_plain_model();
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
    let mut number: u16 = 0;
    let mut back_b = true;
    loop {
        signal(4, number);
        if automatic_capture {
            capture_report(65, number);
        }
        gamepad_capture();
        let input = gamepad_take_frame();
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
            let mut eye = demo_point(
                fix32::from_words(BLOX_CAMERA0_EYE_X_LOW, BLOX_CAMERA0_EYE_X_HIGH),
                fix32::from_words(BLOX_CAMERA0_EYE_Y_LOW, BLOX_CAMERA0_EYE_Y_HIGH),
                fix32::from_words(BLOX_CAMERA0_EYE_Z_LOW, BLOX_CAMERA0_EYE_Z_HIGH),
            );
            if camera_level == 1 {
                eye = demo_point(
                    fix32::from_words(BLOX_CAMERA1_EYE_X_LOW, BLOX_CAMERA1_EYE_X_HIGH),
                    fix32::from_words(BLOX_CAMERA1_EYE_Y_LOW, BLOX_CAMERA1_EYE_Y_HIGH),
                    fix32::from_words(BLOX_CAMERA1_EYE_Z_LOW, BLOX_CAMERA1_EYE_Z_HIGH),
                );
            }
            camera.set_look_at_y_up(
                eye,
                demo_point(fix32::zero(), fix32::from_words(39322, 0), fix32::zero()),
            );
            web_camera_axes(&mut camera);
        }
        let floor = plane_floor();
        let fade = fading();
        let color = tint();
        if automatic_capture {
            demo_number(&mut grid, 0, 0, 4, GAME[5u16]);
            demo_number(&mut grid, 6, 0, 1, input.connected);
        }
        if fade {
            if automatic_capture {
                capture_report(66, number);
            }
            commands.reset();
            let color_load = scene_color_load(&mut commands, &background, &offscreen);
            {
                let mut pass = commands.pass(
                    &offscreen,
                    &offscreen_depth,
                    color_load,
                    DepthLoad::clear(65535),
                );
                pass.set_camera(&camera);
                pass.set_state(&state);
                prepare_block(&mut block_model);
                draw_floor(&mut pass, &floor, &floor_model, &mut floor_material);
                draw_bridges(
                    &mut pass,
                    &floor_model,
                    &mut floor_material,
                    &state,
                    &hint_state,
                );
                draw_switches(
                    &mut pass,
                    &mut pad_model,
                    &mut hatch_material,
                    &mut pad_material,
                );
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
                pass.set_state(&state);
                draw_block(&mut pass, &block, &mut block_model, &material);
                if automatic_capture {
                    pass.draw_ui(
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
            let resume_before = profiler_resume_epoch();
            gpu.submit(commands);
            commands = gpu.wait();
            if profiler_resume_epoch() != resume_before {
                gamepad_capture();
                gamepad_discard_edges();
            }
            session_captured();
            signal(11, SESSION[4u16]);
            if automatic_capture {
                capture_report(67, number);
                demo_number(&mut grid, 0, 0, 4, 1000 + color);
            }
        }
        commands.reset();
        let back = if back_b { &color_b } else { &color_a };
        let mut color_load = ColorLoad::clear(Rgb565::from_raw(0x1107));
        if fade {
            commands.invalidate_textures();
        } else {
            color_load = scene_color_load(&mut commands, &background, back);
        }
        {
            let mut pass = commands.pass(back, &depth, color_load, DepthLoad::clear(65535));
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
                pass.draw(&screen, &screen_model, &tint_material, &view);
                if automatic_capture {
                    pass.draw_ui(
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
                pass.set_camera(&camera);
                pass.set_state(&state);
                prepare_block(&mut block_model);
                draw_floor(&mut pass, &floor, &floor_model, &mut floor_material);
                draw_bridges(
                    &mut pass,
                    &floor_model,
                    &mut floor_material,
                    &state,
                    &hint_state,
                );
                draw_switches(
                    &mut pass,
                    &mut pad_model,
                    &mut hatch_material,
                    &mut pad_material,
                );
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
                pass.set_state(&state);
                draw_block(&mut pass, &block, &mut block_model, &material);
                if automatic_capture {
                    pass.draw_ui(
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
        }
        if automatic_capture {
            capture_report(68, color);
        }
        let resume_before = profiler_resume_epoch();
        gpu.submit(commands);
        if automatic_capture {
            capture_report(69, color);
        }
        commands = gpu.wait();
        if profiler_resume_epoch() != resume_before {
            gamepad_capture();
            gamepad_discard_edges();
        }
        display.set_next_swap(back);
        display.wait_vblank();
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
