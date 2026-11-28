//! Stable resource slots, first-name bindings and explicit current-geometry copies.
use crate::dsl_rt::*;
use crate::static_assets::AssetBytes;
use crate::{attitude, gpu_window, integer, mesh_break, mesh_storage};

pub struct Store {
    // Slot word 0: present/shared in bits 0..1; (index page + 1) above it.
    // A zero page tag keeps caller-owned CPU indices.
    // Thirteen words per slot: flags, hint, radius, material key, revision[4],
    // geometry words, center lengths[3], center stride. Flags: present=1, shared=2.
    pub records: Array<u16>,
    // One addressable immutable geometry root per slot. Its indices and face
    // attributes are shared; the arena owns vertices, flags and center columns.
    pub sources: Array<u16>,
    // Stable payload and index addresses per slot; mutable spans use the CPU
    // heap or the configured bank. Zero represents an empty span.
    pub data: Array<u16>,
    pub pages: Array<u16>,
    // New payloads use this optional external region. Existing CPU payloads
    // keep their own page tag and remain directly addressable.
    pub arena_page: u16,
    pub arena_start: u16,
    pub arena_words: u16,
    pub indices: Array<u16>,
    pub cold: AssetBytes,
    pub slot_words: u16,
    pub count: u16,
    pub capacity: u16,
    pub free: Array<u16>,
    pub free_count: u16,
    // [normalized name key, slot] pairs. Keys are resolved by the asset loader.
    pub names: Array<u16>,
    pub name_count: u16,
    pub name_capacity: u16,
}
#[derive(Copy, Clone)]
pub struct Model {
    // Counts root. RAM models retain their full original geometry here;
    // catalog sources use a six-word descriptor with a cold packet location.
    pub geometry: Array<u16>,
    pub geometry_words: u16,
    // Catalog-backed columns can be read directly into their final allocation.
    pub cold: bool,
    pub page: u16,
    pub index_page: u16,
    pub vertices: Array<i16>,
    pub indices: Array<u16>,
    pub flags: Array<u16>,
    pub centers: Array<u16>,
    pub lengths: Array<u16>,
    pub center_stride: u16,
    pub hint: u16,
    pub radius: u16,
    // Immutable material-table identity; zero is the empty table.
    pub materials: u16,
}
pub struct View {
    pub mesh: mesh_break::Mesh,
    pub revision: Array<u16>,
    pub valid: bool,
    pub page: u16,
    pub index_page: u16,
}
pub fn empty_model() -> Model {
    let words = unsafe { Ptr::from_addr(0).as_u16_array() };
    Model {
        geometry: words,
        geometry_words: 0,
        cold: false,
        page: 65535,
        index_page: 65535,
        vertices: unsafe { Ptr::from_addr(0).as_i16_array() },
        indices: words,
        flags: words,
        centers: words,
        lengths: words,
        center_stride: 0,
        hint: 0,
        radius: 0,
        materials: 0,
    }
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn at(words: Array<u16>, offset: u16) -> Array<u16> {
    unsafe { words.as_ptr().add(offset as i16).as_u16_array() }
}
fn exists(store: &Store, id: u16) -> bool {
    id < store.count && at(store.records, id * 13)[0u16] & 1 != 0
}
fn touch(mut revision: Array<u16>) {
    let mut i = 0u16;
    while i < 4 {
        revision[i] += 1;
        if revision[i] != 0 {
            return;
        }
        i += 1;
    }
}
// A resolved external view is usable while this selection remains resident.
// Caller restores the returned page after consuming that view.
pub fn select(store: &Store, id: u16) -> u16 {
    let saved = read_dseg_page(3);
    if id < store.count && store.data[id] != 0 && store.pages[id] != 65535 {
        unsafe {
            write_dseg_page(3, store.pages[id]);
        }
    }
    saved
}
pub fn use_arena(store: &mut Store, page: u16, start: u16, words: u16) -> bool {
    if store.arena_page != 65535 || !mesh_storage::begin(page, start, words) {
        return false;
    }
    store.arena_page = page;
    store.arena_start = start;
    store.arena_words = words;
    true
}
// Start a fresh payload page with the same region bounds. Existing slots
// retain their page, so lookup and release keep the original ownership.
pub fn open_arena_page(store: &mut Store, page: u16) -> bool {
    if store.arena_page == 65535 || page == store.arena_page {
        return false;
    }
    let mut id = 0u16;
    while id < store.count {
        if store.data[id] != 0 && store.pages[id] == page {
            return false;
        }
        id += 1;
    }
    if !mesh_storage::begin(page, store.arena_start, store.arena_words) {
        return false;
    }
    store.arena_page = page;
    true
}
fn allocate(store: &Store, words: u16) -> Ptr {
    if store.arena_page == 65535 {
        return try_malloc(words);
    }
    Ptr::from_addr(mesh_storage::allocate(
        store.arena_page,
        store.arena_start,
        store.arena_words,
        words,
    ))
}
fn release(store: &Store, id: u16) {
    if store.data[id] == 0 {
        return;
    }
    if store.pages[id] == 65535 {
        free(Ptr::from_addr(store.data[id]));
    } else {
        mesh_storage::release(
            store.pages[id],
            store.arena_start,
            store.arena_words,
            store.data[id],
        );
    }
}
fn cold_read(store: &Store, destination: Array<u16>, offset: fix32, count: u16) {
    if count == 0 {
        return;
    }
    let a = store.cold;
    let words = fix32::from_words(
        (a.byte_len_lo >> 1) | (a.byte_len_hi << 15),
        a.byte_len_hi >> 1,
    );
    if offset < n(0) || offset > words || fix32::from_words(count, 0) > words - offset {
        halt(0x7f6e);
    }
    if !gpu_window::gpu_window_asset_read(
        destination,
        a.first_sector + (offset.hi_bits() << 10) + (offset.lo_bits() >> 6),
        offset.lo_bits() & 63,
        count,
    ) {
        halt(0x7f6f);
    }
}
fn cold_target(store: &Store, dst: Array<u16>, page: u16, offset: fix32, count: u16) {
    if page == 65535 {
        cold_read(store, dst, offset, count);
        return;
    }
    let scratch: Buf<u16, 64> = Buf::new([0; 64]);
    let saved = read_dseg_page(3);
    let mut position = 0u16;
    while position < count {
        let mut size = count - position;
        if size > 64 {
            size = 64;
        }
        cold_read(
            store,
            scratch.as_array(),
            offset + fix32::from_words(position, 0),
            size,
        );
        mesh_storage::copy(at(dst, position), page, scratch.as_array(), 65535, size);
        position += size;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}
fn cold_columns(store: &Store, row: Array<u16>, source: Array<u16>, data: Array<u16>, page: u16) {
    let offset = attitude::get(at(source, 2), 0);
    let vertices = source[0u16] * 3;
    let triangles = source[1u16];
    cold_target(
        store,
        data,
        page,
        offset + fix32::from_words(10, 0),
        vertices,
    );
    cold_target(
        store,
        at(data, vertices),
        page,
        offset + fix32::from_words(10 + vertices + triangles * 3, 0),
        triangles,
    );
    cold_target(
        store,
        at(data, vertices + triangles),
        page,
        offset + fix32::from_words(8 + row[8u16], 0),
        row[12u16] * 3,
    );
}
fn materialize(store: &Store, id: u16) -> bool {
    if store.data[id] != 0 {
        return true;
    }
    let source = source(store, id);
    let row = at(store.records, id * 13);
    let words = source[0u16] * 3 + source[1u16] + row[12u16] * 3;
    if words == 0 {
        return true;
    }
    let allocation = allocate(store, words);
    if allocation.addr() == 0 {
        return false;
    }
    cold_columns(
        store,
        row,
        source,
        unsafe { allocation.as_u16_array() },
        store.arena_page,
    );
    let mut roots = store.data;
    roots[id] = allocation.addr();
    let mut pages = store.pages;
    pages[id] = store.arena_page;
    true
}
pub fn create(store: &mut Store, model: &Model, shared: bool) -> fix32 {
    let vertices = model.geometry[0u16] * 3;
    let triangles = model.geometry[1u16];
    let mut stride = 0u16;
    let mut i = 0u16;
    while i < 3 {
        let length = model.lengths[i];
        if length != 65535 && length > stride {
            stride = length;
        }
        i += 1;
    }
    // Capacity failures precede all owner changes. The caller reserves slots
    // and a per-resource span limit. Payload OOM also precedes owner changes.
    if vertices > store.slot_words
        || triangles > store.slot_words - vertices
        || stride > (store.slot_words - vertices - triangles) / 3
    {
        return n(-4);
    }
    if model.cold && store.cold.sector_count == 0 {
        return n(-1);
    }
    if model.index_page != 65535 && model.index_page > 255 {
        return n(-1);
    }
    let mut id = store.count;
    if store.free_count != 0 {
        id = store.free[store.free_count - 1];
        if id >= store.count || exists(store, id) {
            return n(-1);
        }
    } else if store.count >= store.capacity {
        return n(-4);
    }
    let words = vertices + triangles + stride * 3;
    let mut allocation = Ptr::from_addr(0);
    if words != 0 && !(model.cold && shared) {
        allocation = allocate(store, words);
        if allocation.addr() == 0 {
            return n(-4);
        }
    }
    let mut row = at(store.records, id * 13);
    if store.free_count != 0 {
        store.free_count -= 1;
    } else {
        store.count += 1;
        i = 0;
        while i < 4 {
            row[4 + i] = 0;
            i += 1;
        }
    }
    touch(at(row, 4));
    row[0u16] = 1;
    if shared {
        row[0u16] |= 2;
    }
    if model.index_page != 65535 {
        row[0u16] |= (model.index_page + 1) << 2;
    }
    row[1u16] = model.hint;
    row[2u16] = model.radius;
    row[3u16] = model.materials;
    row[8u16] = model.geometry_words;
    row[12u16] = stride;
    let mut sources = store.sources;
    sources[id] = model.geometry.as_ptr().addr();
    let mut payloads = store.data;
    let mut indices = store.indices;
    payloads[id] = allocation.addr();
    let mut pages = store.pages;
    pages[id] = 65535;
    if allocation.addr() != 0 {
        pages[id] = store.arena_page;
    }
    indices[id] = model.indices.as_ptr().addr();
    let data = unsafe { allocation.as_u16_array() };
    i = 0;
    while i < 3 {
        row[9 + i] = model.lengths[i];
        i += 1;
    }
    if allocation.addr() != 0 {
        if model.cold {
            cold_columns(store, row, model.geometry, data, store.arena_page);
        } else {
            mesh_storage::copy(
                data,
                store.arena_page,
                unsafe { model.vertices.as_ptr().as_u16_array() },
                model.page,
                vertices,
            );
            mesh_storage::copy(
                at(data, vertices),
                store.arena_page,
                model.flags,
                model.page,
                triangles,
            );
        }
    }
    i = 0;
    while i < 3 {
        let length = model.lengths[i];
        if length != 65535 && allocation.addr() != 0 && !model.cold {
            mesh_storage::copy(
                at(data, vertices + triangles + i * stride),
                store.arena_page,
                at(model.centers, i * model.center_stride),
                model.page,
                length,
            );
        }
        i += 1;
    }
    fix32::from_words(id, 0)
}
pub fn insert(store: &mut Store, geometry: Array<u16>, words: u16) -> fix32 {
    let absent: Buf<u16, 3> = Buf::new([65535; 3]);
    let model = Model {
        geometry: geometry,
        geometry_words: words,
        cold: false,
        page: 65535,
        index_page: 65535,
        vertices: unsafe { at(geometry, 2).as_ptr().as_i16_array() },
        indices: at(geometry, 2 + geometry[0u16] * 3),
        flags: at(geometry, 2 + geometry[0u16] * 3 + geometry[1u16] * 3),
        centers: geometry,
        lengths: absent.as_array(),
        center_stride: 0,
        hint: 0,
        radius: 0,
        materials: 0,
    };
    create(store, &model, false)
}
pub fn bind(store: &mut Store, key: u16, model: &Model) -> fix32 {
    let mut i = 0u16;
    while i < store.name_count {
        if store.names[i * 2] == key {
            return fix32::from_words(store.names[i * 2 + 1], 0);
        }
        i += 1;
    }
    if store.name_count >= store.name_capacity {
        return n(-4);
    }
    let id = create(store, model, true);
    if id >= n(0) {
        let mut names = store.names;
        names[store.name_count * 2] = key;
        names[store.name_count * 2 + 1] = id.lo_bits();
        store.name_count += 1;
    }
    id
}
pub fn private_copy(store: &mut Store, id: u16) -> fix32 {
    if !exists(store, id) {
        return n(-1);
    }
    let row = at(store.records, id * 13);
    let data = payload(store, id);
    let geometry = source(store, id);
    let vertices = geometry[0u16] * 3;
    let triangles = geometry[1u16];
    let model = Model {
        geometry: geometry,
        geometry_words: row[8u16],
        cold: store.data[id] == 0 && store.cold.sector_count != 0,
        page: store.pages[id],
        index_page: index_page(store, id),
        vertices: unsafe { data.as_ptr().as_i16_array() },
        indices: unsafe { Ptr::from_addr(store.indices[id]).as_u16_array() },
        flags: at(data, vertices),
        centers: at(data, vertices + triangles),
        lengths: at(row, 9),
        center_stride: row[12u16],
        hint: row[1u16],
        radius: row[2u16],
        materials: row[3u16],
    };
    create(store, &model, false)
}
pub fn release_private(store: &mut Store, id: u16) -> bool {
    if !exists(store, id) || store.free_count >= store.capacity {
        return false;
    }
    let mut row = at(store.records, id * 13);
    if row[0u16] & 2 != 0 {
        return false;
    }
    release(store, id);
    let mut payloads = store.data;
    payloads[id] = 0;
    let mut indices = store.indices;
    indices[id] = 0;
    row[0u16] &= 65534;
    row[3u16] = 0;
    row[8u16] = 0;
    row[9u16] = 65535;
    row[10u16] = 65535;
    row[11u16] = 65535;
    row[12u16] = 0;
    let mut sources = store.sources;
    sources[id] = 0;
    let mut free = store.free;
    free[store.free_count] = id;
    store.free_count += 1;
    true
}
pub fn set_hint(store: &Store, id: u16, hint: u16) -> bool {
    if !exists(store, id) {
        return false;
    }
    let mut row = at(store.records, id * 13);
    row[1u16] = hint;
    true
}
// Loading/session teardown owns shared as well as private allocations. No slot
// remains addressable after this explicit release of the whole resource owner.
pub fn finish(store: &mut Store) {
    let mut i = 0u16;
    while i < store.count {
        release(store, i);
        let mut payloads = store.data;
        payloads[i] = 0;
        i += 1;
    }
    store.count = 0;
    store.free_count = 0;
    store.name_count = 0;
    store.arena_page = 65535;
    store.arena_start = 0;
    store.arena_words = 0;
}
pub fn payload(store: &Store, id: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(store.data[id]).as_u16_array() }
}
// The immutable root stays alive across private copies. Mutable coordinates and
// flags are obtained through view(), rather than this original shape payload.
pub fn source(store: &Store, id: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(store.sources[id]).as_u16_array() }
}
pub fn index_page(store: &Store, id: u16) -> u16 {
    let tag = at(store.records, id * 13)[0u16] >> 2;
    if tag == 0 {
        return 65535;
    }
    tag - 1
}
// Read-only resolution retains the revision. A mutable access increments it
// before checking the payload, including an access to a released slot.
pub fn view(store: &Store, id: u16, mutable: bool) -> View {
    let mut row = store.records;
    let mut data = store.records;
    let valid = id < store.count;
    if valid {
        row = at(store.records, id * 13);
        data = payload(store, id);
        if mutable {
            touch(at(row, 4));
        }
    }
    let mut vertices = 0u16;
    let mut triangles = 0u16;
    let mut index_data = data;
    let mut present = valid && row[0u16] & 1 != 0;
    if present {
        present = materialize(store, id);
        data = payload(store, id);
        let geometry = source(store, id);
        index_data = unsafe { Ptr::from_addr(store.indices[id]).as_u16_array() };
        vertices = geometry[0u16];
        triangles = geometry[1u16];
    }
    View {
        mesh: mesh_break::Mesh {
            vertices: unsafe { data.as_ptr().as_i16_array() },
            vertex_count: vertices,
            indices: index_data,
            flags: at(data, vertices * 3),
            triangle_count: triangles,
            centers: unsafe { at(data, vertices * 3 + triangles).as_ptr().as_i16_array() },
            center_stride: row[12u16],
            center_lengths: at(row, 9),
            radius: at(row, 2),
        },
        revision: at(row, 4),
        valid: present,
        page: if valid && store.data[id] != 0 {
            store.pages[id]
        } else {
            65535
        },
        index_page: if present {
            index_page(store, id)
        } else {
            65535
        },
    }
}
