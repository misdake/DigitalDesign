//! Direct views of the same car damage, prefix, geometry and effect owners.
#![link_cseg = "world"]
use super::*;
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
fn part(
    player: &player_world::Player,
    car: &driving::Car,
    index: u16,
    upper: u16,
) -> tank_hit::Part {
    let model = at(player.hit_model.as_array(), (index * 7) as i16);
    let geometry = player_world::combat(index);
    let body = at(player.body, (index * DRIVING_MESH_WORDS) as i16);
    let revision = at(
        player.hit_fields.as_array(),
        (index * 24 + 12 + upper * 4) as i16,
    );
    let mut selected = player.body_selection.as_array();
    let mut pose = at(car.attitude, 18);
    let mut mesh = damage::Mesh {
        present: model[1u16] != 0,
        page: player.weapons.effect_page,
        vertices: unsafe { body.as_ptr().add(4).as_i16_array() },
        vertex_count: DRIVING_BODY_VERTICES,
        indices: at(body, (4 + DRIVING_BODY_VERTICES * 3) as i16),
        normals: unsafe {
            body.as_ptr()
                .add((4 + DRIVING_BODY_VERTICES * 3 + DRIVING_BODY_FACES * 3) as i16)
                .as_i16_array()
        },
        face_count: DRIVING_BODY_FACES,
    };
    if upper != 0 {
        selected = geometry;
        pose = ptr(0xc000 + car_runtime::CAR_WORD + index * car_runtime::CAR_WORDS);
        mesh = damage::Mesh {
            present: model[3u16] != 0,
            page: player.weapons.effect_page,
            vertices: unsafe { geometry.as_ptr().add(4).as_i16_array() },
            vertex_count: model[4u16],
            indices: at(geometry, (4 + model[4u16] * 3) as i16),
            normals: unsafe {
                geometry
                    .as_ptr()
                    .add((4 + model[4u16] * 3 + model[5u16] * 3) as i16)
                    .as_i16_array()
            },
            face_count: model[5u16],
        };
    }
    tank_hit::Part {
        present: model[upper * 2] != 0,
        revision,
        target: damage::Target {
            state: at(player.auxiliary, (index * 36 + 16) as i16),
            player: car.player,
            basis: pose,
            position: at(pose, 18),
            selected,
            mesh,
            revision,
            has_revision: model[upper * 2] != 0,
        },
    }
}
fn car(
    player: &player_world::Player,
    car: &driving::Car,
    parts: Array<tank_hit::Part>,
    index: u16,
) -> scene_projectile_impact::HitVehicle {
    let effects = ptr(0xc000 + car_runtime::CAR_WORD + index * car_runtime::CAR_WORDS);
    let prefix = at(player.prefixes, (index * 50) as i16);
    let fields = at(player.hit_fields.as_array(), (index * 24) as i16);
    let external = attitude::get(effects, 459);
    let mut basis = car.attitude;
    if external == integer::from_i16(2) {
        basis = at(effects, 920);
    }
    scene_projectile_impact::HitVehicle {
        present: true,
        car: tank_hit::Vehicle {
            player: car.player,
            visible: car.visible,
            inventory: at(player.inventory, (index * 4) as i16),
            timer: at(car.suspension, 66),
            velocity: car.motion,
            angular: at(car.attitude, 42),
            external: basis,
            has_external: external != integer::from_i16(0),
            collision_size: at(player.auxiliary, (index * 36 + 32) as i16),
            hit_counts: fields,
            smoke: at(effects, 488),
            smoke_stride: 2,
            parts,
            base_position: at(car.motion, 18),
            grip: at(prefix, 16),
            drive_grip: at(fields, 20),
            temporary: at(prefix, 8),
        },
    }
}
pub fn apply(
    state: &scene_parent::State,
    player: &player_world::Player,
    impact: &scene_projectile_impact::Impact,
    cup: u16,
    tuning_offset: u16,
) -> fix32 {
    if !impact.action[0u16].has_contact {
        return integer::from_i16(0);
    }
    let tuning: Buf<u16, 25> = Buf::new([0; 25]);
    read(
        tuning.as_array(),
        player.context,
        fix32::from_words(tuning_offset, 0),
        25,
    );
    let parts: Buf<tank_hit::Part, 6> = Buf::new([
        part(player, &state.cars[0u16], 0, 0),
        part(player, &state.cars[0u16], 0, 1),
        part(player, &state.cars[1u16], 1, 0),
        part(player, &state.cars[1u16], 1, 1),
        part(player, &state.cars[2u16], 2, 0),
        part(player, &state.cars[2u16], 2, 1),
    ]);
    let mut rows = parts.as_array();
    let upper = unsafe {
        view_of(&mut rows[2u16])
            .as_ptr()
            .as_array::<tank_hit::Part>()
    };
    let cars: Buf<scene_projectile_impact::HitVehicle, 3> = Buf::new([
        car(player, &state.cars[0u16], parts.as_array(), 0),
        car(player, &state.cars[1u16], upper, 1),
        car(
            player,
            &state.cars[2u16],
            unsafe {
                view_of(&mut rows[4u16])
                    .as_ptr()
                    .as_array::<tank_hit::Part>()
            },
            2,
        ),
    ]);
    let damage = scene_projectile_impact::Damage {
        cars: cars.as_array(),
        count: state.car_count,
        shell: tuning.as_array(),
        homer: at(tuning.as_array(), 17),
        cup,
        temporary: temporary::State {
            core: ptr(0xc000 + 9896),
            rows: ptr(0xc000 + 9978),
        },
        effect_page: 65535,
        present: true,
    };
    scene_projectile_impact::direct(&state.runtime, &player.weapons, impact, &damage)
}
