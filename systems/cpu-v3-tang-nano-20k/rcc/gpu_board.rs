//! System factories for the fixed command slots, planes and UI pages.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_layout;
use gpu_layout::*;
mod surface;
use surface::*;

pub const BOARD_COLOR_A: Rgb565Surface =
    unsafe { Rgb565Surface::from_sectors(GPU_COLOR_A_SECTOR, GPU_PLANE_SECTORS) };
pub const BOARD_COLOR_B: Rgb565Surface =
    unsafe { Rgb565Surface::from_sectors(GPU_COLOR_B_SECTOR, GPU_PLANE_SECTORS) };

pub fn board_gpu_config() -> GpuConfig {
    let first = CommandMemory::new(
        unsafe { GpuMemory::from_sectors(GPU_COMMAND_FIRST_SECTOR, GPU_COMMAND_CAPACITY_SECTORS) },
        unsafe { GpuMemory::from_sectors(GPU_OBJECT_FIRST_SECTOR, GPU_OBJECT_CAPACITY_SECTORS) },
    );
    let second = CommandMemory::new(
        unsafe {
            GpuMemory::from_sectors(GPU_SLOT1_COMMAND_FIRST_SECTOR, GPU_COMMAND_CAPACITY_SECTORS)
        },
        unsafe {
            GpuMemory::from_sectors(GPU_SLOT1_OBJECT_FIRST_SECTOR, GPU_OBJECT_CAPACITY_SECTORS)
        },
    );
    GpuConfig::double(first, second)
}
pub fn board_color_a() -> ColorBuffer {
    ColorBuffer::from_surface(BOARD_COLOR_A)
}
pub fn board_color_b() -> ColorBuffer {
    ColorBuffer::from_surface(BOARD_COLOR_B)
}
pub fn board_depth() -> DepthBuffer {
    DepthBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(GPU_DEPTH_SECTOR, GPU_PLANE_SECTORS)
    })
}
pub fn board_ui_memory(second: bool) -> GpuMemory {
    let mut first = GPU_UI_PAGE0_SECTOR;
    if second {
        first = GPU_UI_PAGE1_SECTOR;
    }
    unsafe { GpuMemory::from_sectors(first, GPU_UI_PAGE_SECTORS) }
}
