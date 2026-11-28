//! Player input sampling over persistent vehicle state.
#![link_cseg = "vehicle"]
use super::*;
fn at(v: Array<u16>, word: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(word as i16).as_u16_array() }
}
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
pub fn sample(
    state: &car_runtime::State,
    car: &mut driving::Car,
    config: &driving::Config,
    frame: Array<u16>,
) -> fix32 {
    let command = g(frame, 29).lo_bits();
    let keys = command & 511;
    // This qualifier exercises sampled controls and Tank physics. The complete
    // Player method retains route/lap and fire at its intervening call sites.
    if !car.player || keys & 128 != 0 {
        return n(-1);
    }
    let previous = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, state.page);
    }
    let fields = ptr(0xc000 + car_runtime::INPUT_WORD + car.owner * 26);
    let stored = ptr(0xc000 + car_runtime::METADATA_WORD + 2 + car.owner * 28);
    let core = at(state.prefixes, car.owner * 50);
    let effects = ptr(0xc000 + car_runtime::CAR_WORD + car.owner * car_runtime::CAR_WORDS);
    p(fields, 12, g(frame, 3));
    let tuning: Buf<u16, 12> = Buf::new([800, 0, 70, 0, 800, 0, 720, 0, 720, 0, 700, 0]);
    let values: Buf<u16, 20> = Buf::new([0; 20]);
    let f = values.as_array();
    let dt = g(frame, 0);
    p(f, 0, dt);
    p(f, 1, g(fields, 12));
    p(f, 3, g(car.drive, 2));
    p(f, 4, fix32::from_words((command >> 9) & 15, 0));
    let axes: Buf<u16, 4> = Buf::new([0; 4]);
    let outcome = player_control::input(
        &player_control::Input {
            drive: stored,
            state: fields,
            countdown: at(core, 2),
            analog_latch: ptr(0xc000 + 10466),
        },
        f,
        tuning.as_array(),
        keys,
        axes.as_array(),
    );
    if outcome & 1 != 0 {
        let aux = at(state.auxiliary, car.owner * 36);
        player_control::collision_decay(
            &player_control::Collision {
                counters: at(aux, 30),
                ground_delta: at(aux, 10),
                tank_delta: at(aux, 4),
                obstacle_impulse: at(aux, 34),
            },
            dt,
            true,
        );
        p(
            fields,
            9,
            n(g(car.ground.effect.as_array(), 0).lo_bits() as i16),
        );
        player_control::turret(at(fields, 10), at(effects, 912), (keys >> 5) & 3, dt);
        let fast = outcome & 2 != 0 && g(car.drive, 2).abs() > fix32::from_words(53392, 3);
        let restoring = g(stored, 4) != n(0) && (g(core, 4) != n(0) || !fast);
        let mut slide = n(0);
        if restoring {
            let material = car.ground.material0;
            if material.hi_bits() != 0 || material.lo_bits() >= 10 {
                unsafe {
                    write_dseg_page(3, previous);
                }
                return n(-1);
            }
            slide = g(config.drive, 82 + material.lo_bits() * 18);
        }
        let brake_values: Buf<u16, 10> = Buf::new([0; 10]);
        let b = brake_values.as_array();
        p(b, 0, dt);
        p(b, 1, g(car.drive, 2));
        p(b, 2, g(frame, 1));
        p(b, 3, g(core, 4));
        p(b, 4, fix32::from_words((outcome >> 1) & 1, 0));
        let reference: Buf<u16, 4> = Buf::new([0; 4]);
        p(reference.as_array(), 0, slide);
        p(
            reference.as_array(),
            1,
            n(g(config.drive, 35).lo_bits() as i16),
        );
        player_control::brake(
            &player_control::Brake {
                saved_turn: at(fields, 4),
                reverse_braking: at(effects, 948),
                drive: stored,
                max_turn: at(fields, 6),
                yaw_lock: at(fields, 8),
            },
            b,
            reference.as_array(),
        );
    }
    p(frame, 4, g(fields, 1));
    p(frame, 28, g(fields, 4));
    copy(at(frame, 38), stored, 14);
    // The physical sampler owns column 29 after input protocol decoding.
    p(frame, 29, fix32::from_words((command >> 13) & 1, 0));
    unsafe {
        write_dseg_page(3, previous);
    }
    n(0)
}
