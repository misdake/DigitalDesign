mod gpu_api;
use gpu_api::*;

static G2_PHASE: u16 = 0;
fn mark(value: u16) {
    unsafe {
        addr_of(&G2_PHASE).write(0, value);
    }
}
fn check(ok: bool) {
    if !ok {
        halt(0xbad);
    }
}
fn v(x: i16, y: i16, z: i16) -> Vec3 {
    Vec3 {
        x: fix32::from_int(x),
        y: fix32::from_int(y),
        z: fix32::from_int(z),
    }
}
fn transform() -> Transform {
    Transform {
        position: v(10, -4, -12),
        angles: Vec3 {
            x: fix32::from_words(8192, 0),
            y: fix32::from_words(40960, 65535),
            z: fix32::from_words(40960, 0),
        },
        scale: Vec3 {
            x: fix32::from_int(2),
            y: fix32::from_int(-4),
            z: fix32::from_words(32768, 0),
        },
    }
}
fn model() -> Model {
    Model::new(transform())
}
fn make_euler(projection: Projection) -> Camera {
    let camera = Camera::new(CameraDesc {
        eye: v(3, -2, 5),
        angles: Vec3 {
            x: fix32::from_words(40960, 65535),
            y: fix32::from_words(40960, 0),
            z: fix32::from_words(49152, 65535),
        },
        projection: projection,
    });
    camera
}
fn make_look(projection: Projection) -> Camera {
    let camera = Camera::look_at(
        v(3, -2, 5),
        Vec3 {
            x: fix32::from_int(-1),
            y: fix32::from_words(32768, 0),
            z: fix32::from_int(-3),
        },
        Vec3 {
            x: fix32::from_words(32768, 0),
            y: fix32::from_int(1),
            z: fix32::from_words(16384, 0),
        },
        projection,
    );
    camera
}
fn rtt_view() -> TextureView {
    let color = ColorBuffer::from_memory(unsafe { GpuMemory::from_sectors(0x8800, 2048) });
    let view = color.texture_view();
    drop(color);
    view
}
fn main() {
    let a: CommandMemory = CommandMemory {
        commands: unsafe { GpuMemory::from_sectors(0x9800, 512) },
        push: unsafe { GpuMemory::from_sectors(0x9a00, 512) },
    };
    let b: CommandMemory = CommandMemory {
        commands: unsafe { GpuMemory::from_sectors(0xfc00, 512) },
        push: unsafe { GpuMemory::from_sectors(0xfe00, 512) },
    };
    let mut gpu = Gpu::init(GpuConfig {
        object_arena_bytes: 8192,
        command_a: a,
        command_b: Some(b),
    });
    let mut first = gpu.command_buffer();
    let mut second = gpu.command_buffer();
    let projection = Projection::from_focal_pixels(fix32::from_int(384), fix32::from_int(128));
    let mut euler = make_euler(projection);
    let look = make_look(projection);
    let mut m0 = model();
    let m1 = model();
    let m2 = model();
    let m3 = model();
    let m4 = model();
    let m5 = model();
    let mut material = Material::new(MaterialDesc {
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
        shininess: Shininess::nearest(18),
        unlit: false,
    });
    let mut state = RenderState::new(RenderStateDesc {
        depth: DepthFunc::LessEqual,
        depth_write: true,
        blend: BlendMode::SrcOver,
        filter: TextureFilter::Bilinear,
        lighting: Lighting {
            direction: v(1, 2, -3),
            ambient: fix32::from_words(49152, 0),
            directional: fix32::from_words(32768, 0),
        },
    });
    let color = ColorBuffer::from_memory(unsafe { GpuMemory::from_sectors(0x8000, 2048) });
    let depth = DepthBuffer::from_memory(unsafe { GpuMemory::from_sectors(0x9000, 2048) });
    let mesh = Mesh::from_native(MeshDesc {
        directory_sector: 0xa000,
        directory_word: 0,
        meshlet_count: 1,
        storage: unsafe { GpuMemory::from_sectors(0xa000, 16) },
    });
    let texture = Texture::from_native(TextureDesc {
        format: TextureFormat::Rgb565,
        memory: unsafe { GpuMemory::from_sectors(0xa800, 128) },
        size_log2: 6,
        full_mip: true,
    });
    let view = texture.view();
    drop(texture);
    check(view.width() == 64 && view.height() == 64);
    let rtt = rtt_view();
    check(rtt.width() == 400 && rtt.height() == 240);
    let atlas = UiAtlas::from_native(unsafe { GpuMemory::from_sectors(0xb000, 6) }, 3);
    let mut grid = UiGrid::from_memory(unsafe { GpuMemory::from_sectors(0xb0ff, 6) }, &atlas);
    drop(atlas);
    grid.fill(255);
    grid.write(
        CellRect {
            x: 1,
            y: 1,
            width: 3,
            height: 1,
        },
        b"\x00\x01",
    );
    grid.write(
        CellRect {
            x: 25,
            y: 2,
            width: 5,
            height: 1,
        },
        b"\x00\x01\x02",
    );
    {
        let mut pass = first.pass(
            color.ptr(),
            depth.ptr(),
            ColorLoad::load(),
            DepthLoad::load(),
        );
        pass.set_camera(euler.ptr());
        pass.set_state(state.ptr());
        pass.draw_colored(mesh.ptr(), m0.ptr(), material.ptr());
        pass.draw_colored(mesh.ptr(), m1.ptr(), material.ptr());
    }
    {
        let mut pass = second.pass(
            color.ptr(),
            depth.ptr(),
            ColorLoad::load(),
            DepthLoad::load(),
        );
        pass.set_camera(look.ptr());
        pass.set_state(state.ptr());
        pass.draw_colored(mesh.ptr(), m0.ptr(), material.ptr());
    }
    mark(1);
    m0.set_pose(Transform {
        position: v(11, 0, -3),
        angles: v(0, 0, 0),
        scale: v(1, 1, 1),
    });
    euler.set_pose(v(0, 0, 0), v(0, 0, 0));
    state.set_lighting(Lighting {
        direction: v(0, 0, 1),
        ambient: fix32::from_int(1),
        directional: fix32::from_int(1),
    });
    material.set_uniform_color(Rgba8 {
        r: 1,
        g: 2,
        b: 3,
        a: 4,
    });
    drop(m0);
    let replacement = Model::new(Transform {
        position: v(11, 0, -3),
        angles: v(0, 0, 0),
        scale: v(1, 1, 1),
    });
    let replacement_world = replacement.world();
    check(replacement_world.row0.w == fix32::from_int(11));
    mark(2);
    {
        let mut pass = first.pass(
            color.ptr(),
            depth.ptr(),
            ColorLoad::load(),
            DepthLoad::load(),
        );
        pass.set_camera(euler.ptr());
        pass.set_state(state.ptr());
        pass.draw(mesh.ptr(), replacement.ptr(), material.ptr(), view.ptr());
        pass.draw(mesh.ptr(), replacement.ptr(), material.ptr(), rtt.ptr());
        pass.draw_ui(
            grid.ptr(),
            CellRect {
                x: 1,
                y: 1,
                width: 3,
                height: 1,
            },
        );
        pass.draw_colored(mesh.ptr(), replacement.ptr(), material.ptr());
        material.set_uniform_color(Rgba8 {
            r: 1,
            g: 2,
            b: 3,
            a: 8,
        });
        pass.draw_colored(mesh.ptr(), replacement.ptr(), material.ptr());
    }
    mark(3);
    drop(replacement);
    drop(m1);
    drop(m2);
    drop(m3);
    drop(m4);
    drop(m5);
    drop(material);
    drop(state);
    drop(euler);
    drop(look);
    drop(view);
    drop(rtt);
    drop(grid);
    drop(mesh);
    drop(color);
    drop(depth);
    mark(4);
    drop(first);
    drop(second);
    drop(gpu);
    halt(0x600d);
}
