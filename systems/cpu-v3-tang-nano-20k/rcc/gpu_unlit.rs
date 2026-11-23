//! Immutable mesh directory and unlit material packets for reusable draws.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_record;
use gpu_record::*;
mod gpu_bind;
use gpu_bind::*;
mod gpu_mesh;
use gpu_mesh::*;
mod gpu_scene_math;
use gpu_scene_math::*;
mod gpu_target;
use gpu_target::*;

const BANK_KEY_COUNT: u16 = 16;
const BANK_FACT_COUNT: u16 = 17;
const BANK_FROZEN: u16 = 18;
const BANK_SCENE: u16 = 32;
const BANK_WORLD: u16 = 48;
const BANK_KEYS: u16 = BANK_WORLD + 2 * 64;
const BANK_KEY_LIMIT: u16 = 256;
const BANK_FACTS: u16 = BANK_KEYS + BANK_KEY_LIMIT * 16;
const BANK_DIRECTORY: u16 = BANK_FACTS + 64 * 4;
const BANK_VERTICAL: u16 = 512;

static GPU_UNLIT_OWNER: Buf<u16, 2> = Buf::new([0; 2]);
static GPU_UNLIT_EPOCH: Buf<u16, 4> = Buf::new([0; 4]);
const BANK_READY_WORDS: u16 = BANK_KEY_LIMIT / 16;
static GPU_UNLIT_READY: Buf<u16, 32> = Buf::new([0; 32]);
static GPU_UNLIT_LAST: Buf<u16, 2> = Buf::new([0; 2]);

fn gpu_unlit_array(data: Array<u16>, at: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(at as i16).as_u16_array() }
}
fn gpu_unlit_new(mesh: Array<u16>, first: u16, count: u16) -> Ptr {
    let data = malloc(BANK_DIRECTORY + count * 2);
    let mut bank = unsafe { data.as_u16_array() };
    gpu_scene_zero(bank, BANK_DIRECTORY);
    bank[GPU_MESH_COUNT] = count;
    if count != 0 {
        let entries = unsafe { Ptr::from_addr(mesh[GPU_MESH_DIRECTORY_CACHE]).as_u16_array() };
        gpu_bind_copy(bank, BANK_DIRECTORY, entries, first * 2, count * 2);
    }
    let mut facts = gpu_unlit_array(bank, BANK_FACTS);
    facts[0u16] = mesh[GPU_MESH_STORAGE];
    facts[1u16] = mesh[GPU_MESH_STORAGE_COUNT];
    facts[2u16] = ResourceFactKind::Mesh as u16;
    facts[3u16] = GPU_FACT_READ;
    bank[BANK_FACT_COUNT] = 1;
    data
}
fn gpu_unlit_add(
    mut bank: Array<u16>,
    state: Array<u16>,
    material: Array<u16>,
    view: Array<u16>,
    flags: u16,
) -> u16 {
    let textured = flags & 1 != 0;
    if bank[BANK_FROZEN] != 0 || material[GPU_MATERIAL_FLAGS] & 1 == 0 {
        halt(0x7b21);
    }
    let native = gpu_native_scratch();
    if !gpu_scene_material_prepare(state, material, view, textured, native) {
        halt(0x7b22);
    }
    if bank[BANK_KEY_COUNT] == 0 {
        gpu_bind_copy(bank, BANK_SCENE, state, 0, GPU_STATE_CONTROL);
    } else if !gpu_bind_equal(bank, BANK_SCENE, state, 0, GPU_STATE_CONTROL) {
        halt(0x7b23);
    }
    let mut key: u16 = 0;
    while key < bank[BANK_KEY_COUNT] {
        let at = BANK_KEYS + key * 16;
        if gpu_bind_equal(bank, at, native, GPU_SCRATCH_INSTANCE + 26, 4)
            && gpu_bind_equal(bank, at + 4, native, GPU_SCRATCH_RENDER, 12)
        {
            break;
        }
        key += 1;
    }
    if key == bank[BANK_KEY_COUNT] {
        if key == BANK_KEY_LIMIT {
            halt(0x7b24);
        }
        let at = BANK_KEYS + key * 16;
        gpu_bind_copy(bank, at, native, GPU_SCRATCH_INSTANCE + 26, 4);
        gpu_bind_copy(bank, at + 4, native, GPU_SCRATCH_RENDER, 12);
        bank[BANK_KEY_COUNT] += 1;
    }
    if textured {
        let mut facts = gpu_unlit_array(bank, BANK_FACTS);
        let mut i: u16 = 1;
        while i < bank[BANK_FACT_COUNT] {
            if facts[i * 4] == view[GPU_VIEW_BASE] && facts[i * 4 + 1] == view[GPU_VIEW_COUNT] {
                break;
            }
            i += 1;
        }
        if i == bank[BANK_FACT_COUNT] {
            if i == 64 {
                halt(0x7b26);
            }
            facts[i * 4] = view[GPU_VIEW_BASE];
            facts[i * 4 + 1] = view[GPU_VIEW_COUNT];
            facts[i * 4 + 2] = ResourceFactKind::Texture as u16;
            facts[i * 4 + 3] = GPU_FACT_READ;
            bank[BANK_FACT_COUNT] += 1;
        }
    }
    key + 1 + (flags & BANK_VERTICAL)
}

