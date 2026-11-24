//! One CPU snapshot of the current level's immutable render-face lists.
use crate::dsl_rt::*;
mod boot_resources;
use boot_resources::*;

struct LevelGeometryCache {
    level_key: u16,
    face_count: u16,
    ground_first: u16,
    ground_count: u16,
    words: Buf<u16, { BLOX_LEVEL_GEOMETRY_WORDS as usize }>,
}
static LEVEL_GEOMETRY: LevelGeometryCache = LevelGeometryCache {
    level_key: 0,
    face_count: 0,
    ground_first: 0,
    ground_count: 0,
    words: Buf::new([0; BLOX_LEVEL_GEOMETRY_WORDS as usize]),
};

pub fn level_geometry_load(which: u16) {
    if which >= BLOX_LEVEL_COUNT {
        halt(0xb15);
    }
    if LEVEL_GEOMETRY.level_key == which + 1 {
        return;
    }
    // Archive order is packed rules, visible faces, then ground count + faces.
    // The next level's packed rules (or the palette) bound this single span.
    let start = BLOX_LEVEL_HEADERS[which].faces_first;
    let end = if which + 1 < BLOX_LEVEL_COUNT {
        BLOX_LEVEL_HEADERS[which + 1].packed_first
    } else {
        BLOX_PALETTE_WORD
    };
    let faces = BLOX_LEVEL_HEADERS[which].face_count;
    if end <= start || end - start > BLOX_LEVEL_GEOMETRY_WORDS || faces >= end - start {
        halt(0xb15);
    }
    blox_asset_read(start, LEVEL_GEOMETRY.words.as_array(), end - start);
    let ground_count = LEVEL_GEOMETRY.words[faces];
    if ground_count != end - start - faces - 1 {
        halt(0xb15);
    }
    LEVEL_GEOMETRY.face_count = faces;
    LEVEL_GEOMETRY.ground_first = faces + 1;
    LEVEL_GEOMETRY.ground_count = ground_count;
    LEVEL_GEOMETRY.level_key = which + 1;
}
pub fn level_face_count() -> u16 {
    LEVEL_GEOMETRY.face_count
}
pub fn level_face(index: u16) -> u16 {
    LEVEL_GEOMETRY.words[index]
}
pub fn ground_face_count() -> u16 {
    LEVEL_GEOMETRY.ground_count
}
pub fn ground_face(index: u16) -> u16 {
    LEVEL_GEOMETRY.words[LEVEL_GEOMETRY.ground_first + index]
}
