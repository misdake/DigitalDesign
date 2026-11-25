//! Standard CB06 slice: mapped collision pages against unmodified reference queries.
use crate::dsl_rt::*;
mod boot_resources;
use boot_resources::*;
mod gpu_api;
use gpu_api::*;
mod gpu_window;
use gpu_window::gpu_window_asset_read;
mod integer;
mod sampling;
mod static_assets;
mod terrain;
use terrain::{Hit, Normal, Terrain};

static TERRAIN_SAMPLE: u16 = 0;
static TERRAIN_FIELD: u16 = 0;
static TERRAIN_CHECKS: u16 = 0;
static TERRAIN_SWITCHES: u16 = 0;
static TERRAIN_PHASE: u16 = 0;

fn mark(phase: u16) {
    unsafe {
        addr_of(&TERRAIN_PHASE).write(0, phase);
    }
}
fn raw(data: Array<u16>, at: u16) -> fix32 {
    fix32::from_words(data[at], data[at + 1])
}
fn put(mut data: Array<u16>, at: u16, value: fix32) {
    data[at] = value.lo_bits();
    data[at + 1] = value.hi_bits();
}
fn sentinel_normal() -> Normal {
    Normal {
        x: fix32::from_words(0x1111, 0x1111),
        y: fix32::from_words(0x2222, 0x2222),
        z: fix32::from_words(0x3333, 0x3333),
    }
}

fn main() {
    mark(1);
    let input: Buf<u16, 66> = Buf::new([0; 66]);
    let result: Buf<u16, 59> = Buf::new([0; 59]);
    let local: Buf<u16, 6> = Buf::new([33, 0, 65459, 65535, 91, 0]);
    let transform: Buf<u16, 24> = Buf::new([
        0, 0, 0, 0, 61440, 65535, 0, 0, 4096, 0, 0, 0, 4096, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    let incoming = read_dseg_page(3);
    let mut scope = Terrain::begin(TERRAIN_PAGE_A);
    let mut record: u16 = 0;
    let mut count = TERRAIN_QUERY_COUNT;
    if TERRAIN_QUERY_LIMIT != 0 && TERRAIN_QUERY_LIMIT < count {
        count = TERRAIN_QUERY_LIMIT;
    }
    while record < count {
        unsafe {
            addr_of(&TERRAIN_SAMPLE).write(0, record);
        }
        let offset = fmul_lo32(fix32::from_words(record, 0), integer::from_i16(66));
        if !gpu_window_asset_read(
            input.as_array(),
            TERRAIN_QUERIES.first_sector + (offset.hi_bits() << 10) + (offset.lo_bits() >> 6),
            offset.lo_bits() & 63,
            66,
        ) {
            halt(0x7c10);
        }
        let data = input.as_array();
        let x = raw(data, 0);
        let y = raw(data, 2);
        let z = raw(data, 4);
        mark(2);
        let cell = scope.cell_at(x, z);
        if cell == TERRAIN_CELL_A {
            scope.bind(TERRAIN_PAGE_A);
        } else if cell == TERRAIN_CELL_B {
            scope.bind(TERRAIN_PAGE_B);
        } else if cell != 65535 {
            halt(0x7c11);
        }
        let mut hit = Hit {
            height: integer::from_i16(-55),
            node: fix32::from_words(0x3344, 0x1122),
            face: fix32::from_words(0x7788, 0x5566),
            nx: integer::from_i16(-12),
            ny: integer::from_i16(-13),
            nz: integer::from_i16(-14),
            surface: fix32::from_words(0xaabb, 0x8899),
            special: integer::from_i16(-77),
            extra0: integer::from_i16(19),
            extra1: integer::from_i16(23),
            extra2: integer::from_i16(-29),
            extra3: integer::from_i16(31),
            flags: fix32::from_words(0x1234, 0xabcd),
        };
        let mut output = result.as_array();
        output[0u16] = scope.query(x, z, &mut hit);
        let values = unsafe { view_of(&mut hit).as_ptr().as_u16_array() };
        let mut field: u16 = 0;
        while field < 26 {
            output[field + 1] = values[field];
            field += 1;
        }
        let mut height = fix32::from_words(0x5678, 0x1234);
        let found = scope.height(x, z);
        output[27u16] = found.status;
        if found.status <= 1 {
            height = found.height;
        }
        put(output, 28, height);
        height = fix32::from_words(0x5678, 0x1234);
        let mut normal = sentinel_normal();
        let found = scope.normal_height(x, z, &mut normal);
        output[30u16] = found.status;
        if found.status <= 1 {
            height = found.height;
        }
        put(output, 31, height);
        put(output, 33, normal.x);
        put(output, 35, normal.y);
        put(output, 37, normal.z);
        normal = sentinel_normal();
        output[39u16] = scope.fallback(x, y, z, data[6u16], &mut normal);
        put(output, 40, normal.x);
        put(output, 42, normal.y);
        put(output, 44, normal.z);
        let matrix = transform.as_array();
        put(matrix, 18, x - integer::from_i16(91));
        put(matrix, 20, y + integer::from_i16(77));
        put(matrix, 22, z + integer::from_i16(33));
        let mut auxiliary = sampling::Auxiliary {
            x: integer::from_i16(-1),
            y: integer::from_i16(-2),
            z: integer::from_i16(-3),
            kind: integer::from_i16(-11),
            surface: integer::from_i16(-12),
            cached: integer::from_i16(-13),
        };
        output[46u16] = auxiliary.sample(&mut scope, matrix, local.as_array(), data[6u16] & 1 != 0);
        put(output, 47, auxiliary.x);
        put(output, 49, auxiliary.y);
        put(output, 51, auxiliary.z);
        put(output, 53, auxiliary.kind);
        put(output, 55, auxiliary.surface);
        put(output, 57, auxiliary.cached);
        mark(3);
        field = 0;
        while field < 59 {
            if output[field] != data[field + 7] {
                unsafe {
                    addr_of(&TERRAIN_FIELD).write(0, field);
                }
                halt(0x7c12);
            }
            field += 1;
        }
        unsafe {
            addr_of(&TERRAIN_CHECKS).write(0, TERRAIN_CHECKS + 59);
        }
        record += 1;
    }
    unsafe {
        addr_of(&TERRAIN_SWITCHES).write(0, scope.switches);
    }
    scope.finish();
    // The scope began with the application's incoming map, not the first asset page.
    if read_dseg_page(3) != incoming {
        halt(0x7c13);
    }
    mark(4);
    halt(0x600d);
}