fn gpu_unlit_write_view(slot: u16, bank: Array<u16>, camera: Array<u16>) -> bool {
    let desired: Buf<u16, 48> = Buf::new([0; 48]);
    let d = desired.as_array();
    gpu_bind_copy(d, 0, camera, 0, 32);
    gpu_bind_copy(d, 32, bank, BANK_SCENE, GPU_STATE_CONTROL);
    let native = gpu_native_scratch();
    if !gpu_scene_encode_view_light(d, gpu_unlit_array(d, 32), native) {
        return false;
    }
    gpu_scene_pass_fields(d, native);
    gpu_scene_view_prepare(d, native);
    let mut c = gpu_record_data(slot);
    if !gpu_window_write_words(
        c[GPU_RECORD_PUSH_BASE],
        0,
        gpu_unlit_array(native, GPU_SCRATCH_VIEW),
        64,
    ) {
        c[GPU_RECORD_PHASE] = RecordPhase::Poisoned as u16;
        return false;
    }
    true
}
fn gpu_unlit_prepare(
    slot: u16,
    mut bank: Array<u16>,
    camera: Array<u16>,
    model: Array<u16>,
) -> bool {
    if !gpu_target_owned(slot) {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != RecordPhase::Ready as u16
        || c[GPU_RECORD_COMMAND_USED] != 0
        || c[GPU_RECORD_PUSH_USED] != 0
        || bank[BANK_KEY_COUNT] == 0
    {
        return false;
    }
    let prefix = (1 + bank[BANK_KEY_COUNT]) * 64;
    if prefix > c[GPU_RECORD_PUSH_CAPACITY] {
        return false;
    }
    let plan: Buf<u16, 64> = Buf::new([0; 64]);
    let facts = gpu_unlit_array(bank, BANK_FACTS);
    if !gpu_record_resource_room(slot, facts, bank[BANK_FACT_COUNT], plan.as_array())
        || !gpu_unlit_write_view(slot, bank, camera)
    {
        return false;
    }
    let native = gpu_native_scratch();
    gpu_scene_zero(gpu_unlit_array(native, GPU_SCRATCH_INSTANCE), 64);
    gpu_bind_world_prepare(model, native);
    gpu_bind_copy(
        bank,
        BANK_WORLD + slot * 64,
        native,
        GPU_SCRATCH_INSTANCE,
        64,
    );
    c[GPU_RECORD_PUSH_USED] = prefix;
    bank[BANK_FROZEN] = 1;
    GPU_UNLIT_OWNER[slot] = bank.as_ptr().addr();
    GPU_UNLIT_EPOCH[slot * 2] = c[GPU_RECORD_GENERATION_LOW];
    GPU_UNLIT_EPOCH[slot * 2 + 1] = c[GPU_RECORD_GENERATION_HIGH];
    let mut i: u16 = 0;
    while i < BANK_READY_WORDS {
        GPU_UNLIT_READY[slot * BANK_READY_WORDS + i] = 0;
        i += 1;
    }
    GPU_UNLIT_LAST[slot] = 0;
    gpu_record_resource_commit(slot, facts, bank[BANK_FACT_COUNT], plan.as_array());
    true
}

fn gpu_unlit_matches(slot: u16, bank: Array<u16>) -> bool {
    if slot >= 2 || GPU_UNLIT_OWNER[slot] != bank.as_ptr().addr() {
        return false;
    }
    let c = gpu_record_data(slot);
    GPU_UNLIT_EPOCH[slot * 2] == c[GPU_RECORD_GENERATION_LOW]
        && GPU_UNLIT_EPOCH[slot * 2 + 1] == c[GPU_RECORD_GENERATION_HIGH]
}

fn gpu_unlit_end(slot: u16) {
    if slot < 2 {
        GPU_UNLIT_OWNER[slot] = 0;
    }
}

fn gpu_unlit_draw(slot: u16, bank: Array<u16>, binding: u16, first: u16, total: u16) -> bool {
    if !gpu_unlit_matches(slot, bank)
        || first > bank[GPU_MESH_COUNT]
        || total > bank[GPU_MESH_COUNT] - first
    {
        return false;
    }
    let mut c = gpu_record_data(slot);
    let id = binding & (BANK_VERTICAL - 1);
    if c[GPU_RECORD_PHASE] != RecordPhase::PassOpen as u16
        || id == 0
        || id > bank[BANK_KEY_COUNT]
        || binding >= 2 * BANK_VERTICAL
    {
        return false;
    }
    if total == 0 {
        return true;
    }
    let key = BANK_KEYS + (id - 1) * 16;
    let last = GPU_UNLIT_LAST[slot];
    let previous = last & (BANK_VERTICAL - 1);
    let mut changes: u16 = 0;
    if last == 0 {
        changes = 1 | 2 | 4 | GPU_BIND_PLAN_VIEW;
    } else if previous != id {
        changes = 4;
        let prior = BANK_KEYS + (previous - 1) * 16;
        // Uniform tint goes directly to DRAW. Compare only the CP's Render words.
        if !gpu_bind_equal(bank, prior + 4, bank, key + 4, 8)
            || !gpu_bind_equal(bank, prior + 14, bank, key + 14, 2)
        {
            changes |= 2;
        }
        if binding & BANK_VERTICAL == 0 && !gpu_bind_equal(bank, prior, bank, key, 4) {
            changes |= 8;
        }
    }
    // A billboard can skip the normal/light response write in Geometry.
    if last & BANK_VERTICAL != 0 && binding & BANK_VERTICAL == 0 {
        changes |= 8;
    }
    let mode = if binding & BANK_VERTICAL != 0 {
        3u16
    } else {
        1u16
    };
    let world = id * 64;
    let plan: Buf<u16, 7> = Buf::new([changes, 0, 0, world, c[GPU_RECORD_PUSH_USED], 0, mode]);
    let native = gpu_native_scratch();
    let code = gpu_unlit_array(native, GPU_SCRATCH_COMMAND);
    let words = gpu_bind_code(slot, plan.as_array(), code);
    if total > (32768 - words - 6) / 6
        || !gpu_record_preflight(slot, words + total * 6, c[GPU_RECORD_PUSH_USED])
    {
        return false;
    }
    let ready = slot * BANK_READY_WORDS + ((id - 1) >> 4);
    let bit = 1u16 << ((id - 1) & 15);
    if GPU_UNLIT_READY[ready] & bit == 0 {
        let staging = gpu_unlit_array(native, GPU_SCRATCH_INSTANCE);
        gpu_bind_copy(staging, 0, bank, BANK_WORLD + slot * 64, 64);
        gpu_bind_copy(staging, 26, bank, key, 4);
        gpu_bind_copy(staging, 52, bank, key + 4, 12);
        if !gpu_window_write_words(c[GPU_RECORD_PUSH_BASE], world, staging, 64) {
            c[GPU_RECORD_PHASE] = RecordPhase::Poisoned as u16;
            return false;
        }
        GPU_UNLIT_READY[ready] |= bit;
    }
    let entries = gpu_unlit_array(bank, BANK_DIRECTORY + first * 2);
    if !gpu_mesh_emit(slot, native, plan.as_array(), entries, total) {
        return false;
    }
    GPU_UNLIT_LAST[slot] = binding;
    c[GPU_RECORD_UI_CLEAN] = 0;
    true
}

fn gpu_unlit_patch(
    slot: u16,
    bank: Array<u16>,
    camera: Array<u16>,
    color: Array<u16>,
    depth: Array<u16>,
) -> bool {
    if !gpu_unlit_matches(slot, bank)
        || GPU_UNLIT_LAST[slot] == 0
        || !gpu_target_binding(slot)
        || !gpu_target_targets(slot, color, depth)
    {
        return false;
    }
    // The existing submit path enforces GPU and display leases. Only the
    // CPU-owned View and Target snapshots are modified here.
    gpu_unlit_write_view(slot, bank, camera) && gpu_target_write(slot, color, depth)
}
