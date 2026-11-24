//! Retired, caller-owned heap geometry. Only changed cells patch packed indices.
use crate::dsl_rt::*;
mod boot_resources;
use boot_resources::*;
mod session;
use session::*;
mod gpu_api;
use gpu_api::*;
mod device_abi;
use device_abi::*;

// owner, directory word, GPU sectors, records, generation, hidden, cells, map word.
fn data(owner: Ptr) -> Array<u16> {
    unsafe { owner.as_u16_array() }
}
fn idle() {
    if dev_recv(GPU_DEVICE, GPU_STATUS) & (GPU_STATUS_BUSY | GPU_STATUS_TRANSPORT_ERROR) != 0 {
        halt(0xfb01);
    }
}
fn sectors(cells: u16) -> u16 {
    (64 + cells * 120 + ((cells * 10 + 2) / 3) * 4 + 63) >> 6
}
fn bucket_of(cell: u16) -> u16 {
    (cell % GAME_SESSION.level.width) / 3
        + (div_u16(cell, GAME_SESSION.level.width) / 3) * ((GAME_SESSION.level.width + 2) / 3)
}
// RMW each six-bit field. Every qword has three18-bit triangles and10 padding bits.
fn indices(mut words: Array<u16>, slot: u16, hidden: bool) {
    let base = 64 + (words[1u16] >> 8) * 4;
    let mut triangle: u16 = 0;
    while triangle < 10 {
        let t = slot * 10 + triangle;
        let mut corner: u16 = 0;
        while corner < 3 {
            let value = if hidden {
                0u16
            } else {
                slot * 20
                    + (triangle / 2) * 4
                    + if corner == 0 {
                        0u16
                    } else {
                        corner + (triangle & 1)
                    }
            };
            let bit = (t % 3) * 18 + corner * 6;
            let at = base + (t / 3) * 4 + (bit >> 4);
            let shift = bit & 15;
            words[at] = (words[at] & !(63u16 << shift)) | (value << shift);
            if shift > 10 {
                let tail = 63u16 >> (16 - shift);
                words[at + 1] = (words[at + 1] & !tail) | (value >> (16 - shift));
            }
            corner += 1;
        }
        triangle += 1;
    }
}
fn record(mut words: Array<u16>, cells: Array<u16>, count: u16, bucket: u16) {
    let mut at: u16 = 0;
    while at < sectors(count) * 64 {
        words[at] = 0;
        at += 1;
    }
    words[0u16] = count * 20 | ((count * 10) << 8);
    words[1u16] = 20 | ((count * 30) << 8);
    let columns = (GAME_SESSION.level.width + 2) / 3;
    words[9u16] = (((bucket % columns) * 6) as i16 - GAME_SESSION.level.width as i16) as u16;
    words[10u16] = 39424;
    words[11u16] = 65535;
    words[13u16] =
        ((div_u16(bucket, columns) * 6) as i16 - GAME_SESSION.level.height as i16) as u16;
    words[14u16] = 265;
    let template: Buf<u16, 20> = Buf::new([0; 20]);
    let mut tile: u16 = 0;
    while tile < count {
        let x = cells[tile] % GAME_SESSION.level.width;
        let z = div_u16(cells[tile], GAME_SESSION.level.width);
        let variant = (x * 3 + z * 5) % 4;
        let mut face: u16 = 0;
        while face < 5 {
            blox_fragile_template(variant, face, template.as_array());
            let mut corner: u16 = 0;
            while corner < 4 {
                let bits = template[corner * 5];
                let dx = (x % 3) * 256 + (bits & 1) * 256;
                let dy = ((bits >> 1) & 1) * 51;
                let dz = (z % 3) * 256 + ((bits >> 2) & 1) * 256;
                let out = 64 + (tile * 20 + face * 4 + corner) * 6;
                words[out] = dx | ((dy & 63) << 10);
                words[out + 1] = (dy >> 6) | (dz << 4);
                let mut attribute: u16 = 0;
                while attribute < 4 {
                    words[out + 2 + attribute] = template[corner * 5 + 1 + attribute];
                    attribute += 1;
                }
                corner += 1;
            }
            face += 1;
        }
        indices(words, tile, false);
        tile += 1;
    }
}
pub fn fragile_begin() -> Ptr {
    let owner = malloc(8);
    let mut c = data(owner);
    let mut i: u16 = 0;
    while i < 8 {
        c[i] = 0;
        i += 1;
    }
    c[4u16] = GAME_SESSION.generation - 1;
    c[5u16] = 65535;
    owner
}
pub fn fragile_matches_level(owner: Ptr) -> bool {
    data(owner)[4u16] == GAME_SESSION.generation
}
// Both CBs are retired/reset by the application before a level rebuild/free.
pub fn fragile_rebuild(owner: Ptr) {
    idle();
    let mut c = data(owner);
    if c[0u16] != 0 {
        free(Ptr::from_addr(c[0u16]));
        c[0u16] = 0;
    }
    c[3u16] = 0;
    c[6u16] = 0;
    c[5u16] = 65535;
    c[1u16] = 0;
    c[2u16] = 0;
    c[7u16] = 0;
    let buckets = ((GAME_SESSION.level.width + 2) / 3) * ((GAME_SESSION.level.height + 2) / 3);
    let mut bucket: u16 = 0;
    let mut payload: u16 = 0;
    while bucket < buckets {
        let mut n: u16 = 0;
        let mut i: u16 = 0;
        while i < GAME_SESSION.level.cell_count {
            if GAME_SESSION.level_field(i, 0) == 2 && bucket_of(i) == bucket {
                n += 1;
            }
            i += 1;
        }
        c[6u16] += n;
        c[3u16] += (n + 2) / 3;
        payload += (n / 3) * sectors(3);
        if n % 3 != 0 {
            payload += sectors(n % 3);
        }
        bucket += 1;
    }
    if c[3u16] != 0 {
        let map_sectors = (c[6u16] * 2 + 63) >> 6;
        let directory_sectors = (c[3u16] * 2 + 63) >> 6;
        let words = (map_sectors + directory_sectors + payload) * 64;
        if words > 8192 {
            halt(0xfb02);
        }
        let allocation = malloc(words + 63);
        let base = (allocation.addr() + 63) & 65472;
        c[0u16] = allocation.addr();
        c[7u16] = base;
        c[1u16] = base + map_sectors * 64;
        c[2u16] = directory_sectors + payload;
        let mut directory = unsafe { Ptr::from_addr(c[1u16]).as_u16_array() };
        let mut map = unsafe { Ptr::from_addr(base).as_u16_array() };
        let mut cells: Buf<u16, 3> = Buf::new([0; 3]);
        let mut next = c[1u16] + directory_sectors * 64;
        let mut entry: u16 = 0;
        let mut mapped: u16 = 0;
        bucket = 0;
        while bucket < buckets {
            let mut count: u16 = 0;
            let mut i: u16 = 0;
            while i <= GAME_SESSION.level.cell_count {
                if i < GAME_SESSION.level.cell_count
                    && GAME_SESSION.level_field(i, 0) == 2
                    && bucket_of(i) == bucket
                {
                    cells[count] = i;
                    count += 1;
                }
                if count == 3 || (i == GAME_SESSION.level.cell_count && count != 0) {
                    let record_words = unsafe { Ptr::from_addr(next).as_u16_array() };
                    record(record_words, cells.as_array(), count, bucket);
                    directory[entry * 2] = next >> 6;
                    directory[entry * 2 + 1] = sectors(count) - 1;
                    let mut slot: u16 = 0;
                    while slot < count {
                        map[mapped * 2] = cells[slot];
                        map[mapped * 2 + 1] = ((next >> 6) << 2) | slot;
                        mapped += 1;
                        slot += 1;
                    }
                    next += sectors(count) * 64;
                    entry += 1;
                    count = 0;
                }
                i += 1;
            }
            bucket += 1;
        }
        if entry != c[3u16] || mapped != c[6u16] || next != c[1u16] + c[2u16] * 64 {
            halt(0xfb03);
        }
    }
    c[4u16] = GAME_SESSION.generation;
}
fn patch(c: Array<u16>, cell: u16, hidden: bool) {
    let map = unsafe { Ptr::from_addr(c[7u16]).as_u16_array() };
    let mut i: u16 = 0;
    while i < c[6u16] {
        if map[i * 2] == cell {
            let address = map[i * 2 + 1];
            let words = unsafe { Ptr::from_addr((address >> 2) << 6).as_u16_array() };
            indices(words, address & 3, hidden);
            return;
        }
        i += 1;
    }
    halt(0xfb04);
}
pub fn fragile_sync(owner: Ptr) {
    let mut c = data(owner);
    let hidden = if GAME_SESSION.fracture_cell != 65535
        && GAME_SESSION.fragile_dropped(GAME_SESSION.fracture_cell)
    {
        GAME_SESSION.fracture_cell
    } else {
        65535u16
    };
    if hidden != c[5u16] {
        idle();
        if c[5u16] != 65535 {
            patch(c, c[5u16], false);
        }
        if hidden != 65535 {
            patch(c, hidden, true);
        }
        c[5u16] = hidden;
    }
}
pub fn fragile_mesh(owner: Ptr) -> Mesh {
    let c = data(owner);
    Mesh::from_native(MeshDesc {
        directory_sector: c[1u16] >> 6,
        directory_word: 0,
        meshlet_count: c[3u16],
        storage: unsafe { GpuMemory::from_sectors(c[1u16] >> 6, c[2u16]) },
    })
}
pub fn fragile_count(owner: Ptr) -> u16 {
    data(owner)[3u16]
}
pub fn fragile_hidden(owner: Ptr) -> u16 {
    data(owner)[5u16]
}
pub fn fragile_end(owner: Ptr) {
    idle();
    let c = data(owner);
    if c[0u16] != 0 {
        free(Ptr::from_addr(c[0u16]));
    }
    free(owner);
}
