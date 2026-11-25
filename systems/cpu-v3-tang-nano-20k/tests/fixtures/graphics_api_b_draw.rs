use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_scene_math;
use gpu_scene_math::*;
mod gpu_scene_objects;
use gpu_scene_objects::*;
mod gpu_scene_transforms;
use gpu_scene_transforms::*;

static CAMERA: Buf<u16, 48> = Buf::new([0xbeef; 48]);
static MODEL: Buf<u16, 48> = Buf::new([0xbeef; 48]);
static MATERIAL: Buf<u16, 16> = Buf::new([0; 16]);
static STATE: Buf<u16, 16> = Buf::new([0; 16]);
static VIEW: Buf<u16, 16> = Buf::new([0; 16]);
static DESIRED: Buf<u16, 48> = Buf::new([0; 48]);
static INPUTS: Buf<u16, 128> = Buf::new([0; 128]);
static DRAW_A: Buf<u16, 256> = Buf::new([0xaced; 256]);
static DRAW_B: Buf<u16, 256> = Buf::new([0xaced; 256]);
static ROTATED_MODEL: Buf<u16, 48> = Buf::new([0; 48]);
static PROJECTION: Buf<u16, 6> = Buf::new([0; 6]);
static DIAGONAL_LIGHT: Buf<u16, 3> = Buf::new([0; 3]);

