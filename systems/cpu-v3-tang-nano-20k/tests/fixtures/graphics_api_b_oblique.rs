static OB_MODEL: Buf<u16, 48> = Buf::new([0xbeef; 48]);
static OB_EULER: Buf<u16, 48> = Buf::new([0xbeef; 48]);
static OB_LOOK: Buf<u16, 48> = Buf::new([0xbeef; 48]);
static OB_DESIRED: Buf<u16, 48> = Buf::new([0; 48]);
static OB_NATIVE_EULER: Buf<u16, 256> = Buf::new([0xaced; 256]);
static OB_NATIVE_LOOK: Buf<u16, 256> = Buf::new([0xaced; 256]);

fn oblique_prepare(camera: Array<u16>, native: Array<u16>) {
    let mut at: u16 = 0;
    while at < 32 {
        OB_DESIRED[at] = camera[at];
        at += 1;
    }
    at = 0;
    while at < 16 {
        OB_DESIRED[32 + at] = STATE[at];
        at += 1;
    }
    check(
        gpu_scene_draw_prepare(
            OB_DESIRED.as_array(),
            OB_MODEL.as_array(),
            MATERIAL.as_array(),
            VIEW.as_array(),
            true,
            native,
        ),
        80,
    );
}
fn main() {
    material_state();
    let one = fix32::from_int(1);
    let half = fix32::from_words(32768, 0);
    let light: Lighting = Lighting {
        direction: Vec3 {
            x: one,
            y: fix32::from_int(2),
            z: fix32::from_int(-3),
        },
        ambient: fix32::from_words(49152, 0),
        directional: half,
    };
    check(gpu_scene_state_lighting(STATE.as_array(), light), 81);
    check(
        gpu_scene_projection_focal(
            PROJECTION.as_array(),
            fix32::from_int(384),
            fix32::from_int(128),
        ),
        82,
    );
    let projection: Projection = gpu_scene_projection_value(PROJECTION.as_array());
    let model: Pose = Pose {
        position: Vec3 {
            x: fix32::from_int(10),
            y: fix32::from_int(-4),
            z: fix32::from_int(-12),
        },
        angles: Vec3 {
            x: fix32::from_words(8192, 0),
            y: fix32::from_words(40960, 65535),
            z: fix32::from_words(40960, 0),
        },
        scale: Vec3 {
            x: fix32::from_int(2),
            y: fix32::from_int(-4),
            z: half,
        },
    };
    check(gpu_scene_model_init(OB_MODEL.as_array(), model), 83);
    let eye: Vec3 = Vec3 {
        x: fix32::from_int(3),
        y: fix32::from_int(-2),
        z: fix32::from_int(5),
    };
    let camera: CameraDesc = CameraDesc {
        eye: eye,
        angles: Vec3 {
            x: fix32::from_words(40960, 65535),
            y: fix32::from_words(40960, 0),
            z: fix32::from_words(49152, 65535),
        },
        projection: projection,
    };
    check(gpu_scene_camera_init(OB_EULER.as_array(), camera), 84);
    let target: Vec3 = Vec3 {
        x: -one,
        y: half,
        z: fix32::from_int(-3),
    };
    let up: Vec3 = Vec3 {
        x: half,
        y: one,
        z: fix32::from_words(16384, 0),
    };
    check(
        gpu_scene_camera_init_look_at(OB_LOOK.as_array(), eye, target, up, projection),
        85,
    );
    let mut roundtrip: Mat3x4 = gpu_scene_camera_get_view(OB_LOOK.as_array());
    check(
        gpu_scene_camera_view(OB_LOOK.as_array(), view_of(&mut roundtrip)),
        86,
    );
    oblique_prepare(OB_EULER.as_array(), OB_NATIVE_EULER.as_array());
    oblique_prepare(OB_LOOK.as_array(), OB_NATIVE_LOOK.as_array());
    halt(0x600d);
}
