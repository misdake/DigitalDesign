//! Private CPU prepared/storage schema for the explicit GPU API.
//! All offsets are 16-bit CPU words, not bytes or GPU local qwords.
use crate::dsl_rt::*;

mod gpu_limits;
pub use gpu_limits::*;
pub const GPU_API_SMALL_WORDS: u16 = 16;
pub const GPU_API_LARGE_WORDS: u16 = 48;
pub const GPU_API_CB_WORDS: u16 = 160;
pub const GPU_API_SCRATCH_WORDS: u16 = 256;
pub const GPU_API_STORAGE_CHUNK_WORDS: u16 = 64;
pub const GPU_API_PUSH_CHUNK_WORDS: u16 = 128;
pub const GPU_API_PLANE_SECTORS: u16 = 1600;

// Camera: unreflected source-world affine view, focal, far, native ray, zero.
pub const GPU_CAMERA_VIEW: u16 = 0;
pub const GPU_CAMERA_FOCAL: u16 = 24;
pub const GPU_CAMERA_FAR: u16 = 26;
pub const GPU_CAMERA_RAY: u16 = 28;
pub const GPU_CAMERA_WORDS: u16 = 32;
// Private Camera identity for view/focal dependencies; not part of native data.
pub const GPU_CAMERA_REVISION: u16 = 32;
// Model: source-world affine and inverse-transpose; private revision at44..47.
pub const GPU_MODEL_WORLD: u16 = 0;
pub const GPU_MODEL_NORMAL: u16 = 24;
pub const GPU_MODEL_WORDS: u16 = 42;
pub const GPU_MODEL_REVISION: u16 = 44;
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
pub const GPU_MESH_DIRECTORY_CACHE: u16 = 5;
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
pub const GPU_CB_RESPONSE: u16 = 124;
// Binding metadata occupies the spare span between copy facts and Material24.
pub const GPU_CB_VIEW_WORD: u16 = 106;
pub const GPU_CB_VIEW_BLOCKS: u16 = 107;
pub const GPU_CB_MODEL_REVISION: u16 = GPU_CB_GLOBAL + 20;
pub const GPU_BIND_VIEW_DIRTY: u16 = 128;
pub const GPU_BIND_LIGHT_VALID: u16 = 64;
pub const GPU_BIND_SCENE_DIRTY: u16 = 256;
pub const GPU_BIND_RESPONSE_VALID: u16 = 512;
// CPU scratch metadata, never uploaded as part of a scene or matrix.
pub const GPU_SCRATCH_BILLBOARD: u16 = 30;
pub const GPU_SCRATCH_WORLD_MODE: u16 = 31;
// A single transient recorder shares the ordinary encoder, never submission.
pub const GPU_PREPARED_RECORD: u16 = 2;
pub const GPU_RECORD_COUNT: u16 = 3;
pub const GPU_GROUP_RECORDING: u16 = 149;
#[cfg(not(gpu_debug))]
pub const GPU_GROUP_META_HEADER_WORDS: u16 = 4;
#[cfg(gpu_debug)]
pub const GPU_GROUP_META_HEADER_WORDS: u16 = 8;
pub const GPU_GROUP_META_EXIT: u16 = GPU_GROUP_META_HEADER_WORDS;
pub const GPU_GROUP_META_FACTS: u16 = GPU_GROUP_META_EXIT + 12;
#[cfg(gpu_debug)]
pub const GPU_GROUP_RESPONSE_MAX: u16 = GPU_CB_GLOBAL + 16;
// Prepared allocation payload. State is empty=0, recording=1 or ready=2.
// External input lifetimes are caller-owned; CBs retain no locks or list links.
pub const GPU_PREPARED_COMMAND: u16 = 0;
pub const GPU_PREPARED_COMMAND_CAPACITY: u16 = 1;
pub const GPU_PREPARED_PUSH: u16 = 2;
pub const GPU_PREPARED_PUSH_CAPACITY: u16 = 3;
pub const GPU_PREPARED_META: u16 = 4;
pub const GPU_PREPARED_STATE: u16 = 5;
pub const GPU_PREPARED_POOL: u16 = 6;
pub const GPU_PREPARED_NEXT: u16 = 7;
pub const GPU_PREPARED_READY: u16 = 2;
pub const GPU_PREPARED_RECORDING: u16 = 1;
pub const GPU_POOL_BASE: u16 = 0;
pub const GPU_POOL_CAPACITY: u16 = 1;
pub const GPU_POOL_HEAD: u16 = 2;
pub const GPU_KIND_PREPARED: u16 = 12;
pub const GPU_KIND_PREPARED_POOL: u16 = 13;
// Read-only draw input view begins at GPU_CB_CAMERA, spanning camera and state.
pub const GPU_CB_DESIRED_WORDS: u16 = 48;
pub const GPU_DESIRED_CAMERA: u16 = 0;
pub const GPU_DESIRED_STATE: u16 = 32;
// Shared sequential CPU scratch: final native blocks, not persistent bindings.
pub const GPU_SCRATCH_GLOBAL: u16 = 0;
pub const GPU_SCRATCH_RENDER: u16 = 116;
pub const GPU_SCRATCH_TARGET: u16 = 48;
pub const GPU_SCRATCH_INSTANCE: u16 = 64;
pub const GPU_SCRATCH_UI: u16 = 128;
// Mesh binding payload is published before command encoding reuses this span.
pub const GPU_SCRATCH_VIEW: u16 = 192;

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

// Caller-local binding plan: addresses/offsets/counts are CPU u16 words.
pub const GPU_BIND_PLAN_CHANGES: u16 = 0;
pub const GPU_BIND_PLAN_MODEL: u16 = 1;
pub const GPU_BIND_PLAN_VIEW_OFFSET: u16 = 2;
pub const GPU_BIND_PLAN_WORLD_OFFSET: u16 = 3;
pub const GPU_BIND_PLAN_PUSH_END: u16 = 4;
pub const GPU_BIND_PLAN_CODE_WORDS: u16 = 5;
pub const GPU_BIND_PLAN_DRAW_MODE: u16 = 6;
// Command staging may overwrite View192..223 only after payload publication.
pub const GPU_SCRATCH_COMMAND: u16 = 160;
