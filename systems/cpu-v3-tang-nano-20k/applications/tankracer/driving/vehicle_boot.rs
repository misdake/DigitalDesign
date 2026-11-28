//! Cold publication of a constructed vehicle into the continuous driving owner.
#![link_cseg = "vehicle"]
use super::*;
pub struct Resources {
    pub models: AssetBytes,
    pub actors_page: u16,
    pub actors_words: u16,
    pub gun_words: u16,
    pub count: u16,
}
const WORD: u16 = 1088;
fn at(v: Array<u16>, w: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(w as i16).as_u16_array() }
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
pub fn construct(
    actors: Array<driving::Car>,
    prefixes: Array<prefix::State>,
    config: &driving::Config,
    complete: &car_runtime::State,
    resources: &Resources,
) {
    let saved = read_dseg_page(3);
    let fixed = read_dseg_page(2);
    let page = resources.actors_page;
    if WORD + vehicle_startup::WORDS + 1 > resources.actors_words {
        halt(0x7f86);
    }
    let metadata: Buf<u16, 200> = Buf::new([0; 200]);
    let m = metadata.as_array();
    let mut actors = actors;
    let mut i = 0u16;
    while i < resources.count {
        unsafe {
            write_dseg_page(3, page);
        }
        let mut out = unsafe { Ptr::from_addr(0xc000 + WORD).as_u16_array() };
        out[vehicle_startup::WORDS] = 0x6b17;
        read(m, resources.models, fix32::from_words(i * 200, 0), 200);
        let status = vehicle_startup::construct(
            out,
            &vehicle_startup::Model {
                pose: m,
                contacts: at(m, 54),
                shadow: unsafe { at(m, 78).as_ptr().as_i16_array() },
                shadow_vertices: m[144u16],
                shadow_radius: m[145u16],
                control: at(m, 90),
                sine: config.sine,
                angle: config.angle,
            },
            at(m, 102),
            at(m, 126),
        );
        if status != fix32::zero() {
            halt(0x7f53);
        }
        let car = &mut actors[i];
        let mut j = 0u16;
        while j < 266 {
            let mut data = car.attitude;
            data[j] = 0;
            j += 1;
        }
        copy(car.attitude, at(out, 6), 18);
        copy(at(car.attitude, 18), at(out, 30), 18);
        copy(at(car.attitude, 36), at(out, 48), 6);
        copy(at(car.attitude, 42), at(out, 220), 6);
        p(car.attitude, 24, g(out, 114));
        p(car.attitude, 25, g(out, 113));
        copy(at(car.attitude, 52), at(out, 230), 6);
        copy(car.suspension, at(out, vehicle_startup::SPRINGS), 80);
        copy(car.motion, at(out, 214), 6);
        copy(at(car.motion, 6), out, 6);
        copy(at(car.motion, 12), at(out, 208), 6);
        copy(at(car.motion, 18), at(out, 24), 6);
        copy(at(car.motion, 24), at(out, 331), 8);
        copy(car.boost, at(out, vehicle_startup::PREFIX), 8);
        copy(car.tracks, at(out, 54), 48);
        copy(
            unsafe { view_of(&mut car.ground).as_ptr().as_u16_array() },
            at(out, vehicle_startup::SAMPLING),
            206,
        );
        copy(car.articulation.as_array(), at(out, 236), 8);
        copy(car.lean.as_array(), at(out, 244), 8);
        copy(car.hull.as_array(), at(out, 292), 37);
        car.ready = g(at(out, 329), 0) != fix32::zero();
        car.turret_z = g(at(out, vehicle_startup::POSE), 11);
        copy(car.blast, at(out, vehicle_startup::FIELDS + 176), 14);
        if m[146u16] != resources.gun_words {
            halt(0x7f54);
        }
        let mut gun = car.gun;
        j = 0;
        while j < resources.gun_words {
            gun[j] = m[147 + j] as i16;
            j += 1;
        }
        copy(prefixes[i].core, at(out, vehicle_startup::PREFIX + 8), 50);
        // The hot state above uses the fixed world page. Publish effects with
        // the constructor row in DSEG2 and their destination in DSEG3.
        unsafe {
            write_dseg_page(2, page);
            write_dseg_page(3, complete.page);
        }
        out = unsafe { Ptr::from_addr(0x8000 + WORD).as_u16_array() };
        let live = unsafe {
            Ptr::from_addr(0xc000 + car_runtime::CAR_WORD + i * car_runtime::CAR_WORDS)
                .as_u16_array()
        };
        copy(live, at(out, 102), 106);
        copy(at(live, 106), at(out, vehicle_startup::EFFECTS), 374);
        copy(at(live, 480), at(out, vehicle_startup::EFFECTS + 374), 350);
        copy(at(live, 830), at(out, vehicle_startup::EFFECTS + 724), 10);
        p(live, 420, car.entity);
        copy(at(live, 842), at(out, vehicle_startup::EFFECTS + 734), 6);
        copy(at(live, 848), at(out, vehicle_startup::POSE), 54);
        copy(at(live, 902), at(out, vehicle_startup::POSE_INPUT), 36);
        p(at(live, 948), 0, fix32::zero());
        p(at(live, 950), 0, fix32::zero());
        p(at(live, 952), 0, fix32::zero());
        p(at(live, 954), 0, integer::from_i16(-1));
        let inactive =
            unsafe { Ptr::from_addr(0xc000 + car_runtime::METADATA_WORD + i * 28).as_u16_array() };
        copy(inactive, at(out, vehicle_startup::FIELDS + 16), 2);
        copy(at(inactive, 2), at(out, vehicle_startup::FIELDS + 76), 14);
        let player =
            unsafe { Ptr::from_addr(0xc000 + car_runtime::INPUT_WORD + i * 26).as_u16_array() };
        copy(player, at(out, vehicle_startup::FIELDS + 90), 6);
        copy(at(player, 8), at(out, vehicle_startup::FIELDS + 144), 2);
        copy(at(player, 10), at(out, vehicle_startup::FIELDS + 126), 14);
        copy(at(player, 24), at(out, vehicle_startup::FIELDS + 124), 2);
        if out[vehicle_startup::WORDS] != 0x6b17 {
            halt(0x7f87);
        }
        unsafe {
            write_dseg_page(2, fixed);
        }
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}
