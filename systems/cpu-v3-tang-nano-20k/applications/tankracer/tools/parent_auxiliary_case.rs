// Persistent class-chain qualification. Each owner is moved once at loading;
// every call thereafter mutates the same rows, resources, queues and seed.
pub struct ParentCase {
    pub state: scene_parent::State,
    pub grid: spatial::Grid,
    pub actor: scene_parent::Actor,
    pub model: scene_meshes::Model,
    pub meta: Array<u16>,
    pub controls: Array<u16>,
    pub count: u16,
}
pub fn load_parent_case(
    input: Array<u16>,
    lifetime: Array<u16>,
    resource_memory: Array<u16>,
    scratch: Array<u16>,
    map: ground::World,
) -> ParentCase {
    let life = load_scene_lifecycle(life_at(input, 4), lifetime);
    let aux = life_at(input, 4 + input[0u16]);
    let meta = life_at(aux, input[1u16]);
    let resources = auxiliary_resources(aux, resource_memory);
    let object = auxiliary_object(aux, scratch, &resources);
    let no_actors: Buf<scene_parent::Actor, 0> = Buf::new([]);
    let no_cars: Buf<driving::Car, 0> = Buf::new([]);
    let no_debris: Buf<shatter::State, 0> = Buf::new([]);
    let no_words = unsafe { Ptr::from_addr(0).as_u16_array() };
    ParentCase {
        state: scene_parent::State {
            lifetime: life.store,
            runtime: life.runtime,
            resources: resources,
            map: map,
            actors: no_actors.as_array(),
            actor_count: 0,
            cars: no_cars.as_array(),
            car_count: 0,
            slots: no_words,
            symbols: no_words,
            models: mesh_catalog::Sources {
                asset: static_assets::AssetBytes {
                    first_sector: 0,
                    sector_count: 0,
                    byte_len_lo: 0,
                    byte_len_hi: 0,
                },
                count: 0,
                maximum_words: 0,
                records: no_words,
                indices: no_words,
            },
        },
        grid: life.grid,
        actor: scene_parent::Actor {
            fragments: object,
            properties: life_at(meta, 19),
            state: life_at(meta, 9),
            kind: meta[0u16],
            has_state: meta[1u16] != 0,
            reference_alias: meta[2u16] != 0,
            debris: no_debris.as_array(),
            attached: no_words,
            attached_count: 0,
            definition: no_words,
        },
        model: auxiliary_model(aux, 0),
        meta: meta,
        controls: life_at(meta, input[2u16]),
        count: input[3u16],
    }
}
// Six source parameters plus a returned scalar fit the target call convention.
pub struct ParentTables {
    pub sine: Array<i16>,
    pub angle: Array<i16>,
    pub inverse: Array<i16>,
}
pub fn run_parent_frame(
    scene: &scene_query::Scene,
    case: &mut ParentCase,
    frame: u16,
    out: Array<u16>,
    tables: &ParentTables,
) -> u16 {
    let control = life_at(case.controls, frame * 10);
    let action = attitude::get(control, 0);
    let limit = attitude::get(control, 3);
    case.state.lifetime.limit = limit;
    let result: Buf<u16, 2> = Buf::new([0; 2]);
    let ignored: Buf<u16, 2> = Buf::new([0; 2]);
    let velocity: Buf<u16, 6> = Buf::new([53392, 3, 46608, 65533, 41248, 7]);
    let row = life_at(case.state.lifetime.records, case.actor.fragments.id * 8);
    let entity = attitude::get(case.actor.fragments.collider, 0);
    let mut status = fix32::zero();
    if action == fix32::zero() {
        attitude::put(result.as_array(), 0, fix32::from_words(0x7182, 0));
        let call = scene_parent::Hit {
            method: if case.actor.kind == 1 { 1 } else { 82 },
            auxiliary: scene_fragments::Hit {
                row: row,
                limit: limit,
                powerup: false,
                name: 0,
                model_status: attitude::get(control, 4),
                model: case.model,
                has_terrain: true,
                vehicle: attitude::get(life_at(case.meta, 67), 0),
                velocity: velocity.as_array(),
                paired_tail: 0,
                has_paired_tail: false,
                result: ignored.as_array(),
            },
            result: result.as_array(),
            forward: attitude::get(life_at(case.meta, 3), 0),
            minimum_turn: attitude::get(life_at(case.meta, 5), 0),
            maximum_turn: attitude::get(life_at(case.meta, 7), 0),
            current: 0,
            mode: attitude::get(control, 2),
            dt: attitude::get(control, 1),
            sine: tables.sine,
        };
        status = scene_parent::hit(
            scene,
            &mut case.state,
            &mut case.grid,
            &mut case.actor,
            &call,
        );
    } else if action == integer::from_i16(1) {
        let call = scene_parent::Frame {
            auxiliary: scene_fragments::Frame {
                entity: entity,
                dt: attitude::get(control, 1),
                mode: attitude::get(control, 2),
                sine: tables.sine,
                result: ignored.as_array(),
            },
            result: result.as_array(),
            group: fix32::zero(),
            player: life_at(case.meta, 61),
            has_player: true,
            peer: life_at(case.meta, 43),
            has_peer: true,
            tuning: life_at(case.meta, 35),
            angle: tables.angle,
            inverse_cosine: tables.inverse,
        };
        status = scene_parent::tick(
            scene,
            &mut case.state,
            &mut case.grid,
            &mut case.actor,
            &call,
        );
    } else if action == integer::from_i16(2) {
        if row[7u16] & 32 != 0 {
            let saved = read_dseg_page(3);
            if case.actor.fragments.motion_page != 65535 {
                unsafe {
                    write_dseg_page(3, case.actor.fragments.motion_page);
                }
            }
            let mut i = 0u16;
            while i < case.actor.fragments.motion.count {
                attitude::put(
                    case.actor.fragments.motion.faces,
                    i * 8 + 6,
                    integer::from_i16(1),
                );
                i += 1;
            }
            unsafe {
                write_dseg_page(3, saved);
            }
        }
    } else if action == integer::from_i16(4) {
        scene_meshes::release_private(
            &mut case.state.resources,
            case.actor.fragments.private_mesh.lo_bits(),
        );
    } else if action == integer::from_i16(5) {
        case.actor.fragments.private_mesh = fix32::from_words(65534, 0);
    } else {
        status = scene_parent::destroy(
            scene,
            &mut case.state,
            &mut case.grid,
            &mut case.actor,
            entity,
        );
    }
    attitude::put(out, 0, status);
    let mut position = 2 + capture_lifetime_parts(
        life_at(out, 2),
        status,
        &case.state.lifetime,
        &case.state.runtime,
        &case.grid,
        case.actor.fragments.id,
    );
    attitude::put(out, 1, attitude::get(result.as_array(), 0));
    let mut i = 0u16;
    while i < 5 {
        let mut value = fix32::zero();
        if case.actor.has_state {
            value = attitude::get(case.actor.state, i);
        }
        attitude::put(life_at(out, position), i, value);
        i += 1;
    }
    position += 10;
    position = store_copy(out, case.actor.fragments.collider, position, 60);
    position = store_copy(out, case.actor.properties, position, 16);
    let mut output = out;
    output[position] = case.actor.fragments.active[0u16];
    position += 1;
    let mut current = integer::from_i16(-1);
    let mut private = integer::from_i16(-1);
    if row[7u16] & 1 != 0 {
        current = attitude::get(case.actor.fragments.current_mesh, 0);
    }
    if row[7u16] & 16 != 0 {
        private = case.actor.fragments.private_mesh;
    }
    attitude::put(life_at(out, position), 0, current);
    attitude::put(life_at(out, position), 1, private);
    position += 4;
    output[position] = 0;
    position += 1;
    if row[7u16] & 32 != 0 {
        let saved = read_dseg_page(3);
        if case.actor.fragments.motion_page != 65535 {
            unsafe {
                write_dseg_page(3, case.actor.fragments.motion_page);
            }
        }
        output[position - 1] = case.actor.fragments.motion.count;
        position = store_copy(out, case.actor.fragments.motion.header, position, 8);
        position = store_copy(
            out,
            case.actor.fragments.motion.faces,
            position,
            case.actor.fragments.motion.count * 16,
        );
        unsafe {
            write_dseg_page(3, saved);
        }
    }
    position + capture_mesh_store(life_at(out, position), status, &case.state.resources)
}
