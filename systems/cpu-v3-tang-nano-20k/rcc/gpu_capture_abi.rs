//! Diagnostic lease descriptor. Addresses/lengths are physical bytes, low16 first.
//! This is a software bridge ABI, never a header inside executable GPU commands.
pub const GPU_CAPTURE_VERSION: u16 = 1;
pub const GPU_CAPTURE_WORDS: u16 = 64;
pub const GPU_CAPTURE_RANGE_WORDS: u16 = 8;
pub const GPU_CAPTURE_MAX_RANGES: u16 = 64;
pub const GPU_CAPTURE_MANIFEST_WORDS: u16 = 16;

pub const GPU_CAPTURE_H_VERSION: u16 = 0;
pub const GPU_CAPTURE_H_WORDS: u16 = 1;
pub const GPU_CAPTURE_H_FLAGS: u16 = 2;
pub const GPU_CAPTURE_H_BINDINGS: u16 = 3;
pub const GPU_CAPTURE_H_TOKEN: u16 = 4;
pub const GPU_CAPTURE_H_SUBMISSION: u16 = 6;
pub const GPU_CAPTURE_H_FRAME: u16 = 8;
pub const GPU_CAPTURE_H_SLOT: u16 = 9;
pub const GPU_CAPTURE_H_COUNT: u16 = 10;
pub const GPU_CAPTURE_H_PUBLICATION_LOW: u16 = 11;
pub const GPU_CAPTURE_H_COMMAND_BASE: u16 = 12;
pub const GPU_CAPTURE_H_COMMAND_BYTES: u16 = 14;
pub const GPU_CAPTURE_H_COMMAND_PADDED_BYTES: u16 = 16;
pub const GPU_CAPTURE_H_OBJECT_BASE: u16 = 18;
pub const GPU_CAPTURE_H_OBJECT_BYTES: u16 = 20;
pub const GPU_CAPTURE_H_UI_BASE: u16 = 22;
pub const GPU_CAPTURE_H_UI_BYTES: u16 = 24;
pub const GPU_CAPTURE_H_COLOR_BASE: u16 = 26;
pub const GPU_CAPTURE_H_DEPTH_BASE: u16 = 28;
pub const GPU_CAPTURE_H_FRONT_BASE: u16 = 30;
pub const GPU_CAPTURE_H_FRONT_FORMAT: u16 = 32;
pub const GPU_CAPTURE_H_TARGET_FORMAT: u16 = 33;
pub const GPU_CAPTURE_H_DISPLAY_STATUS: u16 = 34;
pub const GPU_CAPTURE_H_GPU_STATUS: u16 = 35;
pub const GPU_CAPTURE_H_RANGE_COUNT: u16 = 36;
pub const GPU_CAPTURE_H_RANGE_WORDS: u16 = 37;
pub const GPU_CAPTURE_H_RANGE_VERSION: u16 = 38;
pub const GPU_CAPTURE_H_LAYOUT_ABI: u16 = 39;
pub const GPU_CAPTURE_H_COMMAND_ABI: u16 = 40;
pub const GPU_CAPTURE_H_UI_REFERENCE: u16 = 41;
pub const GPU_CAPTURE_H_PLANE_BYTES: u16 = 42;
pub const GPU_CAPTURE_H_PUBLICATION_HIGH: u16 = 44;
pub const GPU_CAPTURE_H_MANIFEST_WORDS: u16 = 45;
// Words46/47 are reserved zero. Words48..63 hold the declared manifest ID.
pub const GPU_CAPTURE_H_MANIFEST: u16 = 48;

pub const GPU_CAPTURE_PUBLISHED: u16 = 1;
pub const GPU_CAPTURE_BINDINGS_SELF_CONTAINED: u16 = 1;
pub const GPU_CAPTURE_NATIVE_COMMAND_ABI: u16 = 1;
pub const GPU_CAPTURE_LAYOUT_ABI: u16 = 1;
// Storage selection is independent of the native command version.
pub const GPU_CAPTURE_LAYOUT_ROW_ABI: u16 = 1;
pub const GPU_CAPTURE_LAYOUT_COLUMN_ABI: u16 = 2;
pub const GPU_CAPTURE_RANGE_VERSION: u16 = 1;

pub const GPU_CAPTURE_R_FLAGS: u16 = 0;
pub const GPU_CAPTURE_R_TAG: u16 = 1;
pub const GPU_CAPTURE_R_BASE: u16 = 2;
pub const GPU_CAPTURE_R_BYTES: u16 = 4;
pub const GPU_CAPTURE_R_GENERATION: u16 = 6;
pub const GPU_CAPTURE_READ: u16 = 1;
pub const GPU_CAPTURE_WRITE: u16 = 2;
pub const GPU_CAPTURE_IMMUTABLE: u16 = 4;
pub const GPU_CAPTURE_PREIMAGE: u16 = 8;
pub const GPU_CAPTURE_FULL_CLEAR: u16 = 16;
pub const GPU_CAPTURE_FLAGS_ALL: u16 = 31;
pub const GPU_CAPTURE_TAG_COMMANDS: u16 = 1;
pub const GPU_CAPTURE_TAG_OBJECTS: u16 = 2;
pub const GPU_CAPTURE_TAG_UI: u16 = 3;
pub const GPU_CAPTURE_TAG_MESH: u16 = 4;
pub const GPU_CAPTURE_TAG_TEXTURE: u16 = 5;
pub const GPU_CAPTURE_TAG_ATLAS: u16 = 6;
pub const GPU_CAPTURE_TAG_COLOR: u16 = 7;
pub const GPU_CAPTURE_TAG_DEPTH: u16 = 8;
pub const GPU_CAPTURE_TAG_FRONT: u16 = 9;

pub const GPU_CAPTURE_CONTINUE: u16 = 0;
pub const GPU_CAPTURE_RESUMED: u16 = 1;
pub const GPU_CAPTURE_FATAL: u16 = 2;
pub const GPU_CAPTURE_OFF: u16 = 0;
pub const GPU_CAPTURE_PREPARED: u16 = 1;
pub const GPU_CAPTURE_PAUSED: u16 = 2;
pub const GPU_CAPTURE_DUMPING: u16 = 3;
pub const GPU_CAPTURE_FAULT: u16 = 4;

pub const GPU_CAPTURE_OK: u16 = 0;
pub const GPU_CAPTURE_NO_LEASE: u16 = 1;
pub const GPU_CAPTURE_BOUNDS: u16 = 2;
pub const GPU_CAPTURE_MEMORY: u16 = 3;
pub const GPU_CAPTURE_GPU_ERROR: u16 = 4;
pub const GPU_CAPTURE_DISPLAY_ERROR: u16 = 5;
pub const GPU_CAPTURE_IDENTITY_ERROR: u16 = 6;
pub const GPU_CAPTURE_CAPACITY: u16 = 7;
pub const GPU_CAPTURE_GATE_ERROR: u16 = 8;
