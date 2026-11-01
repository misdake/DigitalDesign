// Append to the production scene-math fixture; no public root or enum methods.
mod gpu_api_fault;
mod gpu_arena;
mod gpu_execute;
mod gpu_mesh;
mod gpu_present;
mod gpu_ui_grid;
mod gpu_ui_packet;

static COMBO_IDS: Buf<u16, 6> = Buf::new([0; 6]);
static COMBO_MESH: Buf<u16, 16> = Buf::new([0; 16]);
static COMBO_DIRECTORY: Buf<u16, 2> = Buf::new([0xa002, 1]);
static COMBO_ATLAS: Buf<u16, 16> = Buf::new([0; 16]);
static COMBO_GRID: Buf<u16, 16> = Buf::new([0; 16]);
static COMBO_UI: Buf<u16, 16> = Buf::new([0; 16]);

fn combo_transform(x: i16) -> Transform {
    let zero = fix32::zero();
    Transform {
        position: Vec3 {
            x: fix32::from_int(10),
            y: fix32::from_int(20),
            z: fix32::from_int(-30),
        },
        angles: Vec3 {
            x: zero,
            y: zero,
            z: zero,
        },
        scale: Vec3 {
            x: fix32::from_int(x),
            y: fix32::from_int(4),
            z: fix32::from_int(8),
        },
    }
}
fn combo_set_desired(slot: u16) {
    check(gpu_bind_desired(slot, true, DESIRED.as_array()), 90);
    let state = unsafe { DESIRED.as_ptr().add(32).as_u16_array() };
    check(gpu_bind_desired(slot, false, state), 91);
}
fn combo_draw(slot: u16, model: u16) {
    let c = gpu_record_data(slot);
    let desired = unsafe { c.as_ptr().add(GPU_CB_CAMERA as i16).as_u16_array() };
    check(
        gpu_scene_draw_prepare(
            desired,
            gpu_arena_data(model),
            MATERIAL.as_array(),
            VIEW.as_array(),
            true,
            gpu_native_scratch(),
        ),
        92,
    );
    check(
        gpu_mesh_draw(slot, COMBO_MESH.as_array(), gpu_native_scratch()),
        93,
    );
}
fn main() {
    run_cases();
    gpu_record_init();
    check(gpu_arena_init(8192), 95);
    let mut n: u16 = 0;
    while n < 6 {
        COMBO_IDS[n] = gpu_arena_alloc(true, 2);
        check(COMBO_IDS[n] != 0, 96);
        check(
            gpu_scene_model_init(gpu_arena_data(COMBO_IDS[n]), combo_transform(2)),
            97,
        );
        n += 1;
    }
    check(gpu_record_configure(0, 0x9800, 8, 0x9a00, 16), 98);
    check(gpu_record_configure(1, 0xfc00, 4, 0xfe00, 16), 99);
    // Targets, texture, directory, UI, commands and push are disjoint.
    check(gpu_packet_begin(0, 0x4000, 0x6000, 0, 0, 0), 100);
    check(gpu_packet_begin(1, 0x5000, 0x6000, 0, 0, 0), 101);
    let memory: GpuMemory = GpuMemory {
        first_sector: 0xa000,
        sector_count: 16,
    };
    let mesh: MeshDesc = MeshDesc {
        directory_sector: 0xa000,
        directory_word: 0,
        meshlet_count: 1,
        storage: memory,
    };
    check(gpu_mesh_init(COMBO_MESH.as_array(), mesh), 102);
    check(
        gpu_window_write_words(0xa000, 0, COMBO_DIRECTORY.as_array(), 2),
        103,
    );
    model_initial();
    material_state();
    camera_a();
    combo_set_desired(0);
    combo_draw(0, COMBO_IDS[0u16]);
    camera_b();
    combo_set_desired(1);
    combo_draw(1, COMBO_IDS[0u16]);
    // A's desired snapshot survives later source changes and B recording.
    mutate_after_draw();
    material_state();
    combo_draw(0, COMBO_IDS[5u16]);
    gpu_scene_material_color(
        MATERIAL.as_array(),
        Rgba8 {
            r: 1,
            g: 2,
            b: 3,
            a: 4,
        },
    );
    combo_draw(0, COMBO_IDS[5u16]);
    let atlas: GpuMemory = GpuMemory {
        first_sector: 0xb000,
        sector_count: 2,
    };
    let grid: GpuMemory = GpuMemory {
        first_sector: 0xb006,
        sector_count: 6,
    };
    check(gpu_ui_atlas_init(COMBO_ATLAS.as_array(), atlas, 1), 104);
    check(
        gpu_ui_grid_init(COMBO_GRID.as_array(), grid, COMBO_ATLAS.as_array()),
        105,
    );
    check(
        gpu_ui_descriptor_prepare(COMBO_GRID.as_array(), COMBO_UI.as_array()),
        106,
    );
    check(gpu_ui_packet_draw(0, COMBO_UI.as_array(), 2, 1, 3, 2), 107);
    combo_draw(0, COMBO_IDS[5u16]);
    check(
        gpu_ui_packet_draw(0, COMBO_UI.as_array(), 49, 14, 1, 1),
        108,
    );
    check(gpu_arena_free(COMBO_IDS[0u16], true, 2), 109);
    let reused = gpu_arena_alloc(true, 2);
    check(reused == COMBO_IDS[0u16], 110);
    check(
        gpu_scene_model_init(gpu_arena_data(reused), combo_transform(4)),
        111,
    );
    combo_draw(0, reused);
    check(gpu_record_close_pass(0) && gpu_record_seal(0), 112);
    check(gpu_record_close_pass(1) && gpu_record_seal(1), 113);
    halt(0x600d);
}
