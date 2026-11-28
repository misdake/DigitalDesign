//! Persistent vehicle effects, pose and inactive motion over the shared owner.
#![link_cseg = "vehicle"]
use super::*;
pub const CAR_WORD: u16 = 12212;
pub const CAR_WORDS: u16 = 956;
pub const CAR_CAPACITY: u16 = 3;
pub const BODY_WORD: u16 = 6810;
pub const BODY_END_WORD: u16 = 7670;
pub const FINISH_MESH_WORD: u16 = 7670;
pub const METADATA_WORD: u16 = 8010;
pub const INPUT_WORD: u16 = 8094;
pub const FINISH_AI_WORD: u16 = 8172;
pub struct State {
    pub enabled: bool,
    pub page: u16,
    pub auxiliary: Array<u16>,
    pub shared: car_effects::Shared,
    pub prefixes: Array<u16>,
    pub prepared: Array<prefix::Shared>,
    pub racers: Array<u16>,
    pub hit_fields: Array<u16>,
}
pub(super) fn ptr(w: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(w).as_u16_array() }
}
pub(super) fn sub(a: Array<u16>, w: u16) -> Array<u16> {
    unsafe { a.as_ptr().add(w as i16).as_u16_array() }
}
pub(super) fn g(a: Array<u16>, i: u16) -> fix32 {
    fix32::load_words(a, i * 2)
}
fn p(a: Array<u16>, i: u16, v: fix32) {
    fix32::store_words(a, i * 2, v);
}
fn n(v: i16) -> fix32 {
    fix32::from_words(v as u16, (v >> 15) as u16)
}
pub(super) fn row(slot: u16) -> Array<u16> {
    ptr(0xc000 + CAR_WORD + slot * CAR_WORDS)
}
// Bind stable addresses after the level's storage has been reserved.
pub fn bind(
    page: u16,
    quads: Array<quad_pool::Pool>,
    impacts: Array<impact::State>,
    auxiliary: Array<u16>,
    prepared: Array<prefix::Shared>,
) -> State {
    State {
        enabled: true,
        page: page,
        auxiliary: auxiliary,
        prefixes: ptr(0),
        prepared: prepared,
        racers: ptr(0),
        hit_fields: ptr(0),
        shared: car_effects::Shared {
            pickup: pickup_light::State { banks: ptr(0xc000) },
            impacts: impacts,
            debris: ptr(0xc000 + 8722),
            shield: ptr(0xc000 + 9086),
            quads,
            temporary: temporary::State {
                core: ptr(0xc000 + 9896),
                rows: ptr(0xc000 + 9978),
            },
            owners: vehicle_debris::Owners {
                rows: sub(row(0), 840),
                count: 2,
                stride: 478,
            },
            own: 0,
            owner_slots: 2,
            impact_accumulator: auxiliary,
            sound: ptr(0xc000 + 10170),
        },
    }
}
// Select the same effects owner as the full tail. Terrain temporarily borrows
// this page and restores it before any following vehicle/effect field access.
pub fn inactive(
    state: &mut State,
    car: &mut driving::Car,
    config: &driving::Config,
    frame: Array<u16>,
    world: &mut ground::World,
    context: &mut impact::Context,
) -> fix32 {
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, state.page);
    }
    let b = row(car.owner);
    let metadata = ptr(0xc000 + METADATA_WORD + car.owner * 28);
    let core = sub(state.prefixes, car.owner * 50);
    let contacts = unsafe { car.ground.points.as_array().as_ptr().as_u16_array() };
    let live = vehicle_inactive::State {
        poses: vehicle_pose::State {
            physical_position: sub(car.motion, 6),
            reference: car.attitude,
            base_position: sub(car.motion, 18),
            body_basis: sub(car.attitude, 18),
            body_position: sub(car.attitude, 36),
            tracks: car.tracks,
            turret: b,
            gun: sub(b, 24),
            attachments: sub(b, 48),
            bottom_position: sub(b, 96),
            recoil: sub(b, 102),
        },
        previous: sub(car.motion, 12),
        velocity: car.motion,
        angular: sub(car.attitude, 42),
        acceleration: sub(car.drive, 26),
        pivot: sub(car.attitude, 52),
        rates: sub(car.motion, 24),
        travel: sub(car.suspension, 6),
        travel_stride: 6,
        hull: car.hull.as_array(),
        sample: unsafe { view_of(&mut car.ground).as_ptr().as_u16_array() },
        local_velocity: car.drive,
        track_speed: sub(car.drive, 12),
        load: sub(car.drive, 30),
        vertical_delta: sub(car.suspension, 60),
        boost: car.boost,
        core: core,
        spin: sub(core, 20),
        boost_control: sub(core, 38),
        pose_config: sub(b, 848),
        pose_input: sub(b, 902),
        stored_input: sub(metadata, 2),
        height: metadata,
        spray: sub(b, 106),
        smoke: sub(b, 480),
        lights: sub(b, 830),
        debris: sub(b, 842),
        cooldown: sub(state.auxiliary, car.owner * 36),
        contacts: sub(contacts, 6),
        contact_stride: 17,
        reverse_braking: sub(b, 948),
        disabled: sub(b, 950),
    };
    state.shared.own = car.owner;
    state.shared.impact_accumulator = sub(state.auxiliary, car.owner * 36 + 30);
    p(
        config.drive,
        2,
        g(ptr(0xc000 + INPUT_WORD + car.owner * 26), 3),
    );
    p(config.drive, 18, g(core, 8));
    p(config.drive, 19, g(core, 9));
    p(sub(state.hit_fields, car.owner * 24 + 20), 0, g(core, 8));
    p(sub(state.hit_fields, car.owner * 24 + 20), 1, g(core, 9));
    let status = vehicle_inactive::step(
        &live,
        &vehicle_inactive::Input {
            entity: car.entity,
            player: car.player,
            drive: config.drive,
            hull: sub(metadata, 16),
            pickup: ptr(0xc000 + 10322),
            light_offset: sub(b, 938),
            light_material: g(sub(b, 944), 0),
            numerics: config.attitude[4u16],
            dt: g(frame, 0),
            number: g(frame, 26),
            mode: g(frame, 1),
            throttle: g(frame, 19),
            brake: g(frame, 20),
        },
        world,
        &state.prepared[0u16],
        &mut state.shared,
        context,
    );
    // These two aliases expose the live prefix timer to the complete snapshot.
    // The inactive branch leaves pose_input.timer and blast protection alone.
    p(sub(b, 952), 0, g(core, 4));
    p(sub(b, 954), 0, g(core, 5));
    let mut quads = state.shared.quads;
    p(ptr(0xc000 + 6800), 0, quads[0u16].count);
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
// Called while the complete phase is mapped, including after a prepare failure.
pub fn publish_prefix(
    car: &driving::Car,
    config: &driving::Config,
    prefix: &prefix::State,
    drive_grip: Array<u16>,
) {
    let b = row(car.owner);
    p(sub(b, 902), 0, g(prefix.core, 4));
    p(sub(b, 952), 0, g(prefix.core, 4));
    p(sub(b, 954), 0, g(prefix.core, 5));
    p(car.blast, 3, g(prefix.spin, 7));
    p(config.drive, 18, g(prefix.core, 8));
    p(config.drive, 19, g(prefix.core, 9));
    p(drive_grip, 0, g(prefix.core, 8));
    p(drive_grip, 1, g(prefix.core, 9));
}
pub fn advance(
    state: &mut State,
    car: &mut driving::Car,
    config: &driving::Config,
    frame: Array<u16>,
    impact_vehicle: &mut impact::Vehicle,
    context: &mut impact::Context,
) -> fix32 {
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, state.page);
    }
    let b = row(car.owner);
    let dt = g(frame, 0);
    let number = g(frame, 26);
    let contacts = unsafe { car.ground.points.as_array().as_ptr().as_u16_array() };
    let materials = unsafe { view_of(&mut car.ground.material0).as_ptr().as_u16_array() };
    let travel: Buf<u16, 8> = Buf::new([0; 8]);
    let t = travel.as_array();
    let mut i = 0u16;
    while i < 4 {
        p(t, i, g(car.suspension, i * 6 + 3));
        i += 1;
    }
    p(sub(b, 848), 11, car.turret_z);
    // The driving diagnostic retains the two source acceleration columns in
    // its packed attitude row as well as the authoritative drive view.
    p(car.attitude, 24, g(car.drive, 14));
    p(car.attitude, 25, g(car.drive, 13));
    let motion = car_motion::State {
        poses: vehicle_pose::State {
            physical_position: sub(car.motion, 6),
            reference: car.attitude,
            base_position: sub(car.motion, 18),
            body_basis: sub(car.attitude, 18),
            body_position: sub(car.attitude, 36),
            tracks: car.tracks,
            turret: b,
            gun: sub(b, 24),
            attachments: sub(b, 48),
            bottom_position: sub(b, 96),
            recoil: sub(b, 102),
        },
        previous: sub(car.motion, 12),
        velocity: car.motion,
        angular: sub(car.attitude, 42),
        acceleration: sub(car.drive, 26),
        pivot: sub(car.attitude, 52),
        articulation: car.articulation.as_array(),
        lean: car.lean.as_array(),
        contacts: sub(contacts, 6),
        contact_stride: 17,
        contact_gap: 4,
        travel: t,
        hull: car.hull.as_array(),
        ready: unsafe { view_of(&mut car.ready).as_ptr().as_u16_array() },
        rates: sub(car.motion, 24),
        gun: car.gun,
        gun_binding: car.gun_binding,
    };
    let status = car_motion::integrate_and_pose(
        &motion,
        &car_motion::Config {
            attitude: config.attitude,
            body: config.body,
            pose: sub(b, 848),
            sine: config.sine,
            angle: config.angle,
        },
        &car_motion::Frame {
            dt: dt,
            number: number,
            player: car.player,
            mode: g(frame, 1),
            pose: vehicle_pose::Environment {
                timer: g(sub(b, 902), 0).lo_bits(),
                values: sub(b, 902),
            },
        },
    );
    if status != n(0) {
        unsafe {
            write_dseg_page(3, saved);
        }
        return status;
    }
    let mut basis = car.attitude;
    let external = g(sub(b, 902), 8);
    if external == n(2) {
        basis = sub(b, 920);
    }
    impact_vehicle.basis = basis;
    impact_vehicle.has_basis = external != n(0);
    let special = g(sub(b, 946), 0) != n(0);
    let mut visible = n(0);
    if car.visible {
        visible = n(1);
    }
    state.shared.own = car.owner;
    state.shared.impact_accumulator = sub(state.auxiliary, car.owner * 36 + 30);
    let mut shield_bank = n(0);
    if car.player {
        shield_bank = n(1);
    }
    let input = car_effects::Input {
        special: special,
        spray: track_effects::SprayInput {
            dt: dt,
            track_speed: g(car.drive, 6),
            velocity: car.motion,
            basis: car.attitude,
            materials: materials,
            kinds: sub(contacts, 12),
            kind_stride: 17,
            contacts: sub(contacts, 6),
            contact_stride: 17,
            visible: visible,
        },
        marks: track_effects::MarksInput {
            player: car.player,
            deformable: g(sub(state.auxiliary, car.owner * 36), 14) != n(-1),
            mode: g(frame, 1),
            dt: dt,
            lateral: g(car.motion, 14),
            slip: g(car.motion, 15),
            velocity: car.motion,
            materials: materials,
            travel: t,
            kinds: sub(contacts, 12),
            kind_stride: 17,
            contacts: sub(contacts, 6),
            contact_stride: 17,
            tracks: car.tracks,
            track_stride: 12,
        },
        smoke: vehicle_smoke::Input {
            dt: dt,
            player: car.player,
            special: special,
            velocity: car.motion,
            lateral: g(car.motion, 14),
            slip: g(car.motion, 15),
            penetration: g(car.motion, 13),
            materials: materials,
            contacts: sub(contacts, 6),
            contact_stride: 17,
            corners: sub(car.suspension, 4),
            corner_stride: 6,
            turret: b,
            body: sub(car.attitude, 18),
        },
        lights: track_effects::LightInput {
            player: car.player,
            disabled: g(sub(b, 950), 0) != n(0),
            active: true,
            visible: visible,
            queued: false,
            reverse_braking: g(sub(b, 948), 0) != n(0),
            material: g(sub(b, 944), 0),
            throttle: g(frame, 19),
            brake: g(frame, 20),
            forward: g(car.drive, 2),
            position: sub(car.motion, 6),
            basis: basis,
            has_basis: external != n(0),
            materials: materials,
            heights: sub(contacts, 18),
            height_stride: 17,
            offset: sub(b, 938),
        },
        debris: vehicle_debris::Input {
            dt: dt,
            player: car.player,
            position: sub(car.motion, 18),
            velocity: car.motion,
            has_plane: car.ground.has_extra,
            plane_distance: g(car.ground.extra.as_array(), 3),
            materials: materials,
            contacts: sub(contacts, 6),
            contact_stride: 34,
            camera_position: ptr(0xc000 + 10174),
            camera_forward: ptr(0xc000 + 10180),
        },
        shield: vehicle_shield::Input {
            enabled: g(car.ground.effect.as_array(), 2) != n(0),
            bank: shield_bank,
            dt: dt,
            position: sub(car.motion, 18),
            basis: basis,
            has_basis: external != n(0),
        },
        base_position: sub(car.motion, 18),
        number: number,
        temporary_timer: g(sub(b, 952), 0).lo_bits() as i16,
        temporary_slot: g(sub(b, 954), 0).lo_bits() as i16,
    };
    let status = car_effects::tick(
        &car_effects::State {
            spray: track_effects::Spray {
                header: sub(b, 106),
                particles: sub(b, 112),
            },
            marks: sub(b, 304),
            smoke: sub(b, 480),
            lights: sub(b, 830),
        },
        &mut state.shared,
        &input,
        &car_effects::Tuning {
            spray: track_effects::SprayTuning {
                kinds: unsafe { ptr(0xc000 + 10234).as_ptr().as_i16_array() },
                styles: ptr(0xc000 + 10186),
            },
            smoke_sockets: ptr(0xc000 + 10250),
            pickup: ptr(0xc000 + 10322),
        },
        impact_vehicle,
        context,
    );
    if status == n(0) {
        car_motion::finish(&motion, dt);
        i = 0;
        while i < 3 {
            p(car.drive, 3 + i, g(car.attitude, 21 + i));
            i += 1;
        }
        p(car.drive, 9, g(car.motion, 15));
        p(car.drive, 12, g(car.motion, 14));
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
