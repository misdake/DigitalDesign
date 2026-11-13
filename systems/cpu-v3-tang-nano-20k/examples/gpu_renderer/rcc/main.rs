//! Application-owned recording, input, timing, UI and explicit GPU/Display retirement.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_layout;
use gpu_layout::*;
mod boot_resources;
use boot_resources::*;
mod device_abi;
use device_abi::*;
mod gamepad;
use gamepad::*;
mod time;
use time::*;
mod profiler_clock;
use profiler_clock::*;
mod frame_stats;
use frame_stats::*;

fn point(x: fix32, y: fix32, z: fix32) -> Vec3 {
    Vec3 { x: x, y: y, z: z }
}
fn zero() -> Vec3 {
    point(fix32::zero(), fix32::zero(), fix32::zero())
}
fn camera() -> Camera {
    Camera::new(CameraDesc {
        eye: zero(),
        angles: zero(),
        projection: Projection::from_focal_pixels(fix32::from_int(200), fix32::from_int(199)),
    })
}
fn model() -> Model {
    let one = fix32::from_int(1);
    Model::new(Transform {
        position: point(fix32::zero(), fix32::zero(), fix32::from_int(-3)),
        angles: zero(),
        scale: point(one, one, one),
    })
}
fn material() -> Material {
    Material::new(MaterialDesc {
        uniform_color: Rgba8 {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        specular: Rgb8 { r: 0, g: 0, b: 0 },
        ambient_response: fix32::from_words(32768, 0),
        directional_response: fix32::from_words(32768, 0),
        shininess: Shininess::S16,
        unlit: false,
    })
}
fn state() -> RenderState {
    RenderState::new(RenderStateDesc {
        depth: DepthFunc::LessEqual,
        depth_write: true,
        blend: BlendMode::Replace,
        filter: TextureFilter::Nearest,
        lighting: Lighting {
            direction: point(fix32::zero(), fix32::zero(), fix32::from_int(1)),
            ambient: fix32::from_int(1),
            directional: fix32::from_int(1),
        },
    })
}
fn mesh() -> Mesh {
    Mesh::from_native(MeshDesc {
        directory_sector: GPU_RENDERER_MESH_DIRECTORY,
        directory_word: GPU_RENDERER_MESH_DIRECTORY_WORD,
        meshlet_count: GPU_RENDERER_MESH_COUNT,
        storage: unsafe {
            GpuMemory::from_sectors(GPU_RENDERER_MESH_FIRST, GPU_RENDERER_MESH_SECTORS)
        },
    })
}
fn texture() -> Texture {
    Texture::from_native(TextureDesc {
        format: TextureFormat::Rgb565,
        memory: unsafe {
            GpuMemory::from_sectors(GPU_RENDERER_TEXTURE, GPU_RENDERER_TEXTURE_SECTORS)
        },
        size_log2: 0,
        full_mip: false,
    })
}
fn grid(sector: u16) -> UiGrid {
    let base =
        (GPU_RENDERER_ATLAS_BYTE_ADDRESS_HIGH << 9) | (GPU_RENDERER_ATLAS_BYTE_ADDRESS_LOW >> 7);
    let atlas = UiAtlas::from_native(unsafe { GpuMemory::from_sectors(base, 512) }, 255);
    let mut grid = UiGrid::from_memory(
        unsafe { GpuMemory::from_sectors(sector, GPU_UI_PAGE_SECTORS) },
        &atlas,
    );
    grid.fill(255);
    grid
}
fn clamp_eye(v: fix32) -> fix32 {
    if v > fix32::from_int(20) {
        return fix32::from_int(20);
    }
    if v < fix32::from_int(-20) {
        return fix32::from_int(-20);
    }
    v
}
// RCC byte slices currently describe immutable packed constants. Formatting is
// application policy; single-cell writes avoid a second persistent grid mirror.
fn glyph(grid: &mut UiGrid, column: u16, value: u16) {
    let cell: CellRect = CellRect {
        x: column,
        y: 0,
        width: 1,
        height: 1,
    };
    if value == 45 {
        grid.write(cell, b"-");
    } else if value == 46 {
        grid.write(cell, b".");
    } else if value == 48 {
        grid.write(cell, b"0");
    } else if value == 49 {
        grid.write(cell, b"1");
    } else if value == 50 {
        grid.write(cell, b"2");
    } else if value == 51 {
        grid.write(cell, b"3");
    } else if value == 52 {
        grid.write(cell, b"4");
    } else if value == 53 {
        grid.write(cell, b"5");
    } else if value == 54 {
        grid.write(cell, b"6");
    } else if value == 55 {
        grid.write(cell, b"7");
    } else if value == 56 {
        grid.write(cell, b"8");
    } else if value == 57 {
        grid.write(cell, b"9");
    } else {
        grid.write(cell, b"?");
    }
}
fn text(grid: &mut UiGrid, stats: Array<u16>) {
    let digits: Buf<u16, 4> = Buf::new([0; 4]);
    frame_stats_tenths_digits(stats, digits.as_array());
    grid.write(
        CellRect {
            x: 0,
            y: 0,
            width: 8,
            height: 1,
        },
        b"OBS ",
    );
    let mut first: u16 = 0;
    if stats[3u16] != 0 && digits[0u16] == 48 {
        first = 1;
    }
    let mut at = first;
    while at < 4 {
        glyph(grid, 4 + at - first, digits[at]);
        at += 1;
    }
}
fn run(limit: u16) {
    let command: CommandMemory = CommandMemory {
        commands: unsafe {
            GpuMemory::from_sectors(GPU_COMMAND_FIRST_SECTOR, GPU_COMMAND_CAPACITY_SECTORS)
        },
        push: unsafe {
            GpuMemory::from_sectors(GPU_OBJECT_FIRST_SECTOR, GPU_OBJECT_CAPACITY_SECTORS)
        },
    };
    let absent: Option<CommandMemory> = None;
    let mut gpu = Gpu::init(GpuConfig {
        object_arena_bytes: 8192,
        command_a: command,
        command_b: absent,
    });
    let mut buffer = gpu.command_buffer();
    let color_a = ColorBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(GPU_COLOR_A_SECTOR, GPU_PLANE_SECTORS)
    });
    let color_b = ColorBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(GPU_COLOR_B_SECTOR, GPU_PLANE_SECTORS)
    });
    let depth = DepthBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(GPU_DEPTH_SECTOR, GPU_PLANE_SECTORS)
    });
    let mut display = Display::open();
    let mut camera = camera();
    let model = model();
    let material = material();
    let state = state();
    let mesh = mesh();
    let texture = texture();
    let view = texture.view();
    drop(texture);
    let mut grid_b = grid(GPU_UI_PAGE0_SECTOR);
    let mut grid_a = grid(GPU_UI_PAGE1_SECTOR);
    gamepad_init();
    let stats: Buf<u16, 4> = Buf::new([0; 4]);
    frame_stats_reset(stats.as_array());
    let mut eye_x = fix32::zero();
    let mut eye_z = fix32::zero();
    let mut previous_ms = profiler_active_milliseconds();
    let mut observed: u16 = 65535;
    let mut completed = false;
    let mut count: u16 = 0;
    let mut back_b = true;
    loop {
        // Boot scanout starts on A; the first Back B needs no vblank wait.
        // Later iterations confirm the previous swap and old Front retirement.
        if completed {
            display.wait_vblank();
        }
        let now = profiler_active_milliseconds();
        // Millisecond conversion uses bounded integers before Q16.16 scaling.
        let mut milliseconds = now - previous_ms;
        if milliseconds > 100 {
            milliseconds = 100;
        }
        let elapsed = fix32::from_int(milliseconds as i16) * fix32::from_words(66, 0);
        previous_ms = now;
        gamepad_capture();
        let input = gamepad_take_frame();
        eye_x = clamp_eye(eye_x + input.left_x * elapsed);
        eye_z = clamp_eye(eye_z + input.left_y * elapsed);
        camera.set_pose(point(eye_x, fix32::zero(), eye_z), zero());
        frame_stats_elapsed_begin(stats.as_array(), observed, completed);
        buffer.reset();
        if count == 0 {
            buffer.invalidate_textures();
        }
        if back_b {
            text(&mut grid_b, stats.as_array());
            let mut pass = buffer.pass(
                color_b.ptr(),
                depth.ptr(),
                ColorLoad::clear(Rgb565::from_raw(0x1111 + count)),
                DepthLoad::clear(65535),
            );
            pass.set_camera(camera.ptr());
            pass.set_state(state.ptr());
            pass.draw(mesh.ptr(), model.ptr(), material.ptr(), view.ptr());
            pass.draw_ui(
                grid_b.ptr(),
                CellRect {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 1,
                },
            );
        } else {
            text(&mut grid_a, stats.as_array());
            let mut pass = buffer.pass(
                color_a.ptr(),
                depth.ptr(),
                ColorLoad::clear(Rgb565::from_raw(0x1111 + count)),
                DepthLoad::clear(65535),
            );
            pass.set_camera(camera.ptr());
            pass.set_state(state.ptr());
            pass.draw(mesh.ptr(), model.ptr(), material.ptr(), view.ptr());
            pass.draw_ui(
                grid_a.ptr(),
                CellRect {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 1,
                },
            );
        }
        let resume_before = profiler_resume_epoch();
        time_clear_microseconds();
        gpu.submit(buffer);
        buffer = gpu.wait();
        observed = time_microseconds();
        if profiler_resume_epoch() != resume_before {
            observed = 65535;
        }
        completed = true;
        if back_b {
            display.set_next_swap(color_b.ptr());
        } else {
            display.set_next_swap(color_a.ptr());
        }
        back_b = !back_b;
        count += 1;
        if limit != 0 && count == limit {
            break;
        }
    }
    display.wait_vblank();
    drop(buffer);
    drop(grid_a);
    drop(grid_b);
    drop(view);
    drop(mesh);
    drop(state);
    drop(material);
    drop(model);
    drop(camera);
    drop(display);
    drop(depth);
    drop(color_b);
    drop(color_a);
    drop(gpu);
}
pub fn renderer_three_frames() {
    run(3);
    halt(0x600d);
}
fn main() {
    run(0);
}
