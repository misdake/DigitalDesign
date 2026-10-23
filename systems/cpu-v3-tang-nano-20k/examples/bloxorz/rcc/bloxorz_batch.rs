//! Imported static level fixture. Manifest handles are assets; CB placement is private.
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
    let params = unsafe { Ptr::from_addr(0x7000).as_u16_array() };
    let mut mesh: GpuMesh = GpuMesh {
        directory_sector: 0,
        directory_word: 0,
        count: 0,
        first_sector: 0x9600,
        sectors: params[7u16],
    };
    let mut texture: GpuTexture = GpuTexture {
        sector: params[6u16],
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
    let mut level: Buf<u16, 2> = Buf::new([0, 0]);
    let mut state: Buf<u16, 10> = Buf::new([0; 10]);
    let half = fix32::from_words(32768, 0);
    level[0u16] = params[0u16];
    level[1u16] = params[1u16];
    state[0u16] = params[2u16];
    state[1u16] = params[3u16];
    state[2u16] = params[4u16];
    check(blox_camera(
        params[0u16],
        params[1u16],
        CAMERA.as_array(),
        VIEW.as_array(),
    ));
    check(gpu_init() == GPU_IDLE);
    let mut frame: u16 = 1;
    while frame <= 1 {
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
        let mut i: u16 = 0;
        while i < params[5u16] {
            let p = 8 + i * 8;
            mesh.directory_sector = params[p];
            mesh.directory_word = params[p + 1];
            mesh.count = params[p + 2];
            if params[p + 4] == 1 {
                check(blox_roll_pose(
                    MODEL.as_array(),
                    level.as_array(),
                    state.as_array(),
                    0,
                    fix32::zero(),
                    fix32::zero(),
                ));
            } else {
                let mut x = fix32::zero();
                let mut z = fix32::zero();
                if params[p + 4] == 2 {
                    x = fix32::from_int(params[p + 5] as i16)
                        - fix32::from_int((params[0u16] - 1) as i16) * half;
                    z = fix32::from_int(params[p + 6] as i16)
                        - fix32::from_int((params[1u16] - 1) as i16) * half;
                }
                gpu_pose_set(
                    MODEL.as_array(),
                    vec3::new(x, -fix32::from_words(6554, 0), z),
                    vec3::zero(),
                    vec3::new(half, half, half),
                );
            }
            check(blox_instance(
                MATRIX.as_array(),
                MODEL.as_array(),
                CAMERA.as_array(),
            ));
            dev_send(7, 2, params[p + 3]);
            dev_send(7, 1, MATRIX.as_ptr().addr());
            check(gpu_pass_draw_mesh(
                pass,
                view_of(&mut mesh),
                MATRIX.as_array(),
                65535,
                65535,
            ));
            i += 1;
        }
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
