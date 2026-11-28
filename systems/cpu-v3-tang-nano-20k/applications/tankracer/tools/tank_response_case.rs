// Two independently owned writable vehicle meshes and ordinary world views.
static DAMAGE_INDICES: Buf<u16, 3> = Buf::new([0, 1, 2]);
static DAMAGE_SELECTED: Buf<u16, 4> = Buf::new([0; 4]);
struct TankBuffers {
    cooldown: Buf<u16, 2>,
    delta: Buf<u16, 6>,
    flags: Buf<u16, 10>,
    vertices: Buf<i16, 9>,
    normals: Buf<i16, 3>,
}
fn tank_buffers() -> TankBuffers {
    TankBuffers {
        cooldown: Buf::new([500, 0]),
        delta: Buf::new([99, 0, 99, 0, 99, 0]),
        flags: Buf::new([0; 10]),
        vertices: Buf::new([4096, -17, -4096, 0, 300, 20, 300, 5, 0]),
        normals: Buf::new([11, 22, 33]),
    }
}
fn tank_vehicle(
    input: Array<u16>,
    geometry: &object_query::Tank,
    data: &TankBuffers,
    corners: Array<u16>,
    present: bool,
) -> tank_response::Vehicle {
    tank_response::Vehicle {
        velocity: input,
        local_velocity: unsafe { input.as_ptr().add(6).as_u16_array() },
        previous_position: unsafe { input.as_ptr().add(12).as_u16_array() },
        timer: unsafe { input.as_ptr().add(18).as_u16_array() },
        delta: data.delta.as_array(),
        corners: corners,
        corner_stride: 1,
        impact: impact::Vehicle {
            entity: geometry.collider.entity,
            cooldown: data.cooldown.as_array(),
            contacts: geometry.contacts.as_array(),
            contact_stride: 3,
            forward: attitude::get(input, 5),
            basis: geometry.collider.external.as_array(),
            has_basis: geometry.collider.has_external != fix32::zero(),
        },
        damage: damage::Target {
            has_revision: false,
            revision: unsafe { Ptr::from_addr(0).as_u16_array() },
            state: data.flags.as_array(),
            player: attitude::get(input, 10) != fix32::zero(),
            basis: geometry.collider.basis.as_array(),
            position: geometry.collider.position.as_array(),
            selected: DAMAGE_SELECTED.as_array(),
            mesh: damage::Mesh {
                page: 65535,
                present: present,
                vertices: data.vertices.as_array(),
                vertex_count: 3,
                indices: DAMAGE_INDICES.as_array(),
                normals: data.normals.as_array(),
                face_count: 1,
            },
        },
    }
}
fn run_tank_response(
    input: Array<u16>,
    out: Array<u16>,
    sine: Array<i16>,
    angle: Array<i16>,
    live: bool,
) {
    let geometry: Buf<object_query::Tank, 2> =
        Buf::new([object_query::tank_zero(), object_query::tank_zero()]);
    let mut geo = geometry.as_array();
    let mut raw = unsafe { geo.as_ptr().as_u16_array() };
    let mut i = 0u16;
    while i < 208 {
        raw[i] = input[16 + i];
        i += 1;
    }
    let slots: Buf<u16, 9> = Buf::new([65535; 9]);
    let mut ids = slots.as_array();
    i = 0;
    while i < 9 {
        ids[i] = attitude::get(input, 134 + i).lo_bits();
        i += 1;
    }
    let data: Buf<TankBuffers, 2> = Buf::new([tank_buffers(), tank_buffers()]);
    let buffers = data.as_array();
    let corners: Buf<u16, 8> = Buf::new([0, 0, 1, 0, 2, 0, 3, 0]);
    let rows = unsafe { input.as_ptr().add(224).as_u16_array() };
    let peer_row = unsafe { rows.as_ptr().add(22).as_u16_array() };
    let vehicles: Buf<tank_response::Vehicle, 2> = Buf::new([
        tank_vehicle(
            rows,
            &geo[0u16],
            &buffers[0u16],
            corners.as_array(),
            attitude::get(input, 143) != fix32::zero(),
        ),
        tank_vehicle(
            peer_row,
            &geo[1u16],
            &buffers[1u16],
            corners.as_array(),
            attitude::get(input, 144) != fix32::zero(),
        ),
    ]);
    let present: Buf<u16, 2> = Buf::new([1; 2]);
    let mut entries = present.as_array();
    if live {
        entries[0u16] = attitude::get(input, 145).lo_bits();
        entries[1u16] = attitude::get(input, 146).lo_bits();
    }
    let mut world = tank_response::World {
        geometry: geo,
        vehicles: vehicles.as_array(),
        slots: ids,
        present: entries,
        geometry_count: attitude::get(input, 5).lo_bits(),
        vehicle_count: attitude::get(input, 6).lo_bits(),
        hit: integer::from_i16(-1),
    };
    let lines: Buf<u16, 1600> = Buf::new([0; 1600]);
    let tail: Buf<u16, 16> = Buf::new([0; 16]);
    let offset: Buf<u16, 12> = Buf::new([0, 0, 0, 0, 2000, 0, 0, 0, 0, 0, 63536, 65535]);
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
    if attitude::get(input, 4) == integer::from_i16(10) {
        impacts.next = integer::from_i16(4);
    }
    let seed: Buf<u16, 2> = Buf::new([7, 0]);
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
        forward_offset: offset.as_array(),
        reverse_offset: unsafe { offset.as_ptr().add(6).as_u16_array() },
    };
    effect_pool::reset(&mut context.pool);
    let env = tank_response::Environment {
        mode: attitude::get(input, 1),
        dt: attitude::get(input, 0),
        separation_limit: attitude::get(input, 2),
    };
    let mut peer = tank_response::Peer {
        tank: integer::from_i16(-1),
        vx: fix32::zero(),
        vz: fix32::zero(),
    };
    let mut own = attitude::get(input, 3).lo_bits();
    let mut status = 0u16;
    let mut car_status = fix32::zero();
    let mut call = 0u16;
    while call < attitude::get(input, 7).lo_bits() {
        if live {
            car_status = tank_response::cars(own, &mut world, &env, &mut impacts, &mut context);
            if car_status != fix32::zero() {
                break;
            }
        } else {
            status =
                tank_response::step(own, &mut world, &env, &mut peer, &mut impacts, &mut context);
            if status >= 2 {
                break;
            }
        }
        own = 1 - own;
        call += 1;
    }
    attitude::put(out, 0, fix32::from_words(status, 0));
    if live {
        attitude::put(out, 0, car_status);
    }
    i = 1;
    while i < 4 {
        attitude::put(out, i, integer::from_i16(-99));
        i += 1;
    }
    impact_capture::write(
        &mut impacts,
        &world.vehicles[0u16].impact,
        &mut context,
        out,
    );
    attitude::put(out, 1288, world.hit);
    attitude::put(out, 1289, peer.tank);
    attitude::put(out, 1290, peer.vx);
    attitude::put(out, 1291, peer.vz);
    i = 0;
    while i < 22 {
        attitude::put(out, 1292 + i, attitude::get(rows, i));
        i += 1;
    }
    attitude::put(out, 1301, attitude::get(world.vehicles[0u16].timer, 0));
    attitude::put(out, 1312, attitude::get(world.vehicles[1u16].timer, 0));
    attitude::put(
        out,
        1314,
        attitude::get(world.vehicles[1u16].impact.cooldown, 0),
    );
    i = 0;
    while i < 2 {
        let mut j = 0u16;
        while j < 10 {
            attitude::put(
                out,
                1315 + i * 22 + j,
                fix32::from_words(buffers[i].flags[j], 0),
            );
            j += 1;
        }
        j = 0;
        while j < 9 {
            attitude::put(
                out,
                1325 + i * 22 + j,
                integer::from_i16(buffers[i].vertices[j]),
            );
            j += 1;
        }
        j = 0;
        while j < 3 {
            attitude::put(
                out,
                1334 + i * 22 + j,
                integer::from_i16(buffers[i].normals[j]),
            );
            j += 1;
        }
        i += 1;
    }
    if live {
        i = 0;
        while i < 2 {
            let mut j = 0u16;
            while j < 3 {
                attitude::put(
                    out,
                    1359 + i * 3 + j,
                    attitude::get(buffers[i].delta.as_array(), j),
                );
                j += 1;
            }
            i += 1;
        }
    }
}
