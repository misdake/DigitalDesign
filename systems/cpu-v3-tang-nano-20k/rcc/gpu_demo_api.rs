//! Ordinary factories for fixed boot assets; applications own scheduling and retirement.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_layout;
use gpu_layout::*;
mod boot_resources;
use boot_resources::*;

pub fn demo_point(x: fix32, y: fix32, z: fix32) -> Vec3 {
    Vec3 { x: x, y: y, z: z }
}
pub fn demo_zero() -> Vec3 {
    demo_point(fix32::zero(), fix32::zero(), fix32::zero())
}
pub fn demo_scale() -> Vec3 {
    let one = fix32::from_int(1);
    demo_point(one, one, one)
}
pub fn demo_pose(position: Vec3, angles: Vec3, scale: Vec3) -> Transform {
    Transform {
        position: position,
        angles: angles,
        scale: scale,
    }
}
pub fn demo_config() -> GpuConfig {
    let second: CommandMemory = CommandMemory {
        commands: unsafe {
            GpuMemory::from_sectors(GPU_SLOT1_COMMAND_FIRST_SECTOR, GPU_COMMAND_CAPACITY_SECTORS)
        },
        push: unsafe {
            GpuMemory::from_sectors(GPU_SLOT1_OBJECT_FIRST_SECTOR, GPU_OBJECT_CAPACITY_SECTORS)
        },
    };
    GpuConfig {
        object_arena_bytes: 8192,
        command_a: CommandMemory {
            commands: unsafe {
                GpuMemory::from_sectors(GPU_COMMAND_FIRST_SECTOR, GPU_COMMAND_CAPACITY_SECTORS)
            },
            push: unsafe {
                GpuMemory::from_sectors(GPU_OBJECT_FIRST_SECTOR, GPU_OBJECT_CAPACITY_SECTORS)
            },
        },
        command_b: Some(second),
    }
}
pub fn demo_color_a() -> ColorBuffer {
    ColorBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(GPU_COLOR_A_SECTOR, GPU_PLANE_SECTORS)
    })
}
pub fn demo_color_b() -> ColorBuffer {
    ColorBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(GPU_COLOR_B_SECTOR, GPU_PLANE_SECTORS)
    })
}
pub fn demo_depth() -> DepthBuffer {
    DepthBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(GPU_DEPTH_SECTOR, GPU_PLANE_SECTORS)
    })
}
pub fn demo_mesh() -> Mesh {
    Mesh::from_native(MeshDesc {
        directory_sector: GPU_RENDERER_MESH_DIRECTORY,
        directory_word: GPU_RENDERER_MESH_DIRECTORY_WORD,
        meshlet_count: GPU_RENDERER_MESH_COUNT,
        storage: unsafe {
            GpuMemory::from_sectors(GPU_RENDERER_MESH_FIRST, GPU_RENDERER_MESH_SECTORS)
        },
    })
}
pub fn demo_mesh_part(first: u16, count: u16) -> Mesh {
    let whole = demo_mesh();
    whole.subset(first, count)
}
pub fn demo_texture() -> Texture {
    Texture::from_native(TextureDesc {
        memory: unsafe {
            GpuMemory::from_sectors(GPU_RENDERER_TEXTURE, GPU_RENDERER_TEXTURE_SECTORS)
        },
        size_log2: GPU_RENDERER_TEXTURE_LOG2,
        full_mip: GPU_RENDERER_TEXTURE_MIP != 0,
    })
}
pub fn demo_camera() -> Camera {
    Camera::new(CameraDesc {
        eye: demo_zero(),
        angles: demo_zero(),
        projection: Projection::from_focal_pixels(fix32::from_int(200), fix32::from_int(199)),
    })
}
pub fn demo_material(color: Rgba8, unlit: bool) -> Material {
    Material::new(MaterialDesc {
        uniform_color: color,
        specular: Rgb8 { r: 0, g: 0, b: 0 },
        ambient_response: fix32::from_words(32768, 0),
        directional_response: fix32::from_words(32768, 0),
        shininess: Shininess::S16,
        unlit: unlit,
    })
}
pub fn demo_white() -> Rgba8 {
    Rgba8 {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    }
}
pub fn demo_lighting(direction: Vec3) -> Lighting {
    Lighting {
        direction: direction,
        ambient: fix32::from_int(1),
        directional: fix32::from_int(1),
    }
}
pub fn demo_state() -> RenderState {
    RenderState::new(RenderStateDesc {
        depth: DepthFunc::LessEqual,
        depth_write: true,
        blend: BlendMode::Replace,
        filter: TextureFilter::Nearest,
        lighting: demo_lighting(demo_point(fix32::zero(), fix32::zero(), fix32::from_int(1))),
    })
}
pub fn demo_grid(second: bool) -> UiGrid {
    let atlas_sector =
        (GPU_RENDERER_ATLAS_BYTE_ADDRESS_HIGH << 9) | (GPU_RENDERER_ATLAS_BYTE_ADDRESS_LOW >> 7);
    let atlas = UiAtlas::from_native(unsafe { GpuMemory::from_sectors(atlas_sector, 512) }, 255);
    let mut first = GPU_UI_PAGE0_SECTOR;
    if second {
        first = GPU_UI_PAGE1_SECTOR;
    }
    let mut grid = UiGrid::from_memory(
        unsafe { GpuMemory::from_sectors(first, GPU_UI_PAGE_SECTORS) },
        &atlas,
    );
    grid.fill(255);
    grid
}
pub fn demo_glyph(grid: &mut UiGrid, x: u16, y: u16, digit: u16) {
    let cell: CellRect = CellRect {
        x: x,
        y: y,
        width: 1,
        height: 1,
    };
    // RCC byte slices are packed constants; no persistent UI mirror is needed.
    if digit == 0 {
        grid.write(cell, b"0");
        return;
    }
    if digit == 1 {
        grid.write(cell, b"1");
        return;
    }
    if digit == 2 {
        grid.write(cell, b"2");
        return;
    }
    if digit == 3 {
        grid.write(cell, b"3");
        return;
    }
    if digit == 4 {
        grid.write(cell, b"4");
        return;
    }
    if digit == 5 {
        grid.write(cell, b"5");
        return;
    }
    if digit == 6 {
        grid.write(cell, b"6");
        return;
    }
    if digit == 7 {
        grid.write(cell, b"7");
        return;
    }
    if digit == 8 {
        grid.write(cell, b"8");
        return;
    }
    if digit == 9 {
        grid.write(cell, b"9");
        return;
    }
    grid.write(cell, b"\xff");
}
pub fn demo_number(grid: &mut UiGrid, x: u16, y: u16, width: u16, value: u16) {
    let mut divisor: u16 = 1;
    while value / divisor >= 10 {
        divisor *= 10;
    }
    let mut column: u16 = 0;
    loop {
        if column >= width {
            break;
        }
        demo_glyph(grid, x + column, y, (value / divisor) % 10);
        column += 1;
        if divisor == 1 {
            break;
        }
        divisor /= 10;
    }
    while column < width {
        demo_glyph(grid, x + column, y, 255);
        column += 1;
    }
}
