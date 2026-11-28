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
    check_mesh_pages(page, largest + count - 10);
    mesh_storage::release(page, start, count, largest);
    if mesh_storage::used(page, start, count) != 0 || read_dseg_page(3) != saved {
        halt(0x7f8d);
    }
}

// CPU backing is also reachable through a mapped alias. Exercise both transfer
// directions at the last payload words, including restoration of both windows.
fn cpu_backing_page(address: u16) -> u16 {
    if address < 0x4000 {
        return read_dseg_page(0);
    }
    if address < 0x8000 {
        return read_dseg_page(1);
    }
    if address < 0xc000 {
        return read_dseg_page(2);
    }
    halt(0x7f96);
}
fn check_mesh_pages(page: u16, address: u16) {
    let before2 = read_dseg_page(2);
    let before3 = read_dseg_page(3);
    let input: Buf<u16, 8> = Buf::new([3, 9, 27, 81, 243, 729, 2187, 6561]);
    let output: Buf<u16, 8> = Buf::new([0; 8]);
    let from = input.as_array().as_ptr().addr();
    let to = output.as_array().as_ptr().addr();
    if (from & 16383) > 16376 || (to & 16383) > 16376 {
        halt(0x7f96);
    }
    let source = unsafe { Ptr::from_addr(0xc000 + (from & 16383)).as_u16_array() };
    let destination = unsafe { Ptr::from_addr(0xc000 + (to & 16383)).as_u16_array() };
    let external = unsafe { Ptr::from_addr(address).as_u16_array() };
    mesh_storage::copy(external, page, source, cpu_backing_page(from), 8);
    mesh_storage::copy(destination, cpu_backing_page(to), external, page, 8);
    let mut i = 0u16;
    while i < 8 {
        if input[i] != output[i] {
            halt(0x7f96);
        }
        i += 1;
    }
    if read_dseg_page(2) != before2 || read_dseg_page(3) != before3 {
        halt(0x7f96);
    }
}
