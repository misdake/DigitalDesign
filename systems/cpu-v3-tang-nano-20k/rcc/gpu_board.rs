//! System factories for the fixed command slots, planes and UI pages.
use crate::dsl_rt::*;
mod gpu_api;use gpu_api::*;
mod gpu_layout;use gpu_layout::*;
pub fn board_gpu_config() -> GpuConfig {
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
pub fn board_color_a() -> ColorBuffer {
    ColorBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(GPU_COLOR_A_SECTOR, GPU_PLANE_SECTORS)
    })
}
pub fn board_color_b() -> ColorBuffer {
    ColorBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(GPU_COLOR_B_SECTOR, GPU_PLANE_SECTORS)
    })
}
pub fn board_depth() -> DepthBuffer {
    DepthBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(GPU_DEPTH_SECTOR, GPU_PLANE_SECTORS)
    })
}
pub fn board_ui_memory(second: bool) -> GpuMemory {
    let mut first = GPU_UI_PAGE0_SECTOR;
    if second { first = GPU_UI_PAGE1_SECTOR; }
    unsafe { GpuMemory::from_sectors(first, GPU_UI_PAGE_SECTORS) }
}
