//! Minimal static RGB scene. Driver alone owns CB storage and submission.
mod bloxorz_scene;
mod gpu;
mod gpu_transform;
static MATRIX: Buf<u16, 42> = Buf::new([0; 42]);
static CAMERA: Buf<u16, 18> = Buf::new([0; 18]);
static MODEL: Buf<u16, 18> = Buf::new([0; 18]);
static VIEW: Buf<u16, 24> = Buf::new([0; 24]);
fn check(ok: bool) {
    if !ok {
        halt(0xbad);
    }
}
fn main() {
    let mut floor: GpuMesh = GpuMesh {
        directory_sector: 0x9600,
        directory_word: 0,
        count: 1,
        first_sector: 0x9600,
        sectors: 32,
    };
    let mut block: GpuMesh = GpuMesh {
        directory_sector: 0x9600,
        directory_word: 2,
        count: 1,
        first_sector: 0x9600,
        sectors: 32,
    };
    let mut texture: GpuTexture = GpuTexture {
        sector: 0x9620,
        sectors: 1,
        size_log2: 0,
        full_mip: 0,
    };
    let mut global: GpuGlobal = GpuGlobal {
        far_low: 0,
        far_high: 100,
        light_x: 0,
        light_y: 0,
        light_z: 16384,
        ray_x: 0,
        ray_y: 0,
        ray_k: 8192,
        ambient: 256,
        directional: 0,
        mode: 0,
        shininess: 0,
        epoch: 0,
    };
    let mut render: GpuRender = GpuRender {
        texture_sector: 0,
        texture_sectors: 0,
        size_log2: 0,
        full_mip: 0,
        filter: 0,
        specular_low: 0,
        specular_high: 0,
        depth_func: 1,
        depth_write: 1,
        blend: 0,
        unlit: 1,
        lod_bias: 0,
    };
    let mut level: Buf<u16, 2> = Buf::new([3, 3]);
    let mut state: Buf<u16, 10> = Buf::new([1, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    let half = fix32::from_words(32768, 0);
    check(blox_camera(3, 3, CAMERA.as_array(), VIEW.as_array()));
    check(gpu_init() == GPU_IDLE);
    let mut frame: u16 = 1;
    while frame <= 2 {
        let cb = gpu_cb_begin();
        check(cb != GPU_CB_NONE);
        let pass = gpu_pass_begin(cb, 0, 65535);
        check(pass != GPU_CB_NONE);
        check(gpu_pass_set_global(pass, view_of(&mut global)));
        check(gpu_pass_set_render(
            pass,
            view_of(&mut texture),
            view_of(&mut render),
        ));
        let mut z: u16 = 0;
        while z < 3 {
            let mut x: u16 = 0;
            while x < 3 {
                if x != 0 || z != 0 {
                    gpu_pose_set(
                        MODEL.as_array(),
                        vec3::new(
                            fix32::from_int((x as i16) - 1),
                            -fix32::from_words(6554, 0),
                            fix32::from_int((z as i16) - 1),
                        ),
                        vec3::zero(),
                        vec3::new(half, half, half),
                    );
                    check(blox_instance(
                        MATRIX.as_array(),
                        MODEL.as_array(),
                        CAMERA.as_array(),
                    ));
                    dev_send(7, 2, 0);
                    dev_send(7, 1, MATRIX.as_ptr().addr());
                    check(gpu_pass_draw_mesh(
                        pass,
                        view_of(&mut floor),
                        MATRIX.as_array(),
                        65535,
                        65535,
                    ));
                }
                x += 1;
            }
            z += 1;
        }
        check(blox_roll_pose(
            MODEL.as_array(),
            level.as_array(),
            state.as_array(),
            0,
            fix32::zero(),
            fix32::zero(),
        ));
        check(blox_instance(
            MATRIX.as_array(),
            MODEL.as_array(),
            CAMERA.as_array(),
        ));
        dev_send(7, 2, 1);
        dev_send(7, 1, MATRIX.as_ptr().addr());
        check(gpu_pass_draw_mesh(
            pass,
            view_of(&mut block),
            MATRIX.as_array(),
            65535,
            65535,
        ));
        check(gpu_pass_end(pass));
        check(gpu_cb_finish(cb));
        check(gpu_submit_idle(cb) == GPU_PENDING);
        check(gpu_wait(cb, 65535) == GPU_COMPLETED);
        // This render-only fixture releases Ready without displaying the frame.
        check(gpu_discard_ready(cb));
        dev_send(7, 0, frame);
        frame += 1;
    }
    halt(0x600d);
}
