//! Private CPU prepared/storage schema for the explicit GPU API.
//! All offsets are 16-bit CPU words, not bytes or GPU local qwords.
use crate::dsl_rt::*;

mod gpu_limits;
pub use gpu_limits::*;
pub const GPU_API_SMALL_WORDS: u16 = 16;
pub const GPU_API_LARGE_WORDS: u16 = 48;
pub const GPU_API_CB_WORDS: u16 = 224;
pub const GPU_API_SCRATCH_WORDS: u16 = 256;
pub const GPU_API_STORAGE_CHUNK_WORDS: u16 = 64;
pub const GPU_API_PLANE_SECTORS: u16 = 1600;

// Camera: unreflected source-world affine view, focal, far, native ray, zero.
pub const GPU_CAMERA_VIEW: u16 = 0;
pub const GPU_CAMERA_FOCAL: u16 = 24;
pub const GPU_CAMERA_FAR: u16 = 26;
pub const GPU_CAMERA_RAY: u16 = 28;
pub const GPU_CAMERA_WORDS: u16 = 32;
// Private Camera identity for view/focal dependencies; not part of native data.
pub const GPU_CAMERA_REVISION: u16 = 32;
// Model: source-world affine and inverse-transpose, tightly packed; zero tail.
pub const GPU_MODEL_WORLD: u16 = 0;
pub const GPU_MODEL_NORMAL: u16 = 24;
pub const GPU_MODEL_WORDS: u16 = 42;
// Public Model resource owns its second allocation through this private tail word.
pub const GPU_MODEL_DERIVED: u16 = 42;
// A second large allocation belongs to each Model: coefficients0..41 and state.
pub const GPU_MODEL_DIRTY: u16 = 42;
pub const GPU_MODEL_VIEW_REVISION: u16 = 43;
// Material: packed RGBA, packed RGB, two fix32 responses, code, flags.
pub const GPU_MATERIAL_RGBA: u16 = 0;
pub const GPU_MATERIAL_SPECULAR: u16 = 2;
pub const GPU_MATERIAL_AMBIENT: u16 = 4;
pub const GPU_MATERIAL_DIRECTIONAL: u16 = 6;
pub const GPU_MATERIAL_SHININESS: u16 = 8;
pub const GPU_MATERIAL_FLAGS: u16 = 9;
// State: normalized source-world light, two fix32 intensities, controls, filter.
pub const GPU_STATE_LIGHT: u16 = 0;
pub const GPU_STATE_AMBIENT: u16 = 6;
pub const GPU_STATE_DIRECTIONAL: u16 = 8;
pub const GPU_STATE_CONTROL: u16 = 10;
pub const GPU_STATE_FILTER: u16 = 11;
// External descriptions are values; none contains an owning arena handle.
pub const GPU_MESH_DIRECTORY: u16 = 0;
pub const GPU_MESH_DIRECTORY_WORD: u16 = 1;
pub const GPU_MESH_COUNT: u16 = 2;
pub const GPU_MESH_STORAGE: u16 = 3;
pub const GPU_MESH_STORAGE_COUNT: u16 = 4;
pub const GPU_VIEW_BASE: u16 = 0;
pub const GPU_VIEW_COUNT: u16 = 1;
pub const GPU_VIEW_EXPONENT: u16 = 2;
pub const GPU_VIEW_FLAGS: u16 = 3;
pub const GPU_VIEW_WIDTH: u16 = 4;
pub const GPU_VIEW_HEIGHT: u16 = 5;
// Per-CB recorder: 32 control words, then explicit snapshots and native caches.
pub const GPU_CB_CONTROL: u16 = 0;
pub const GPU_CB_CAMERA_REVISION: u16 = 16;
pub const GPU_CB_CAMERA: u16 = 32;
pub const GPU_CB_STATE: u16 = 64;
pub const GPU_CB_GLOBAL: u16 = 80;
pub const GPU_CB_RENDER: u16 = 112;
pub const GPU_CB_UI: u16 = 128;
pub const GPU_CB_INSTANCE: u16 = 144;
// Optional single immutable prepared draw group, private to its owning CB.
pub const GPU_GROUP_BASE: u16 = 208;
pub const GPU_GROUP_TEMPLATE: u16 = 208;
pub const GPU_GROUP_VALID: u16 = 209;
pub const GPU_GROUP_WORDS: u16 = 210;
pub const GPU_GROUP_PUSH_END: u16 = 211;
pub const GPU_GROUP_FACT_COUNT: u16 = 212;
pub const GPU_GROUP_RECORDING: u16 = 213;
pub const GPU_GROUP_START: u16 = 214;
pub const GPU_GROUP_SAVED_PUSH: u16 = 215;
pub const GPU_GROUP_SAVED_CAPACITY: u16 = 216;
pub const GPU_GROUP_FIRST_GLOBAL: u16 = 217;
pub const GPU_GROUP_FIRST_RENDER: u16 = 218;
pub const GPU_GROUP_LAYOUT: u16 = 219;
pub const GPU_GROUP_REFERENCED: u16 = 220;
pub const GPU_GROUP_COUNT: u16 = 16;
pub const GPU_GROUP_CODE_WORDS: u16 = 512;
pub const GPU_GROUP_META_WORDS: u16 = 384;
pub const GPU_GROUP_PUSH_PREFIX: u16 = 23552;
// Read-only draw input view begins at GPU_CB_CAMERA, spanning camera and state.
pub const GPU_CB_DESIRED_WORDS: u16 = 48;
pub const GPU_DESIRED_CAMERA: u16 = 0;
pub const GPU_DESIRED_STATE: u16 = 32;
// Shared sequential CPU scratch: final native blocks, not persistent bindings.
pub const GPU_SCRATCH_GLOBAL: u16 = 0;
pub const GPU_SCRATCH_RENDER: u16 = 32;
pub const GPU_SCRATCH_TARGET: u16 = 48;
pub const GPU_SCRATCH_INSTANCE: u16 = 64;
pub const GPU_SCRATCH_UI: u16 = 128;
pub const GPU_SCRATCH_MATH: u16 = 144;

// Shared external extent value. A owns its public unsafe registration/factory.
#[derive(Copy, Clone)]
pub struct GpuMemory {
    first_sector: u16,
    sector_count: u16,
}
impl GpuMemory {
    pub fn first_sector(&self) -> u16 {
        self.first_sector
    }
    pub fn sector_count(&self) -> u16 {
        self.sector_count
    }
}
// Atlas/grid prepared fields; B serializes them, A owns the resulting slot.
pub const GPU_UI_ATLAS_BASE: u16 = 0;
pub const GPU_UI_ATLAS_COUNT: u16 = 1;
pub const GPU_UI_ATLAS_GLYPH_COUNT: u16 = 2;
pub const GPU_UI_GRID_BASE: u16 = 0;
pub const GPU_UI_GRID_COUNT: u16 = 1;
pub const GPU_UI_GRID_ATLAS_BASE: u16 = 2;
pub const GPU_UI_GRID_ATLAS_COUNT: u16 = 3;
pub const GPU_UI_GRID_GLYPH_COUNT: u16 = 4;

// A's unsafe registration uses this check; B only validates resource semantics.
fn gpu_memory_extent_valid(first_sector: u16, sector_count: u16) -> bool {
    sector_count != 0 && sector_count - 1 <= 65535 - first_sector
}
