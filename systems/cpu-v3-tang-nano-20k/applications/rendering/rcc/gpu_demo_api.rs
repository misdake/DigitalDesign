//! Ordinary factories for fixed boot assets; applications own scheduling and retirement.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_board;
use gpu_board::*;
mod boot_resources;
use boot_resources::*;

// Legacy names stay direct to avoid adding another aggregate-return ABI layer.
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
pub fn demo_mesh() -> Mesh {
    // Legacy boot-resource offsets stay at this compatibility boundary.
    let storage =
        unsafe { GpuMemory::from_sectors(GPU_RENDERER_MESH_FIRST, GPU_RENDERER_MESH_SECTORS) };
    let directory = (GPU_RENDERER_MESH_DIRECTORY - GPU_RENDERER_MESH_FIRST) * 64
        + GPU_RENDERER_MESH_DIRECTORY_WORD;
    Mesh::from_native(MeshDesc::from_memory(
        storage,
        directory,
        GPU_RENDERER_MESH_COUNT,
    ))
}
pub fn demo_mesh_part(first: u16, count: u16) -> Mesh {
    let whole = demo_mesh();
    whole.subset(first, count)
}
pub fn demo_texture() -> Texture {
    let memory =
        unsafe { GpuMemory::from_sectors(GPU_RENDERER_TEXTURE, GPU_RENDERER_TEXTURE_SECTORS) };
    let mut mipmaps = Mipmaps::BaseOnly;
    if GPU_RENDERER_TEXTURE_MIP != 0 {
        mipmaps = Mipmaps::Full;
    }
    Texture::from_native(TextureDesc::from_memory(
        memory,
        TextureLayout::square(
            1 << GPU_RENDERER_TEXTURE_LOG2,
            TextureFormat::Rgb565,
            mipmaps,
        ),
    ))
}
pub fn demo_camera() -> Camera {
    Camera::new(CameraDesc::new(
        Vec3::zero(),
        Vec3::zero(),
        Projection::from_focal_pixels(fix32::from_int(200), fix32::from_int(199)),
    ))
}
pub fn demo_material(color: Rgba8, unlit: bool) -> Material {
    let half = fix!(0.5);
    Material::new(
        MaterialDesc::lit(color, Rgb8::black(), Shininess::S16)
            .with_response(half, half)
            .with_unlit(unlit),
    )
}
pub fn demo_white() -> Rgba8 {
    Rgba8::white()
}
pub fn demo_lighting(direction: Vec3) -> Lighting {
    Lighting::directional(direction, fix32::one(), fix32::one())
}
pub fn demo_state() -> RenderState {
    RenderState::new(
        RenderStateDesc::opaque(demo_lighting(Vec3::unit_z()))
            .with_depth(DepthFunc::LessEqual, true)
            .with_filter(TextureFilter::Nearest),
    )
}
pub fn demo_grid(second: bool) -> UiGrid {
    let atlas_sector =
        (GPU_RENDERER_ATLAS_BYTE_ADDRESS_HIGH << 9) | (GPU_RENDERER_ATLAS_BYTE_ADDRESS_LOW >> 7);
    let atlas = UiAtlas::from_native(unsafe { GpuMemory::from_sectors(atlas_sector, 512) }, 255);
    let mut grid = UiGrid::from_memory(board_ui_memory(second), &atlas);
    grid.clear();
    grid
}
pub fn demo_glyph(grid: &mut UiGrid, x: u16, y: u16, digit: u16) {
    let cell = CellRect::new(x, y, 1, 1);
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
