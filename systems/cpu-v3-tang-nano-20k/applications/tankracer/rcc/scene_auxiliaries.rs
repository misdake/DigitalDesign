//! Constructed parent views into stable level state and the shared motion bank.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, integer, lifecycle, mesh_break, scene_creation, scene_fragments,
    scene_lifecycle, scene_meshes, scene_parent, scene_query, scene_world, scenery_contact,
    shatter,
};

fn at(data: Array<u16>, offset: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(offset as i16).as_u16_array() }
}
pub fn drain(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    bindings: Array<u16>,
    weapons: &mut scene_creation::Weapons,
) -> fix32 {
    let mut pass = lifecycle::begin_drain(state.lifetime.limit);
    loop {
        let event = lifecycle::pop(&mut state.runtime.queues, &mut pass, state.lifetime.limit);
        if event.status == 0 {
            break;
        }
        if event.status != 1 {
            return integer::from_i16(-1);
        }
        let mut weapon = false;
        if entities::handle_kind(&state.lifetime.identities, event.id) == 1 {
            let id = entities::handle(&state.lifetime.identities, event.id);
            weapon = id.hi_bits() == 0
                && id.lo_bits() < weapons.object_capacity
                && scene_creation::component_kind(weapons, id.lo_bits()) != 0;
        }
        if weapon {
            let status = scene_creation::dispatch(
                scene,
                &mut state.lifetime,
                &mut state.runtime,
                world,
                weapons,
                &scene_creation::Call {
                    entity: event.id,
                    slot: 5,
                },
            );
            if status != fix32::zero() {
                return status;
            }
            if event.destroy && entities::present(&state.lifetime.identities, event.id) {
                let mut slot = 2u16;
                while slot < 4 {
                    let status = scene_creation::dispatch(
                        scene,
                        &mut state.lifetime,
                        &mut state.runtime,
                        world,
                        weapons,
                        &scene_creation::Call {
                            entity: event.id,
                            slot,
                        },
                    );
                    if status != fix32::zero() {
                        return status;
                    }
                    slot += 1;
                }
                entities::finish_destroy(&state.lifetime.identities);
            }
            continue;
        }
        let status = scene_lifecycle::stop(
            scene,
            &mut state.lifetime,
            &mut state.runtime,
            &mut world.grid,
            event.id,
        );
        if status != fix32::zero() {
            return status;
        }
        if event.destroy {
            let mut selected = 65535u16;
            if entities::handle_kind(&state.lifetime.identities, event.id) == 1 {
                let id = entities::handle(&state.lifetime.identities, event.id);
                if id.hi_bits() == 0 {
                    selected = find(bindings, id.lo_bits());
                }
            }
            let mut status = fix32::zero();
            if selected == 65535 {
                status = scene_lifecycle::destroy(
                    scene,
                    &mut state.lifetime,
                    &mut state.runtime,
                    &mut world.grid,
                    event.id,
                );
            } else {
                let mut parents = state.actors;
                status = scene_parent::destroy(
                    scene,
                    state,
                    &mut world.grid,
                    &mut parents[selected],
                    event.id,
                );
            }
            if status != fix32::zero() {
                return status;
            }
        }
    }
    fix32::zero()
}
pub fn find(data: Array<u16>, object: u16) -> u16 {
    let mut i = 0u16;
    while i < data[2u16] {
        if data[8 + i * 26] == object {
            return i;
        }
        i += 1;
    }
    65535
}
pub fn actor(
    row: Array<u16>,
    components: &scenery_contact::Components,
    world: &scene_world::World,
    resources: &scene_meshes::Store,
    page: u16,
) -> scene_parent::Actor {
    let id = row[0u16];
    let component = at(components.records, id * 8);
    let empty = scene_meshes::view(resources, 65535, false);
    let empty_debris: Buf<shatter::State, 0> = Buf::new([]);
    scene_parent::Actor {
        fragments: scene_fragments::Object {
            id: id,
            collider: at(world.colliders, id * 60),
            active: at(row, 10),
            current_mesh: at(row, 6),
            private_mesh: attitude::get(at(row, 8), 0),
            motion: scene_fragments::motion(row[5u16], row[11u16]),
            motion_capacity: row[4u16],
            motion_page: page,
            index_page: 65535,
            mesh: empty.mesh,
            config: mesh_break::Config {
                gravity: attitude::get(at(row, 12), 0),
                gains: at(row, 14),
            },
            version: empty.revision,
            resource_present: false,
            geometry_present: false,
        },
        properties: at(world.properties, id * 16),
        state: at(components.state, component[1u16]),
        kind: row[1u16],
        has_state: component[0u16] == 3 || component[0u16] == 4,
        reference_alias: row[24u16] != 0,
        debris: empty_debris.as_array(),
        attached: unsafe { Ptr::from_addr(0).as_u16_array() },
        attached_count: 0,
        definition: row,
    }
}
