// Allocation lifetime and mapping invariants independent of mesh algorithms.
pub fn check_mesh_storage(page: u16, start: u16, count: u16) {
    let saved = read_dseg_page(3);
    let ids: Buf<u16, 4> = Buf::new([0; 4]);
    let sizes: Buf<u16, 4> = Buf::new([1, 2, 3, 4]);
    let order: Buf<u16, 4> = Buf::new([1, 3, 0, 2]);
    let mut pointers = ids.as_array();
    if !mesh_storage::begin(page, start, count) {
        halt(0x7f85);
    }
    let mut i = 0u16;
    while i < 4 {
        pointers[i] = mesh_storage::allocate(page, start, count, sizes[i]);
        if pointers[i] == 0 || read_dseg_page(3) != saved {
            halt(0x7f86);
        }
        unsafe {
            write_dseg_page(3, page);
        }
        let mut data = unsafe { Ptr::from_addr(pointers[i]).as_u16_array() };
        let mut j = 0u16;
        while j < sizes[i] {
            data[j] = 0x51a0 + i * 16 + j;
            j += 1;
        }
        unsafe {
            write_dseg_page(3, saved);
        }
        i += 1;
    }
    let occupied = mesh_storage::used(page, start, count);
    if mesh_storage::allocate(page, start, count, count) != 0
        || mesh_storage::used(page, start, count) != occupied
    {
        halt(0x7f87);
    }
    i = 0;
    while i < 4 {
        // Every still-live allocation retains its contents after a failed
        // allocation and after freeing differently sized neighbors.
        let mut k = 0u16;
        unsafe {
            write_dseg_page(3, page);
        }
        while k < 4 {
            if pointers[k] != 0 {
                let data = unsafe { Ptr::from_addr(pointers[k]).as_u16_array() };
                let mut j = 0u16;
                while j < sizes[k] {
                    if data[j] != 0x51a0 + k * 16 + j {
                        halt(0x7f88);
                    }
                    j += 1;
                }
            }
            k += 1;
        }
        unsafe {
            write_dseg_page(3, saved);
        }
        let id = order[i];
        mesh_storage::release(page, start, count, pointers[id]);
        pointers[id] = 0;
        if read_dseg_page(3) != saved {
            halt(0x7f89);
        }
        i += 1;
    }
    if mesh_storage::used(page, start, count) != 0 {
        halt(0x7f8a);
    }
    let largest = mesh_storage::allocate(page, start, count, count - 2);
    if largest != 0xc000 + start + 2
        || mesh_storage::used(page, start, count) != count - 1
        || mesh_storage::allocate(page, start, count, 1) != 0
    {
        halt(0x7f8b);
    }
    let input: Buf<u16, 1> = Buf::new([0x73d6]);
    let output: Buf<u16, 1> = Buf::new([0]);
    let last = unsafe { Ptr::from_addr(largest + count - 3).as_u16_array() };
    mesh_storage::copy(last, page, input.as_array(), 65535, 1);
    mesh_storage::copy(output.as_array(), 65535, last, page, 1);
    if output[0u16] != input[0u16] || read_dseg_page(3) != saved {
        halt(0x7f8c);
    }
    mesh_storage::release(page, start, count, largest);
    if mesh_storage::used(page, start, count) != 0 || read_dseg_page(3) != saved {
        halt(0x7f8d);
    }
}
