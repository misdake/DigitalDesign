// Move reference fixture state into the same packed bank used by level loading.
// Runtime construction reserves zeroed rows directly and does not copy state.
pub fn place_fragment_motion(
    object: &mut scene_fragments::Object,
    bank: &mut scene_fragments::MotionBank,
) {
    let offset = scene_fragments::reserve(bank, object.motion_capacity);
    if offset == 65535 {
        halt(0x7f74);
    }
    let state = scene_fragments::motion(offset, object.motion.count);
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, bank.page);
    }
    let mut header = state.header;
    let mut faces = state.faces;
    let mut i = 0u16;
    while i < 8 {
        header[i] = object.motion.header[i];
        i += 1;
    }
    i = 0;
    while i < object.motion.count * 16 {
        faces[i] = object.motion.faces[i];
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    object.motion = state;
    object.motion_page = bank.page;
}
pub fn check_fragment_layout(
    bank: &mut scene_fragments::MotionBank,
    layout: Array<u16>,
    count: u16,
    words: u16,
) {
    let saved = read_dseg_page(3);
    let mut i = 0u16;
    while i < count {
        let faces = layout[i * 3 + 1];
        let offset = scene_fragments::reserve(bank, faces);
        if offset != layout[i * 3 + 2] || read_dseg_page(3) != saved {
            halt(0x7f75);
        }
        i += 1;
    }
    if bank.used != words {
        halt(0x7f76);
    }
    unsafe {
        write_dseg_page(3, bank.page);
    }
    let data = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
    i = 0;
    while i < words {
        if data[i] != 0 {
            halt(0x7f77);
        }
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    let used = bank.used;
    if scene_fragments::reserve(bank, 65535) != 65535
        || bank.used != used
        || read_dseg_page(3) != saved
    {
        halt(0x7f78);
    }
}
// A cell can remain cached while another owner uses DSEG3. Rebinding the same
// cell must restore the physical page before any height coefficients are read.
pub fn check_fragment_terrain(map: &mut ground::World, object: &scene_fragments::Object) {
    let x = attitude::get(object.collider, 1);
    let z = attitude::get(object.collider, 3);
    let saved = read_dseg_page(3);
    map.bind(x, z);
    let first = map.terrain.height(x, z);
    let cell_page = read_dseg_page(3);
    if cell_page == object.motion_page {
        halt(0x7f79);
    }
    unsafe {
        write_dseg_page(3, object.motion_page);
    }
    map.bind(x, z);
    let second = map.terrain.height(x, z);
    if read_dseg_page(3) != cell_page
        || first.status != second.status
        || first.height != second.height
    {
        halt(0x7f7a);
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}
pub fn check_fragment_prefix(page: u16, words: u16) {
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, page);
    }
    let data = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
    let mut i = 0u16;
    while i < words {
        if data[i] != 0 {
            halt(0x7f91);
        }
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}