fn check(ok: bool, code: u16) {
    if !ok {
        halt(code);
    }
}
fn vector_word_span_checks() {
    let bits: Buf<u16, 6> = Buf::new([65535, 32767, 0, 32768, 1, 65535]);
    let mut input: Buf<u16, 40> = Buf::new([0xa55a; 40]);
    let output: Buf<u16, 40> = Buf::new([0xa55a; 40]);
    let mut at: u16 = 0;
    while at < 6 {
        input[7 + at] = bits[at];
        at += 1;
    }
    let source = unsafe { input.as_array().as_ptr().add(8).as_u16_array() };
    let value = gpu_scene_vec_read(source, 65535);
    check(
        value.x().lo_bits() == 65535 && value.x().hi_bits() == 32767,
        60,
    );
    check(value.y().lo_bits() == 0 && value.y().hi_bits() == 32768, 61);
    check(value.z().lo_bits() == 1 && value.z().hi_bits() == 65535, 62);
    // Odd, line-crossing and wrapping offsets must still touch only six words.
    gpu_scene_vec_write(output.as_array(), 15, value);
    let destination = unsafe { output.as_array().as_ptr().add(8).as_u16_array() };
    gpu_scene_vec_write(destination, 65535, value);
    at = 0;
    while at < 40 {
        let mut expected: u16 = 0xa55a;
        if at >= 7 && at < 13 {
            expected = bits[at - 7];
        }
        check(input[at] == expected, 63);
        if at >= 15 && at < 21 {
            expected = bits[at - 15];
        }
        check(output[at] == expected, 64);
        at += 1;
    }
}
fn bind() {
    let mut at: u16 = 0;
    while at < 32 {
        DESIRED[at] = CAMERA[at];
        at += 1;
    }
    at = 0;
    while at < 16 {
        DESIRED[32 + at] = STATE[at];
        at += 1;
    }
}
fn snapshot() {
    let mut at: u16 = 0;
    while at < 48 {
        INPUTS[at] = DESIRED[at];
        INPUTS[48 + at] = MODEL[at];
        at += 1;
    }
    at = 0;
    while at < 16 {
        INPUTS[96 + at] = MATERIAL[at];
        INPUTS[112 + at] = VIEW[at];
        at += 1;
    }
}
fn unchanged() {
    let mut at: u16 = 0;
    while at < 48 {
        check(
            INPUTS[at] == DESIRED[at] && INPUTS[48 + at] == MODEL[at],
            40,
        );
        at += 1;
    }
    at = 0;
    while at < 16 {
        check(
            INPUTS[96 + at] == MATERIAL[at] && INPUTS[112 + at] == VIEW[at],
            41,
        );
        at += 1;
    }
}
fn model_initial() {
    let zero = fix32::zero();
    let origin: Vec3 = Vec3 {
        x: zero,
        y: zero,
        z: zero,
    };
    let transform: Pose = Pose {
        position: Vec3 {
            x: fix32::from_int(10),
            y: fix32::from_int(20),
            z: fix32::from_int(-30),
        },
        angles: origin,
        scale: Vec3 {
            x: fix32::from_int(2),
            y: fix32::from_int(4),
            z: fix32::from_int(8),
        },
    };
    check(gpu_scene_model_init(MODEL.as_array(), transform), 1);
    let world: Mat3x4 = gpu_scene_model_world(MODEL.as_array());
    check(
        world.row0.x == fix32::from_int(2) && world.row2.w == fix32::from_int(-30),
        2,
    );
    check(
        gpu_scene_fix_read(MODEL.as_array(), 24) == fix32::from_words(32768, 0)
            && gpu_scene_fix_read(MODEL.as_array(), 32) == fix32::from_words(16384, 0)
            && gpu_scene_fix_read(MODEL.as_array(), 40) == fix32::from_words(8192, 0),
        3,
    );
}
fn material_state() {
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    let material: MaterialDesc = MaterialDesc {
        uniform_color: Rgba8 {
            r: 17,
            g: 34,
            b: 51,
            a: 68,
        },
        specular: Rgb8 {
            r: 85,
            g: 102,
            b: 119,
        },
        ambient_response: fix32::from_words(32768, 0),
        directional_response: fix32::from_words(16384, 0),
        shininess: Shininess::S16,
        unlit: false,
    };
    check(gpu_scene_material_init(MATERIAL.as_array(), material), 4);
    let state: RenderStateDesc = RenderStateDesc {
        depth: DepthFunc::Less,
        depth_write: true,
        blend: BlendMode::Replace,
        filter: TextureFilter::Bilinear,
        lighting: Lighting {
            direction: Vec3 {
                x: one,
                y: zero,
                z: zero,
            },
            ambient: fix32::from_words(49152, 0),
            directional: fix32::from_words(32768, 0),
        },
    };
    check(gpu_scene_state_init(STATE.as_array(), state), 5);
    let texture: TextureDesc = TextureDesc {
        format: TextureFormat::Rgb565,
        memory: GpuMemory {
            first_sector: 32768,
            sector_count: 4,
        },
        size_log2: 0,
        full_mip: false,
    };
    check(gpu_scene_texture_init(VIEW.as_array(), texture), 6);
}
fn camera_a() {
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    let origin: Vec3 = Vec3 {
        x: zero,
        y: zero,
        z: zero,
    };
    check(
        gpu_scene_projection_focal(
            PROJECTION.as_array(),
            fix32::from_int(256),
            fix32::from_int(128),
        ),
        21,
    );
    let projection: Projection = gpu_scene_projection_value(PROJECTION.as_array());
    let camera: CameraDesc = CameraDesc {
        eye: Vec3 {
            x: one,
            y: fix32::from_int(2),
            z: fix32::from_int(3),
        },
        angles: origin,
        projection: projection,
    };
    check(gpu_scene_camera_init(CAMERA.as_array(), camera), 7);
    bind();
    snapshot();
    check(
        gpu_scene_draw_prepare(
            DESIRED.as_array(),
            MODEL.as_array(),
            MATERIAL.as_array(),
            VIEW.as_array(),
            true,
            DRAW_A.as_array(),
        ),
        8,
    );
    unchanged();
    check(gpu_scene_camera_pose(CAMERA.as_array(), origin, origin), 9);
    check(DESIRED[6u16] == 0 && DESIRED[7u16] == 65535, 10);
}
fn camera_b() {
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    check(
        gpu_scene_projection_focal(
            PROJECTION.as_array(),
            fix32::from_int(512),
            fix32::from_int(128),
        ),
        20,
    );
    let second: Projection = gpu_scene_projection_value(PROJECTION.as_array());
    check(second.focal_pixels() == fix32::from_int(512), 11);
    gpu_scene_camera_projection(CAMERA.as_array(), second);
    let mut view: Mat3x4 = Mat3x4 {
        row0: Vec4 {
            x: zero,
            y: zero,
            z: one,
            w: zero,
        },
        row1: Vec4 {
            x: zero,
            y: one,
            z: zero,
            w: zero,
        },
        row2: Vec4 {
            x: -one,
            y: zero,
            z: zero,
            w: zero,
        },
    };
    check(
        gpu_scene_camera_view(CAMERA.as_array(), view_of(&mut view)),
        12,
    );
    let source: Mat3x4 = gpu_scene_camera_get_view(CAMERA.as_array());
    check(source.row0.z == one && source.row2.x == -one, 13);
    bind();
    snapshot();
    check(
        gpu_scene_draw_prepare(
            DESIRED.as_array(),
            MODEL.as_array(),
            MATERIAL.as_array(),
            VIEW.as_array(),
            true,
            DRAW_B.as_array(),
        ),
        14,
    );
    unchanged();
}
fn look_at_checks() {
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    let origin: Vec3 = Vec3 {
        x: zero,
        y: zero,
        z: zero,
    };
    check(
        !gpu_scene_camera_look_at_y_up(CAMERA.as_array(), origin, origin),
        15,
    );
    let before: Mat3x4 = gpu_scene_camera_get_view(CAMERA.as_array());
    check(before.row0.z == one, 16);
    let target: Vec3 = Vec3 {
        x: zero,
        y: zero,
        z: fix32::from_int(-3),
    };
    let up: Vec3 = Vec3 {
        x: zero,
        y: one,
        z: zero,
    };
    let projection: Projection = gpu_scene_projection_value(PROJECTION.as_array());
    check(
        gpu_scene_camera_init_look_at(CAMERA.as_array(), origin, target, up, projection),
        17,
    );
    let mut at: u16 = 31;
    while at < 48 {
        check(CAMERA[at] == 0, 36);
        at += 1;
    }
    let mut aligned: Mat3x4 = gpu_scene_camera_get_view(CAMERA.as_array());
    let tolerance = fix32::from_words(8, 0);
    check(
        (aligned.row0.x - one).abs() < tolerance
            && (aligned.row1.y - one).abs() < tolerance
            && (aligned.row2.z - one).abs() < tolerance,
        18,
    );
    check(
        gpu_scene_camera_view(CAMERA.as_array(), view_of(&mut aligned)),
        19,
    );
}
fn projection_checks() {
    check(
        !gpu_scene_projection_focal(
            PROJECTION.as_array(),
            fix32::from_int(120),
            fix32::from_int(128),
        ),
        22,
    );
    check(
        gpu_scene_projection_fov(
            PROJECTION.as_array(),
            fix32::from_words(37407, 1),
            fix32::from_int(128),
            true,
        ),
        24,
    );
    let value: Projection = gpu_scene_projection_value(PROJECTION.as_array());
    check(
        (value.focal_pixels() - fix32::from_int(200)).abs() < fix32::from_words(1024, 0),
        23,
    );
    check(
        !gpu_scene_projection_fov(
            PROJECTION.as_array(),
            fix32::zero(),
            fix32::from_int(128),
            false,
        ),
        32,
    );
    check(
        gpu_scene_projection_fov(
            PROJECTION.as_array(),
            fix32::from_words(3093, 1),
            fix32::from_int(128),
            false,
        ),
        33,
    );
    let vertical: Projection = gpu_scene_projection_value(PROJECTION.as_array());
    check(
        vertical.focal_pixels() > fix32::from_int(207)
            && vertical.focal_pixels() < fix32::from_int(209),
        34,
    );
}
fn diagonal_checks() {
    let one = fix32::from_int(1);
    let light: Lighting = Lighting {
        direction: Vec3 {
            x: one,
            y: one,
            z: one,
        },
        ambient: fix32::from_words(49152, 0),
        directional: fix32::from_words(32768, 0),
    };
    check(gpu_scene_state_lighting(STATE.as_array(), light), 29);
    bind();
    let absent = unsafe { Ptr::from_addr(0xffff).as_u16_array() };
    check(
        gpu_scene_draw_prepare(
            DESIRED.as_array(),
            MODEL.as_array(),
            MATERIAL.as_array(),
            absent,
            false,
            DRAW_A.as_array(),
        ),
        30,
    );
    check(DRAW_A[32u16] == 0 && DRAW_A[33u16] == 0, 31);
    DIAGONAL_LIGHT[0u16] = DRAW_A[2u16];
    DIAGONAL_LIGHT[1u16] = DRAW_A[3u16];
    DIAGONAL_LIGHT[2u16] = DRAW_A[4u16];
}
fn model_matrix_checks() {
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    let origin: Vec3 = Vec3 {
        x: zero,
        y: zero,
        z: zero,
    };
    let rotated: Pose = Pose {
        position: origin,
        angles: Vec3 {
            x: zero,
            y: zero,
            z: fix32::from_words(37407, 1),
        },
        scale: Vec3 {
            x: fix32::from_int(2),
            y: fix32::from_int(4),
            z: fix32::from_int(8),
        },
    };
    check(gpu_scene_model_init(ROTATED_MODEL.as_array(), rotated), 25);
    let mut world: Mat3x4 = gpu_scene_model_world(MODEL.as_array());
    let mut normal: Mat3x3 = Mat3x3 {
        row0: Vec3 {
            x: fix32::from_words(32768, 0),
            y: zero,
            z: zero,
        },
        row1: Vec3 {
            x: zero,
            y: fix32::from_words(16384, 0),
            z: zero,
        },
        row2: Vec3 {
            x: zero,
            y: zero,
            z: fix32::from_words(8192, 0),
        },
    };
    check(
        gpu_scene_model_matrices(MODEL.as_array(), view_of(&mut world), view_of(&mut normal)),
        26,
    );
    let singular: Pose = Pose {
        position: origin,
        angles: origin,
        scale: Vec3 {
            x: zero,
            y: one,
            z: one,
        },
    };
    check(!gpu_scene_model_init(MODEL.as_array(), singular), 27);
    check(
        gpu_scene_fix_read(MODEL.as_array(), 0) == fix32::from_int(2),
        28,
    );
}
fn mutate_after_draw() {
    let zero = fix32::zero();
    let transform: Pose = Pose {
        position: Vec3 {
            x: fix32::from_int(50),
            y: zero,
            z: zero,
        },
        angles: Vec3 {
            x: zero,
            y: zero,
            z: zero,
        },
        scale: Vec3 {
            x: fix32::from_int(5),
            y: fix32::from_int(6),
            z: fix32::from_int(7),
        },
    };
    check(gpu_scene_model_init(MODEL.as_array(), transform), 35);
    gpu_scene_material_color(
        MATERIAL.as_array(),
        Rgba8 {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
    );
    gpu_scene_state_depth(STATE.as_array(), DepthFunc::Never, false);
}
fn run_cases() {
    vector_word_span_checks();
    model_initial();
    material_state();
    camera_a();
    camera_b();
    diagonal_checks();
    look_at_checks();
    projection_checks();
    model_matrix_checks();
    material_state();
    camera_a();
    mutate_after_draw();
}
