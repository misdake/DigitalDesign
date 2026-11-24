//! Ordinary configuration values and explicit caller-owned initialization.
use crate::dsl_rt::*;
mod gpu_api_values;
use gpu_api_values::*;
mod gpu_arena;
use gpu_arena::*;
mod gpu_execute;
use gpu_execute::*;
mod gpu_record;
use gpu_record::*;

fn gpu_config_command_valid(memory: CommandMemory) -> bool {
    let command: GpuMemory = memory.commands;
    let push: GpuMemory = memory.push;
    gpu_record_extent(command.first_sector(), command.sector_count())
        && gpu_record_extent(push.first_sector(), push.sector_count())
        && !gpu_record_overlap(
            command.first_sector(),
            command.sector_count(),
            push.first_sector(),
            push.sector_count(),
        )
}
fn gpu_config_disjoint(a: CommandMemory, b: CommandMemory) -> bool {
    let ac: GpuMemory = a.commands;
    let ap: GpuMemory = a.push;
    let bc: GpuMemory = b.commands;
    let bp: GpuMemory = b.push;
    !gpu_record_overlap(
        ac.first_sector(),
        ac.sector_count(),
        bc.first_sector(),
        bc.sector_count(),
    ) && !gpu_record_overlap(
        ac.first_sector(),
        ac.sector_count(),
        bp.first_sector(),
        bp.sector_count(),
    ) && !gpu_record_overlap(
        ap.first_sector(),
        ap.sector_count(),
        bc.first_sector(),
        bc.sector_count(),
    ) && !gpu_record_overlap(
        ap.first_sector(),
        ap.sector_count(),
        bp.first_sector(),
        bp.sector_count(),
    )
}
fn gpu_config_record(slot: u16, memory: CommandMemory) -> bool {
    let command: GpuMemory = memory.commands;
    let push: GpuMemory = memory.push;
    gpu_record_configure(
        slot,
        command.first_sector(),
        command.sector_count(),
        push.first_sector(),
        push.sector_count(),
    )
}
fn gpu_config_init(config: GpuConfig) -> bool {
    if config.object_arena_bytes == 0
        || config.object_arena_bytes > GPU_API_ARENA_BYTES
        || config.object_arena_bytes & 31 != 0
        || GPU_EXECUTION[GPU_EXEC_CONTROLLER] != 0
        || GPU_EXECUTION[1u16] != 0
        || !gpu_arena_empty()
        || !gpu_config_command_valid(config.command_a)
    {
        return false;
    }
    // CBs are not arena objects. A handed-out, undropped record still owns its
    // slot even after the public Gpu token has been dropped.
    let mut slot: u16 = 0;
    while slot < GPU_EXECUTION[GPU_EXEC_CONFIGURED] {
        if GPU_EXECUTION[GPU_EXEC_TAKEN] & (1u16 << slot) != 0
            && gpu_record_data(slot)[GPU_RECORD_PHASE] != 0
        {
            return false;
        }
        slot += 1;
    }
    let second: Option<CommandMemory> = config.command_b;
    if let Some(b) = second {
        if !gpu_config_command_valid(b) || !gpu_config_disjoint(config.command_a, b) {
            return false;
        }
    }
    if !gpu_execute_can_init() || !gpu_arena_init(config.object_arena_bytes) {
        return false;
    }
    gpu_execute_init();
    gpu_record_init();
    if !gpu_config_record(0, config.command_a) {
        return false;
    }
    GPU_EXECUTION[GPU_EXEC_CONFIGURED] = 1;
    let second: Option<CommandMemory> = config.command_b;
    if let Some(b) = second {
        if !gpu_config_record(1, b) {
            return false;
        }
        GPU_EXECUTION[GPU_EXEC_CONFIGURED] = 2;
    }
    GPU_EXECUTION[GPU_EXEC_CONTROLLER] = 1;
    true
}
// 65535 is exhaustion/error; real slot zero is an ordinary successful identity.
fn gpu_config_take_buffer() -> u16 {
    if GPU_EXECUTION[0u16] == 0 || GPU_EXECUTION[0u16] == GPU_EXEC_FAULT {
        return 65535;
    }
    let mut slot: u16 = 0;
    while slot < GPU_EXECUTION[GPU_EXEC_CONFIGURED] {
        let mask: u16 = 1 << slot;
        if GPU_EXECUTION[GPU_EXEC_TAKEN] & mask == 0 {
            if !gpu_record_new_generation(gpu_record_data(slot)) {
                return 65535;
            }
            GPU_EXECUTION[GPU_EXEC_TAKEN] |= mask;
            return slot;
        }
        slot += 1;
    }
    65535
}
fn gpu_surface_init(mut out: Array<u16>, memory: GpuMemory) -> bool {
    if memory.first_sector() & 511 != 0 || memory.sector_count() < GPU_API_PLANE_SECTORS {
        return false;
    }
    let mut i: u16 = 0;
    while i < 16 {
        out[i] = 0;
        i += 1;
    }
    out[0u16] = memory.first_sector();
    out[1u16] = memory.sector_count();
    true
}
fn gpu_config_drop_buffer(slot: u16) -> bool {
    if slot >= 2 || GPU_EXECUTION[1u16] == slot + 1 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_READY && c[GPU_RECORD_PHASE] != GPU_RECORD_SEALED {
        return false;
    }
    c[GPU_RECORD_PHASE] = 0;
    // The configured CB is not a pool allocation: Drop does not reissue it.
    true
}
