//! Trusted native uniform20 packages, with a CPU-expanded immutable directory.
use crate::dsl_rt::*;
mod gpu_bind;
use gpu_bind::*;

#[derive(Copy, Clone)]
pub struct MeshDesc {
    pub directory_sector: u16,
    pub directory_word: u16,
    pub meshlet_count: u16,
    pub storage: GpuMemory,
}
impl MeshDesc {
    /// Select native directory entries before registering an owning Mesh.
    pub const fn subset(self, first: u16, count: u16) -> Self {
        assert!(self.directory_word < 64 && self.meshlet_count <= 5460);
        assert!(first <= self.meshlet_count && count <= self.meshlet_count - first);
        let mut result = self;
        result.meshlet_count = count;
        if count != 0 {
            let word = self.directory_word + first * 2;
            assert!((word >> 6) <= 65535 - self.directory_sector);
            result.directory_sector = self.directory_sector + (word >> 6);
            result.directory_word = word & 63;
        }
        result
    }
}
// Directory descriptors are captured and validated at registration/refresh.
// Vertex/index payload bytes remain external and may change after retirement.
fn gpu_mesh_refresh_directory(mut mesh: Array<u16>) -> bool {
    let total = mesh[GPU_MESH_COUNT];
    let mut owner = Ptr::from_addr(0);
    if total != 0 {
        owner = try_malloc(total * 2);
        if owner.addr() == 0 {
            return false;
        }
    }
    let mut first: u16 = 0;
    while first < total {
        let mut count = total - first;
        if count > 64 {
            count = 64;
        }
        let at = unsafe { owner.add((first * 2) as i16).as_u16_array() };
        if !gpu_window_read_words(
            mesh[GPU_MESH_DIRECTORY],
            mesh[GPU_MESH_DIRECTORY_WORD] + first * 2,
            at,
            count * 2,
        ) || !gpu_mesh_entries_valid(mesh, at, count)
        {
            free(owner);
            return false;
        }
        first += count;
    }
    let old = mesh[GPU_MESH_DIRECTORY_CACHE];
    mesh[GPU_MESH_DIRECTORY_CACHE] = owner.addr();
    if old != 0 {
        free(Ptr::from_addr(old));
    }
    true
}
fn gpu_mesh_release_directory(mesh: Array<u16>) {
    let owner = mesh[GPU_MESH_DIRECTORY_CACHE];
    if owner != 0 {
        free(Ptr::from_addr(owner));
    }
}
fn gpu_mesh_init(mut out: Array<u16>, desc: MeshDesc) -> bool {
    let memory: GpuMemory = desc.storage;
    let first = memory.first_sector();
    let sectors = memory.sector_count();
    if desc.directory_word >= 64
        || desc.meshlet_count > 5460
        || desc.directory_sector < first
        || desc.directory_sector - first >= sectors
    {
        return false;
    }
    if desc.meshlet_count != 0 {
        let last = (desc.directory_word + desc.meshlet_count * 2 - 1) >> 6;
        if last >= sectors - (desc.directory_sector - first) {
            return false;
        }
    }
    let mut i: u16 = 0;
    while i < 16 {
        out[i] = 0;
        i += 1;
    }
    out[GPU_MESH_DIRECTORY] = desc.directory_sector;
    out[GPU_MESH_DIRECTORY_WORD] = desc.directory_word;
    out[GPU_MESH_COUNT] = desc.meshlet_count;
    out[GPU_MESH_STORAGE] = first;
    out[GPU_MESH_STORAGE_COUNT] = sectors;
    gpu_mesh_refresh_directory(out)
}
fn gpu_mesh_subset(mut out: Array<u16>, source: Array<u16>, first: u16, count: u16) -> bool {
    let available = source[GPU_MESH_COUNT];
    if first > available || count > available - first {
        return false;
    }
    let mut owner = Ptr::from_addr(0);
    if count != 0 {
        owner = try_malloc(count * 2);
        if owner.addr() == 0 {
            return false;
        }
    }
    let entries = unsafe { owner.as_u16_array() };
    let input = unsafe { Ptr::from_addr(source[GPU_MESH_DIRECTORY_CACHE]).as_u16_array() };
    if count != 0 {
        gpu_bind_copy(entries, 0, input, first * 2, count * 2);
    }
    gpu_bind_copy(out, 0, source, 0, 16);
    out[GPU_MESH_COUNT] = count;
    out[GPU_MESH_DIRECTORY_CACHE] = owner.addr();
    if count != 0 {
        let word = source[GPU_MESH_DIRECTORY_WORD] + first * 2;
        out[GPU_MESH_DIRECTORY] = source[GPU_MESH_DIRECTORY] + (word >> 6);
        out[GPU_MESH_DIRECTORY_WORD] = word & 63;
    }
    true
}

