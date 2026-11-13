//! Public configuration values: no driver state or raw device helpers.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;

#[derive(Copy, Clone)]
pub struct CommandMemory {
    pub commands: GpuMemory,
    pub push: GpuMemory,
}
#[derive(Copy, Clone)]
pub struct GpuConfig {
    pub object_arena_bytes: u16,
    pub command_a: CommandMemory,
    pub command_b: Option<CommandMemory>,
}
impl CommandMemory {
    pub const fn new(commands: GpuMemory, push: GpuMemory) -> Self {
        Self { commands, push }
    }
}
impl GpuConfig {
    pub const fn single(command: CommandMemory) -> Self {
        Self {
            object_arena_bytes: GPU_API_ARENA_BYTES,
            command_a: command,
            command_b: None,
        }
    }
    pub const fn double(first: CommandMemory, second: CommandMemory) -> Self {
        Self {
            object_arena_bytes: GPU_API_ARENA_BYTES,
            command_a: first,
            command_b: Some(second),
        }
    }
}
