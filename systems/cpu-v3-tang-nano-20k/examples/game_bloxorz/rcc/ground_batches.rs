//! Load-time exact grid meshlets; only caller-owned low-memory output is written.
use crate::dsl_rt::*;
mod bloxorz_core;
use bloxorz_core::TileKind;
mod boot_resources;
use boot_resources::*;
mod session;
use session::*;
mod gpu_api;
use gpu_api::*;
mod level_assets;
use level_assets::*;

pub static GROUND_BATCH_MODE: u16 = 1;
static GROUND_BASE: u16 = 0;

pub fn ground_global_quantization() -> bool {
    GAME_SESSION.level.width <= 15 && GAME_SESSION.level.height <= 15
}

pub fn ground_face_key(face: u16) -> u16 {
    let x = face & 31;
    let z = (face >> 5) & 31;
    let direction = (face >> 10) & 7;
    let mut material: u16 = 0;
    if GAME_SESSION.level_field(z * GAME_SESSION.level.width + x, 0) == TileKind::Splitter as u16 {
        material = if direction == 0 { 1u16 } else { 2u16 };
    }
    let bucket = if ground_global_quantization() {
        if direction == 0 {
            0u16
        } else {
            1u16
        }
    } else {
        1 + x / 3 + (z / 3) * ((GAME_SESSION.level.width + 2) / 3)
    };
    material * 32 + bucket
}
fn ground_index(mut words: Array<u16>, base: u16, triangle: u16) {
    let vertex = (triangle / 2) * 4;
    let mut corner: u16 = 0;
    while corner < 3 {
        let value = vertex
            + if corner == 0 {
                0u16
            } else {
                corner + (triangle & 1)
            };
        let bit = (triangle % 3) * 18 + corner * 6;
        let at = base + (triangle / 3) * 4 + (bit >> 4);
        let shift = bit & 15;
        words[at] |= value << shift;
        if shift > 10 {
            words[at + 1] |= value >> (16 - shift);
        }
        corner += 1;
    }
}
// info: next face, total record sectors, material, quantization bucket, quads.
// Caller owns at least512 output heap words and five info words. The complete
// record stays in the heap; only the20-word immutable template is local.
pub fn ground_meshlet(first: u16, mut words: Array<u16>, mut info: Array<u16>) {
    let count = ground_face_count();
    info[0u16] = first;
    info[1u16] = 0;
    if first >= count {
        return;
    }
    let key = ground_face_key(ground_face(first));
    let material = key >> 5;
    let bucket = key & 31;
    let global = ground_global_quantization();
    let mut i: u16 = 0;
    while i < 512 {
        words[i] = 0;
        i += 1;
    }
    let template: Buf<u16, 20> = Buf::new([0; 20]);
    let mut cursor = first;
    let mut quads: u16 = 0;
    while cursor < count && quads < 16 {
        let face = ground_face(cursor);
        if ground_face_key(face) != key {
            break;
        }
        let x = face & 31;
        let z = (face >> 5) & 31;
        let direction = (face >> 10) & 7;
        let variant = face >> 13;
        blox_ground_template(
            if material == 1 {
                20 + variant
            } else {
                direction * 4 + variant
            },
            template.as_array(),
        );
        let mut corner: u16 = 0;
        while corner < 4 {
            let bits = template[corner * 5];
            let dx = if bucket == 0 || global {
                x * 64 + (bits & 1) * 64
            } else {
                (x % 3) * 256 + (bits & 1) * 256
            };
            let dz = if bucket == 0 || global {
                z * 64 + ((bits >> 2) & 1) * 64
            } else {
                (z % 3) * 256 + ((bits >> 2) & 1) * 256
            };
            let dy = if bucket == 0 {
                0u16
            } else {
                ((bits >> 1) & 1) * 51
            };
            let at = 64 + quads * 24 + corner * 6;
            words[at] = dx | ((dy & 63) << 10);
            words[at + 1] = (dy >> 6) | (dz << 4);
            let mut attribute: u16 = 0;
            while attribute < 4 {
                words[at + 2 + attribute] = template[corner * 5 + 1 + attribute];
                attribute += 1;
            }
            corner += 1;
        }
        quads += 1;
        cursor += 1;
    }
    let triangles = quads * 2;
    let index_base = 64 + quads * 24;
    let mut triangle: u16 = 0;
    while triangle < triangles {
        ground_index(words, index_base, triangle);
        triangle += 1;
    }
    words[0u16] = quads * 4 | (triangles << 8);
    words[1u16] = 20 | ((quads * 6) << 8);
    let bx = if bucket == 0 || global {
        0u16
    } else {
        ((bucket - 1) % ((GAME_SESSION.level.width + 2) / 3)) * 6
    };
    let bz = if bucket == 0 || global {
        0u16
    } else {
        ((bucket - 1) / ((GAME_SESSION.level.width + 2) / 3)) * 6
    };
    words[9u16] = (bx as i16 - GAME_SESSION.level.width as i16) as u16;
    words[13u16] = (bz as i16 - GAME_SESSION.level.height as i16) as u16;
    if bucket != 0 {
        words[10u16] = if global { 26624u16 } else { 39424u16 };
        words[11u16] = if global { 65534u16 } else { 65535u16 };
    }
    words[14u16] = if bucket == 0 || global {
        267u16
    } else {
        265u16
    };
    info[0u16] = cursor;
    info[1u16] = (index_base + ((triangles + 2) / 3) * 4 + 63) >> 6;
    info[2u16] = material;
    info[3u16] = bucket;
    info[4u16] = quads;
}

