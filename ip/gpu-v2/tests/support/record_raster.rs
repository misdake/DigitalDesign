//! Bounded test-only semantic record/cursor; host arithmetic is atomic oracle work.
//! No producer Report or source lookup is reachable from Reader/Decoded/Cursor.
#[path = "record_raster/codec.rs"]
mod codec;
#[path = "record_raster/cursor.rs"]
mod cursor;
pub use codec::{profile, Decoded, Encoded, Values, WORDS};
pub use cursor::{Attributes, Reader};

pub const MAX_WALL: u64 = 120_000;
pub const MAX_SOURCES: u64 = 6;
pub const MAX_RECORDS: u64 = 12;
pub const MAX_COVERAGE: u64 = 16_384;
