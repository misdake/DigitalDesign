//! Trusted native uniform20 packages, with a CPU-expanded immutable directory.
use crate::dsl_rt::*;
mod gpu_bind;
use gpu_bind::*;

pub struct MeshDesc {
    pub directory_sector: u16,
    pub directory_word: u16,
    pub meshlet_count: u16,
    pub storage: GpuMemory,
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
    true
}
fn gpu_mesh_subset(mut out: Array<u16>, source: Array<u16>, first: u16, count: u16) -> bool {
    let available = source[GPU_MESH_COUNT];
    if first > available || count > available - first {
        return false;
    }
    gpu_bind_copy(out, 0, source, 0, 16);
    out[GPU_MESH_COUNT] = count;
    if count != 0 {
        let word = source[GPU_MESH_DIRECTORY_WORD] + first * 2;
        out[GPU_MESH_DIRECTORY] = source[GPU_MESH_DIRECTORY] + (word >> 6);
        out[GPU_MESH_DIRECTORY_WORD] = word & 63;
    }
    true
}
fn gpu_mesh_batch(mesh: Array<u16>, first: u16, count: u16, words: Array<u16>) -> bool {
    gpu_window_read_words(
        mesh[GPU_MESH_DIRECTORY],
        mesh[GPU_MESH_DIRECTORY_WORD] + first * 2,
        words,
        count * 2,
    )
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
fn gpu_mesh_draw(slot: u16, mesh: Array<u16>, native: Array<u16>) -> bool {
    if slot >= 2 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_PASS {
        return false;
    }
    let total = mesh[GPU_MESH_COUNT];
    if total == 0 {
        return true;
    }
    let plan: Buf<u16, 6> = Buf::new([0; 6]);
    if !gpu_bind_plan(slot, native, plan.as_array())
        || total > (32768 - plan[5u16] - 6) / 6
        || !gpu_record_preflight(slot, plan[5u16] + total * 6, plan[4u16])
    {
        return false;
    }
    let entries = unsafe { native.as_ptr().add(144).as_u16_array() };
    let mut code = unsafe { native.as_ptr().add(160).as_u16_array() };
    // Whole-operation validation precedes every backing or cache modification.
    let mut first: u16 = 0;
    while first < total {
        let mut count = total - first;
        if count > 8 {
            count = 8;
        }
        if !gpu_mesh_batch(mesh, first, count, entries)
            || !gpu_mesh_entries_valid(mesh, entries, count)
        {
            return false;
        }
        first += count;
    }
    if !gpu_bind_emit(slot, native, plan.as_array()) {
        return false;
    }
    first = 0;
    while first < total {
        let mut count = total - first;
        if count > 8 {
            count = 8;
        }
        if !gpu_mesh_batch(mesh, first, count, entries) {
            c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
            return false;
        }
        let mut i: u16 = 0;
        let mut at: u16 = 0;
        while i < count {
            let index = first + i;
            let record = entries[i * 2];
            let payload = entries[i * 2 + 1];
            code[at] = (record << 12) | ((payload - 1) << 1) | (index & 1);
            code[at + 1] = 0x2000 | (record >> 4);
            at += 2;
            if index != 0 {
                code[at] = (index - 1) & 1;
                code[at + 1] = 0x3000;
                at += 2;
            }
            code[at] = 1;
            code[at + 1] = 0x4000;
            at += 2;
            i += 1;
        }
        if !gpu_record_append(slot, code, at, code, 0, 1) {
            return false;
        }
        first += count;
    }
    code[0u16] = (total - 1) & 1;
    code[1u16] = 0x3000;
    if !gpu_record_append(slot, code, 2, code, 0, 1) {
        return false;
    }
    c[GPU_RECORD_UI_CLEAN] = 0;
    true
}
