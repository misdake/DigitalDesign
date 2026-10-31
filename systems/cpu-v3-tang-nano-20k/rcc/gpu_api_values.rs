//! Public configuration values: no driver state or raw device helpers.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;

#[derive(Copy, Clone)]
pub struct CommandMemory {
    pub commands: GpuMemory,
    pub push: GpuMemory,
}
pub struct GpuConfig {
    pub object_arena_bytes: u16,
    pub command_a: CommandMemory,
    pub command_b: Option<CommandMemory>,
}