pub fn ground_enabled() -> bool {
    GROUND_BATCH_MODE == 1
}
fn ground_metadata() -> Array<u16> {
    unsafe { Ptr::from_addr(GROUND_BASE).as_u16_array() }
}
pub fn ground_begin() -> Ptr {
    if !ground_enabled() {
        return Ptr::from_addr(0);
    }
    let owner = malloc(8255);
    let base = (owner.addr() + 63) & 65472;
    unsafe {
        addr_of(&GROUND_BASE).write(0, base);
    }
    let mut metadata = ground_metadata();
    let mut i: u16 = 0;
    while i < 128 {
        metadata[i] = 0;
        i += 1;
    }
    owner
}
pub fn ground_matches_level() -> bool {
    GROUND_BASE != 0 && ground_metadata()[7u16] == GAME_SESSION.motion.level + 1
}
// Both caller-owned CBs have retired and been reset before this mutation.
// Storage remains allocated and unmoved until ground_end, after CB Drop.
pub fn ground_rebuild() {
    level_geometry_load(GAME_SESSION.motion.level);
    if GAME_SESSION.level.width == 0
        || GAME_SESSION.level.height == 0
        || GAME_SESSION.level.width > 31
        || GAME_SESSION.level.height > 31
        || ((GAME_SESSION.level.width + 2) / 3) * ((GAME_SESSION.level.height + 2) / 3) > 31
    {
        halt(0xb13);
    }
    let mut metadata = ground_metadata();
    let mut i: u16 = 0;
    while i < 128 {
        metadata[i] = 0;
        i += 1;
    }
    let info: Buf<u16, 5> = Buf::new([0; 5]);
    let mut first: u16 = 0;
    let mut used: u16 = 2;
    let mut entry: u16 = 0;
    loop {
        if used > 120 || entry >= 32 {
            halt(0xb12);
        }
        let words = unsafe {
            Ptr::from_addr(GROUND_BASE)
                .add((used * 64) as i16)
                .as_u16_array()
        };
        ground_meshlet(first, words, info.as_array());
        if info[1u16] == 0 {
            break;
        }
        let material = info[2u16];
        if metadata[material * 2 + 1] == 0 {
            metadata[material * 2] = entry;
        }
        metadata[material * 2 + 1] += 1;
        metadata[64 + entry * 2] = (GROUND_BASE >> 6) + used;
        metadata[65 + entry * 2] = info[1u16] - 1;
        used += info[1u16];
        entry += 1;
        first = info[0u16];
    }
    metadata[6u16] = used;
    metadata[7u16] = GAME_SESSION.motion.level + 1;
}
pub fn ground_count(material: u16) -> u16 {
    ground_metadata()[material * 2 + 1]
}
pub fn ground_mesh(material: u16) -> Mesh {
    let metadata = ground_metadata();
    Mesh::from_native(MeshDesc {
        directory_sector: (GROUND_BASE >> 6) + 1,
        directory_word: metadata[material * 2] * 2,
        meshlet_count: metadata[material * 2 + 1],
        storage: unsafe { GpuMemory::from_sectors(GROUND_BASE >> 6, metadata[6u16]) },
    })
}
pub fn ground_end(owner: Ptr) {
    if owner.addr() != 0 {
        unsafe {
            addr_of(&GROUND_BASE).write(0, 0);
        }
        free(owner);
    }
}
