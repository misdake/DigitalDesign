// Four contact responses run the actual impact and damage implementations.
fn run_ground_response(input: Array<u16>, out: Array<u16>, sine: Array<i16>, angle: Array<i16>) {
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
        next: fix32::zero(),
    };
    let seed: Buf<u16, 2> = Buf::new([7, 0]);
    let cooldown: Buf<u16, 2> = Buf::new([500, 0]);
    let mut context = impact::Context {
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
        forward_offset: unsafe { input.as_ptr().add(60).as_u16_array() },
        reverse_offset: unsafe { input.as_ptr().add(66).as_u16_array() },
    };
    effect_pool::reset(&mut context.pool);
    let points: Buf<u16, 136> = Buf::new([0; 136]);
    let pt = points.as_array();
    let springs: Buf<u16, 48> = Buf::new([0; 48]);
    let sp = springs.as_array();
    let velocity: Buf<u16, 6> = Buf::new([0; 6]);
    let vel = velocity.as_array();
    let angular: Buf<u16, 6> = Buf::new([0; 6]);
    let rates = angular.as_array();
    let delta: Buf<u16, 6> = Buf::new([0; 6]);
    let diff = delta.as_array();
    let mut i = 0u16;
    while i < 4 {
        attitude::put(pt, i * 17 + 6, attitude::get(input, 36 + i * 5));
        attitude::put(sp, i * 6 + 2, attitude::get(input, 37 + i * 5));
        let mut j = 0u16;
        while j < 3 {
            attitude::put(pt, i * 17 + 12 + j, attitude::get(input, 38 + i * 5 + j));
            j += 1;
        }
        i += 1;
    }
    i = 0;
    while i < 3 {
        attitude::put(vel, i, attitude::get(input, 56 + i));
        attitude::put(diff, i, integer::from_i16(99));
        i += 1;
    }
    attitude::put(rates, 1, attitude::get(input, 59));
    let flags: Buf<u16, 10> = Buf::new([0; 10]);
    let selected: Buf<u16, 4> = Buf::new([0; 4]);
    let vertices: Buf<i16, 9> = Buf::new([4096, -17, -4096, 0, 300, 20, 300, 5, 0]);
    let indices: Buf<u16, 3> = Buf::new([0, 1, 2]);
    let normals: Buf<i16, 3> = Buf::new([11, 22, 33]);
    let position = unsafe { input.as_ptr().add(126).as_u16_array() };
    let mut ground = ground_response::State {
        velocity: vel,
        angular: rates,
        points: pt,
        springs: sp,
        position: position,
        delta: diff,
        impact: impact::Vehicle {
            entity: integer::from_i16(1),
            cooldown: cooldown.as_array(),
            contacts: unsafe { input.as_ptr().add(18).as_u16_array() },
            contact_stride: 3,
            forward: attitude::get(input, 5),
            basis: unsafe { input.as_ptr().add(42).as_u16_array() },
            has_basis: true,
        },
        damage: damage::Target {
            has_revision: false,
            revision: unsafe { Ptr::from_addr(0).as_u16_array() },
            state: flags.as_array(),
            player: attitude::get(input, 61) != fix32::zero(),
            basis: unsafe { input.as_ptr().add(42).as_u16_array() },
            position: position,
            selected: selected.as_array(),
            mesh: damage::Mesh {
                page: 65535,
                present: attitude::get(input, 62) != fix32::zero(),
                vertices: vertices.as_array(),
                vertex_count: 3,
                indices: indices.as_array(),
                normals: normals.as_array(),
                face_count: 1,
            },
        },
        mode: attitude::get(input, 60),
    };
    let status = ground_response::step(&mut ground, &mut impacts, &mut context);
    attitude::put(out, 0, fix32::from_words(status, 0));
    i = 1;
    while i < 4 {
        attitude::put(out, i, integer::from_i16(-99));
        i += 1;
    }
    impact_capture::write(&mut impacts, &ground.impact, &mut context, out);
    i = 0;
    while i < 3 {
        attitude::put(out, 1288 + i, attitude::get(vel, i));
        attitude::put(out, 1304 + i, attitude::get(diff, i));
        i += 1;
    }
    attitude::put(out, 1291, attitude::get(rates, 1));
    i = 0;
    while i < 4 {
        let mut j = 0u16;
        while j < 3 {
            attitude::put(out, 1292 + i * 3 + j, attitude::get(pt, i * 17 + 12 + j));
            j += 1;
        }
        i += 1;
    }
    i = 0;
    while i < 10 {
        attitude::put(out, 1307 + i, fix32::from_words(flags[i], 0));
        i += 1;
    }
    i = 0;
    while i < 9 {
        attitude::put(out, 1317 + i, integer::from_i16(vertices[i]));
        i += 1;
    }
    i = 0;
    while i < 3 {
        attitude::put(out, 1326 + i, integer::from_i16(normals[i]));
        i += 1;
    }
}