fn gpu_mesh_entries_valid(mesh: Array<u16>, entries: Array<u16>, count: u16) -> bool {
    let first = mesh[GPU_MESH_STORAGE];
    let sectors = mesh[GPU_MESH_STORAGE_COUNT];
    let directory = mesh[GPU_MESH_DIRECTORY];
    let directory_sectors = (mesh[GPU_MESH_DIRECTORY_WORD] + mesh[GPU_MESH_COUNT] * 2 + 63) >> 6;
    let mut i: u16 = 0;
    while i < count {
        let record = entries[i * 2];
        let payload = entries[i * 2 + 1];
        if record < first || payload == 0 || payload > 8 {
            return false;
        }
        let offset = record - first;
        if offset >= sectors
            || payload >= sectors - offset
            || gpu_record_overlap(record, payload + 1, directory, directory_sectors)
        {
            return false;
        }
        i += 1;
    }
    true
}
fn gpu_mesh_draw(slot: u16, mesh: Array<u16>, model: Array<u16>, native: Array<u16>) -> bool {
    gpu_mesh_draw_range(slot, mesh, model, native, 0, mesh[GPU_MESH_COUNT])
}
fn gpu_mesh_draw_range(
    slot: u16,
    mesh: Array<u16>,
    model: Array<u16>,
    native: Array<u16>,
    first: u16,
    total: u16,
) -> bool {
    if slot >= GPU_RECORD_COUNT {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != RecordPhase::PassOpen as u16 {
        return false;
    }
    if first > mesh[GPU_MESH_COUNT] || total > mesh[GPU_MESH_COUNT] - first {
        return false;
    }
    if total == 0 {
        return true;
    }
    let plan: Buf<u16, 7> = Buf::new([0; 7]);
    if !gpu_bind_plan(slot, native, model, plan.as_array())
        || total > (32768 - plan[GPU_BIND_PLAN_CODE_WORDS] - 6) / 6
        || !gpu_record_preflight(
            slot,
            plan[GPU_BIND_PLAN_CODE_WORDS] + total * 6,
            plan[GPU_BIND_PLAN_PUSH_END],
        )
    {
        return false;
    }
    let entries = unsafe {
        Ptr::from_addr(mesh[GPU_MESH_DIRECTORY_CACHE])
            .add((first * 2) as i16)
            .as_u16_array()
    };
    if !gpu_bind_payload(slot, native, plan.as_array())
        || !gpu_mesh_emit(slot, native, plan.as_array(), entries, total)
    {
        return false;
    }
    gpu_bind_commit(slot, native, plan.as_array());
    c[GPU_RECORD_UI_CLEAN] = 0;
    true
}

fn gpu_mesh_emit(
    slot: u16,
    native: Array<u16>,
    plan: Array<u16>,
    entries: Array<u16>,
    total: u16,
) -> bool {
    let mut code = unsafe {
        native
            .as_ptr()
            .add(GPU_SCRATCH_COMMAND as i16)
            .as_u16_array()
    };
    let mut at = gpu_bind_code(slot, plan, code);
    let mut index: u16 = 0;
    while index < total {
        // Leave two words for the final DRAW after the last batch.
        if at + 8 > GPU_API_STORAGE_CHUNK_WORDS {
            if !gpu_record_append_reserved(slot, code, at, code, 0, plan[GPU_BIND_PLAN_PUSH_END]) {
                return false;
            }
            at = 0;
        }
        let record = entries[index * 2];
        let payload = entries[index * 2 + 1];
        code[at] = (record << 12) | ((payload - 1) << 1) | (index & 1);
        code[at + 1] = 0x2000 | (record >> 4);
        at += 2;
        if index != 0 {
            code[at] = ((index - 1) & 1) | (plan[GPU_BIND_PLAN_DRAW_MODE] << 1);
            code[at + 1] = 0x3000;
            at += 2;
        }
        code[at] = 1;
        code[at + 1] = 0x4000;
        at += 2;
        index += 1;
    }
    code[at] = ((total - 1) & 1) | (plan[GPU_BIND_PLAN_DRAW_MODE] << 1);
    code[at + 1] = 0x3000;
    if !gpu_record_append_reserved(slot, code, at + 2, code, 0, plan[GPU_BIND_PLAN_PUSH_END]) {
        return false;
    }
    true
}
