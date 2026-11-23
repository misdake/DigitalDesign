//! Standard CB06 entry; target code loads and verifies every frozen sample.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;
mod attitude;
mod drive;
mod gpu_window;
mod integer;
mod suspension;
mod trig_lookup;
mod vehicle;
use gpu_window::*;
static PHYSICS_PHASE: u16 = 0;
static PHYSICS_SAMPLE: u16 = 0;
static PHYSICS_CHECKS: u16 = 0;
static PHYSICS_FAILURE: u16 = 0;
fn mark(phase: u16) {
    unsafe {
        addr_of(&PHYSICS_PHASE).write(0, phase);
    }
}
fn sample_read(dst: Array<u16>, record: u16, offset: u16, count: u16) {
    let root = unsafe { addr_of(&PHYSICS_SAMPLES).as_u16_array() };
    if !gpu_window_asset_read(
        dst,
        root[0u16],
        record * PHYSICS_RECORD_WORDS + offset,
        count,
    ) {
        halt(0x7e01);
    }
}
fn load(record: u16) {
    sample_read(
        unsafe { Ptr::from_addr(0x1100).as_u16_array() },
        record,
        0,
        58,
    );
    sample_read(
        unsafe { Ptr::from_addr(0x1180).as_u16_array() },
        record,
        58,
        88,
    );
    sample_read(
        unsafe { Ptr::from_addr(0x1300).as_u16_array() },
        record,
        146,
        32,
    );
    sample_read(
        unsafe { Ptr::from_addr(0x1380).as_u16_array() },
        record,
        178,
        32,
    );
    sample_read(
        unsafe { Ptr::from_addr(0x1400).as_u16_array() },
        record,
        210,
        8,
    );
    sample_read(
        unsafe { Ptr::from_addr(0x1500).as_u16_array() },
        record,
        218,
        52,
    );
    sample_read(
        unsafe { Ptr::from_addr(0x1600).as_u16_array() },
        record,
        270,
        520,
    );
    sample_read(
        unsafe { Ptr::from_addr(0x1900).as_u16_array() },
        record,
        790,
        5,
    );
    sample_read(
        unsafe { Ptr::from_addr(0x1920).as_u16_array() },
        record,
        795,
        4,
    );
    sample_read(
        unsafe { Ptr::from_addr(0x2000).as_u16_array() },
        record,
        799,
        36,
    );
    sample_read(
        unsafe { Ptr::from_addr(0x2100).as_u16_array() },
        record,
        835,
        58,
    );
}
fn physics_tick() {
    let a = unsafe { Ptr::from_addr(0x1100).as_u16_array() };
    let s = unsafe { Ptr::from_addr(0x1180).as_u16_array() };
    let d = unsafe { Ptr::from_addr(0x1300).as_u16_array() };
    let m = unsafe { Ptr::from_addr(0x1380).as_u16_array() };
    let b = unsafe { Ptr::from_addr(0x1400).as_u16_array() };
    let sc = unsafe { Ptr::from_addr(0x1500).as_u16_array() };
    let dc = unsafe { Ptr::from_addr(0x1600).as_u16_array() };
    let ac = unsafe { Ptr::from_addr(0x1900).as_u16_array() };
    let pc = unsafe { Ptr::from_addr(0x1920).as_u16_array() };
    let se = unsafe { Ptr::from_addr(0x2000).as_u16_array() };
    let f = unsafe { Ptr::from_addr(0x2100).as_u16_array() };
    let e = unsafe { Ptr::from_addr(0x1000).as_u16_array() };
    vehicle::boost(b, dc, f);
    vehicle::contact_stage(a, s, m, sc, se, e);
    attitude::put(d, 15, attitude::get(s, 40));
    if vehicle::power_stage(a, d, m, dc, f, attitude::get(s, 30)) != 0 {
        halt(0x7e02);
    }
    vehicle::integrate_position(m, f);
    let sine = unsafe { addr_of(&PHYSICS_SINE).as_i16_array() };
    let angle = unsafe { addr_of(&PHYSICS_ANGLE).as_i16_array() };
    if !vehicle::attitude_stage(a, d, ac, f, sine, angle) {
        halt(0x7e03);
    }
    vehicle::finish(s, a, m, pc, f);
}
fn verify_region(record: u16, offset: u16, address: u16, words: u16, component: u16) {
    let tmp: Buf<u16, 32> = Buf::new([0; 32]);
    let values = tmp.as_array();
    let actual = unsafe { Ptr::from_addr(address).as_u16_array() };
    let mut done = 0u16;
    while done < words {
        let mut count = words - done;
        if count > 32 {
            count = 32;
        }
        sample_read(values, record, offset + done, count);
        let mut i = 0u16;
        while i < count {
            if actual[done + i] != values[i] {
                unsafe {
                    addr_of(&PHYSICS_FAILURE).write(0, done + i);
                }
                halt(0x7e10 + component);
            }
            i += 1;
        }
        done += count;
    }
}
fn verify(record: u16) {
    verify_region(record, 893, 0x1100, 58, 0);
    verify_region(record, 951, 0x1180, 88, 1);
    verify_region(record, 1039, 0x1300, 32, 2);
    verify_region(record, 1071, 0x1380, 32, 3);
    verify_region(record, 1103, 0x1400, 8, 4);
    unsafe {
        addr_of(&PHYSICS_CHECKS).write(0, PHYSICS_CHECKS + 109);
    }
}
fn main() {
    let mut repeat = 0u16;
    while repeat < PHYSICS_REPEATS {
        let mut record = 0u16;
        while record < PHYSICS_RECORDS {
            unsafe {
                addr_of(&PHYSICS_SAMPLE).write(0, repeat * PHYSICS_RECORDS + record);
            }
            mark(1);
            load(record);
            mark(2);
            physics_tick();
            mark(3);
            verify(record);
            mark(4);
            record += 1;
        }
        repeat += 1;
    }
    halt(0x600d);
}
