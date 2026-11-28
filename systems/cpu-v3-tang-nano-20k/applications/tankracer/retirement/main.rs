//! Source-created ordinary objects retired through the normal application path.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;
mod gpu_window;
mod static_assets;
use static_assets::AssetBytes;
mod attitude;
mod collision;
mod entities;
mod identity_capture;
mod integer;
mod lifecycle;
mod object_query;
mod ordered;
mod retirement_check;
mod scene_lifecycle;
mod scene_query;
mod scene_world;
mod spatial;
mod trig_lookup;
static RETIREMENT_CASE: u16 = 0;
static RETIREMENT_FIELD: u16 = 0;
static RETIREMENT_CHECKS: u16 = 0;
fn read(dst: Array<u16>, asset: AssetBytes, offset: fix32, count: u16) {
    if !gpu_window::gpu_window_asset_read(
        dst,
        asset.first_sector + (offset.hi_bits() << 10) + (offset.lo_bits() >> 6),
        offset.lo_bits() & 63,
        count,
    ) {
        halt(0x7f31);
    }
}
fn main() {
    let incoming = read_dseg_page(3);
    let storage: RetirementStorage = retirement_storage();
    let directory: Buf<u16, 6> = Buf::new([0; 6]);
    let d = directory.as_array();
    let input = storage.input.as_array();
    let output = storage.output.as_array();
    let mut case = 0u16;
    while case < RETIREMENT_CASES {
        unsafe {
            addr_of(&RETIREMENT_CASE).write(0, case);
        }
        read(d, RETIREMENT_DIRECTORY, fix32::from_words(case * 6, 0), 6);
        read(
            input,
            RETIREMENT_INPUTS,
            fix32::from_words(d[0u16], d[1u16]),
            d[2u16],
        );
        retirement_check::run_scene_lifecycle(input, output, RETIREMENT_SCENE);
        read(
            input,
            RETIREMENT_EXPECTED,
            fix32::from_words(d[3u16], d[4u16]),
            d[5u16],
        );
        let mut i = 0u16;
        while i < d[5u16] {
            if output[i] != input[i] {
                unsafe {
                    addr_of(&RETIREMENT_FIELD).write(0, i);
                }
                halt(0x8000 + i);
            }
            i += 1;
        }
        unsafe {
            addr_of(&RETIREMENT_CHECKS).write(0, RETIREMENT_CHECKS + 1);
        }
        if read_dseg_page(3) != incoming {
            halt(0x7f32);
        }
        case += 1;
    }
    halt(0x600d);
}
