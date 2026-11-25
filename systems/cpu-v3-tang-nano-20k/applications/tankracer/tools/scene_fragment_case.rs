// Shared qualification views; all live words belong to the caller's buffers.
pub struct SceneFragmentCase {
    pub life: SceneCase,
    pub object: scene_fragments::Object,
    pub extra: Array<u16>,
    pub controls: Array<u16>,
    pub count: u16,
    pub entity: fix32,
}
pub fn load_scene_fragment_case(
    input: Array<u16>,
    storage: Array<u16>,
    scratch: Array<u16>,
) -> SceneFragmentCase {
    let life = load_scene_lifecycle(input, storage);
    let extra = life_at(input, life.tail * 2);
    let fragments = load_mesh_break(life_at(extra, 76), scratch);
    let object = scene_fragments::Object {
        id: life.object,
        collider: life_at(extra, 14),
        active: life_at(extra, 4),
        current_mesh: life_at(extra, 6),
        private_mesh: attitude::get(extra, 3),
        motion_capacity: fragments.state.count,
        motion_page: 65535,
        motion: fragments.state,
        mesh: fragments.mesh,
        config: fragments.config,
        version: life_at(extra, 8),
        resource_present: true,
        geometry_present: attitude::get(extra, 6) != fix32::zero(),
    };
    let controls = life_at(extra, (38 + attitude::get(extra, 37).lo_bits()) * 2);
    let count = attitude::get(controls, 0).lo_bits();
    SceneFragmentCase {
        life: life,
        object: object,
        extra: extra,
        controls: controls,
        count: count,
        entity: attitude::get(input, 6),
    }
}
pub fn run_scene_fragment_step(
    scene: &scene_query::Scene,
    case: &mut SceneFragmentCase,
    frame: u16,
    out: Array<u16>,
    sine: Array<i16>,
) -> u16 {
    attitude::put(life_at(case.extra, 2), 0, fix32::zero());
    let step = scene_fragments::Frame {
        entity: case.entity,
        dt: attitude::get(case.controls, 1 + frame * 2),
        mode: attitude::get(case.controls, 2 + frame * 2),
        sine: sine,
        result: life_at(case.extra, 2),
    };
    let status = scene_fragments::tick(
        scene,
        &mut case.life.store,
        &mut case.life.runtime,
        &mut case.life.grid,
        &case.object,
        &step,
    );
    let saved = read_dseg_page(3);
    if case.object.motion_page != 65535 {
        unsafe {
            write_dseg_page(3, case.object.motion_page);
        }
    }
    let mut position = capture_scene_lifecycle(out, status, &case.life) / 2;
    attitude::put(out, position, attitude::get(case.extra, 1));
    attitude::put(
        out,
        position + 1,
        integer::from_i16(case.object.active[0u16] as i16),
    );
    attitude::put(
        out,
        position + 2,
        attitude::get(case.object.current_mesh, 0),
    );
    attitude::put(out, position + 3, attitude::get(case.object.version, 0));
    attitude::put(out, position + 4, attitude::get(case.object.version, 1));
    position += 5;
    let mut i = 0u16;
    while i < 30 {
        attitude::put(out, position + i, attitude::get(case.object.collider, i));
        i += 1;
    }
    position += 30;
    let words = capture_mesh_break(
        life_at(out, position * 2),
        status,
        attitude::get(case.life.runtime.seed, 0),
        &case.object.motion,
        &case.object.mesh,
    );
    unsafe {
        write_dseg_page(3, saved);
    }
    position * 2 + words
}
