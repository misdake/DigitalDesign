// One real candidate and the full immutable class table; other components are
// absent in this branch fixture. Persistent driving covers the full live list.
fn empty_deform(
    data: Array<u16>,
    records: Array<deformation::Record>,
) -> deform_contact::Component {
    let empty: Buf<crate::scene_meshes::Store, 0> = Buf::new([]);
    deform_contact::Component {
        resources: empty.as_array(),
        resource: 65535,
        object: 65535,
        state: deformation::HitState {
            dirty: fix32::zero(),
            last_vehicle: integer::from_i16(-1),
            sound_timer: fix32::zero(),
        },
        config: deformation::HitConfig {
            floor: fix32::zero(),
            compression: fix32::zero(),
            drift_shift: 0,
            material: 0,
            has_sound: false,
        },
        mesh: deformation::Mesh {
            vertices: unsafe { data.as_ptr().as_i16_array() },
            vertex_count: 0,
            flags: data,
            flag_count: 0,
        },
        sampling: deformation::disabled(data, records),
    }
}
fn run_car_deform(input: Array<u16>, out: Array<u16>, sine: Array<i16>, angle: Array<i16>) {
    let count = attitude::get(input, 12).lo_bits();
    let bounds_at = 92 + count * 4;
    let scene = scene_query::Scene::begin(static_assets::AssetBytes {
        first_sector: 16384,
        sector_count: attitude::get(input, bounds_at + 9).lo_bits(),
        byte_len_lo: attitude::get(input, bounds_at + 7).lo_bits(),
        byte_len_hi: attitude::get(input, bounds_at + 8).lo_bits(),
    });
    if !scene.valid {
        halt(0x7f12);
    }
    let raw_collider: Buf<u16, 60> = Buf::new([0; 60]);
    let mut raw = raw_collider.as_array();
    let mut i = 0u16;
    while i < 60 {
        raw[i] = input[28 + i];
        i += 1;
    }
    let properties: Buf<u16, 16> = Buf::new([0; 16]);
    let mut props = properties.as_array();
    attitude::put(props, 0, attitude::get(input, bounds_at + 6));
    attitude::put(props, 1, attitude::get(input, 44));
    i = 0;
    while i < 6 {
        props[10 + i] = attitude::get(input, bounds_at + i).lo_bits();
        i += 1;
    }
    let ids: Buf<u16, 2> = Buf::new([0; 2]);
    let world = scene_world::World {
        colliders: raw,
        properties: props,
        count: 1,
        fixed_count: 1,
        dynamic_page: 0,
        dynamic_capacity: 0,
        grid: spatial::Grid {
            page: 65535,
            origin: input,
            shift_x: 0,
            shift_z: 0,
            items: ids.as_array(),
            ranges: input,
        },
        deformables: ordered::List {
            items: ids.as_array(),
            count: 1,
            capacity: 1,
        },
    };
    let records: Buf<deformation::Record, 1> = Buf::new([deformation::record_zero()]);
    let components: Buf<deform_contact::Component, 7> = Buf::new([
        empty_deform(input, records.as_array()),
        empty_deform(input, records.as_array()),
        empty_deform(input, records.as_array()),
        empty_deform(input, records.as_array()),
        empty_deform(input, records.as_array()),
        empty_deform(input, records.as_array()),
        empty_deform(input, records.as_array()),
    ]);
    let mut parts = components.as_array();
    let vertices: Buf<i16, 300> = Buf::new([0; 300]);
    let flags: Buf<u16, 100> = Buf::new([0; 100]);
    let mut v = vertices.as_array();
    let mut f = flags.as_array();
    i = 0;
    while i < count * 3 {
        v[i] = attitude::get(input, 92 + i).lo_bits() as i16;
        i += 1;
    }
    i = 0;
    while i < count {
        f[i] = attitude::get(input, 92 + count * 3 + i).lo_bits();
        i += 1;
    }
    parts[0u16].object = 0;
    parts[0u16].state = deformation::HitState {
        dirty: attitude::get(input, 50),
        last_vehicle: attitude::get(input, 51),
        sound_timer: attitude::get(input, 52),
    };
    parts[0u16].config = deformation::HitConfig {
        floor: attitude::get(input, 45),
        compression: attitude::get(input, 46),
        drift_shift: attitude::get(input, 47).lo_bits(),
        material: attitude::get(input, 48).lo_bits() as i16,
        has_sound: attitude::get(input, 49) != fix32::zero(),
    };
    parts[0u16].mesh = deformation::Mesh {
        vertices: v,
        vertex_count: count,
        flags: f,
        flag_count: attitude::get(input, 13).lo_bits(),
    };
    let state_words: Buf<u16, 8> = Buf::new([0; 8]);
    let state = state_words.as_array();
    attitude::put(state, 0, attitude::get(input, 8));
    attitude::put(state, 1, attitude::get(input, 7));
    attitude::put(state, 2, attitude::get(input, 9));
    attitude::put(state, 3, attitude::get(input, 10));
    let cooldown: Buf<u16, 2> = Buf::new([0; 2]);
    attitude::put(cooldown.as_array(), 0, attitude::get(input, 4));
    let seed: Buf<u16, 2> = Buf::new([0; 2]);
    attitude::put(seed.as_array(), 0, attitude::get(input, 5));
    let mut vehicle = deform_contact::Vehicle {
        component_count: scene.header[9u16],
        position: unsafe { input.as_ptr().add(108).as_u16_array() },
        previous: unsafe { input.as_ptr().add(114).as_u16_array() },
        base_position: unsafe { input.as_ptr().add(120).as_u16_array() },
        velocity: unsafe { input.as_ptr().add(126).as_u16_array() },
        material: state,
        attached: unsafe { state.as_ptr().add(2).as_u16_array() },
        accumulator: unsafe { state.as_ptr().add(4).as_u16_array() },
        current_slot: unsafe { state.as_ptr().add(6).as_u16_array() },
        corners: unsafe { input.as_ptr().add(176).as_u16_array() },
        corner_stride: 1,
        player: attitude::get(input, 2) != fix32::zero(),
        mode: attitude::get(input, 1),
        dt: attitude::get(input, 0),
        impact: impact::Vehicle {
            entity: attitude::get(input, 53),
            cooldown: cooldown.as_array(),
            contacts: unsafe { input.as_ptr().add(132).as_u16_array() },
            contact_stride: 3,
            forward: attitude::get(input, 3),
            basis: unsafe { input.as_ptr().add(156).as_u16_array() },
            has_basis: attitude::get(input, 87) != fix32::zero(),
        },
    };
    let lines: Buf<u16, 1600> = Buf::new([0; 1600]);
    let tail: Buf<u16, 16> = Buf::new([0; 16]);
    let batches: Buf<impact::Batch, 4> = Buf::new([
        impact::batch_zero(),
        impact::batch_zero(),
        impact::batch_zero(),
        impact::batch_zero(),
    ]);
    let mut impacts = impact::State {
        page: 65535,
        batches: batches.as_array(),
        next: attitude::get(input, 6),
    };
    let offsets: Buf<u16, 12> = Buf::new([0, 0, 0, 0, 2000, 0, 0, 0, 0, 0, 63536, 65535]);
    let mut ctx = impact::Context {
        frame: integer::from_i16(17),
        seed: seed.as_array(),
        pool: effect_pool::Pool {
            page: 65535,
            lines: lines.as_array(),
            count: fix32::zero(),
            overflow: effect_pool::line_zero(),
            exhausted: tail.as_array(),
        },
        sine: sine,
        angle: angle,
        forward_offset: offsets.as_array(),
        reverse_offset: unsafe { offsets.as_array().as_ptr().add(6).as_u16_array() },
    };
    effect_pool::reset(&mut ctx.pool);
    let status =
        deform_contact::respond(&mut vehicle, &scene, &world, parts, &mut impacts, &mut ctx);
    impact_capture::write(&mut impacts, &vehicle.impact, &mut ctx, out);
    attitude::put(out, 0, status);
    i = 1;
    while i < 4 {
        attitude::put(out, i, integer::from_i16(-99));
        i += 1;
    }
    i = 0;
    while i < 4 {
        attitude::put(out, 1288 + i, attitude::get(state, i));
        i += 1;
    }
    i = 0;
    while i < 3 {
        attitude::put(out, 1292 + i, attitude::get(vehicle.velocity, i));
        i += 1;
    }
    attitude::put(out, 1295, parts[0u16].state.dirty);
    attitude::put(out, 1296, parts[0u16].state.last_vehicle);
    attitude::put(out, 1297, parts[0u16].state.sound_timer);
    i = 0;
    while i < count * 3 {
        attitude::put(out, 1298 + i, integer::from_i16(v[i]));
        i += 1;
    }
}
