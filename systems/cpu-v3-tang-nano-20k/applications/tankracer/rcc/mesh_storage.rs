//! Level-owned payload allocation within one mapped 32-KiB working bank.
use crate::dsl_rt::*;

fn words() -> Array<u16> {
    unsafe { Ptr::from_addr(0xc000).as_u16_array() }
}
// Region word 0 holds the free-list head. Blocks have one length word; a
// free block uses its first payload word for the next offset. Offset 0 is null.
pub fn begin(page: u16, start: u16, count: u16) -> bool {
    if page >= 256 || start > 16381 || count < 3 || count > 16384 - start {
        return false;
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, page);
    }
    let mut data = words();
    data[start] = start + 1;
    data[start + 1] = count - 1;
    data[start + 2] = 0;
    unsafe {
        write_dseg_page(3, saved);
    }
    true
}
pub fn allocate(page: u16, start: u16, count: u16, size: u16) -> u16 {
    if size == 0 || size > 16381 {
        return 0;
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, page);
    }
    let mut data = words();
    let mut link = start;
    let mut block = data[link];
    let need = size + 1;
    let mut result = 0u16;
    while block != 0 {
        let span = data[block];
        if block <= start || block >= start + count || span < 2 || span > start + count - block {
            halt(0x7f80);
        }
        if span >= need {
            let next = data[block + 1];
            if span - need >= 2 {
                let tail = block + need;
                data[tail] = span - need;
                data[tail + 1] = next;
                data[link] = tail;
                data[block] = need;
            } else {
                data[link] = next;
            }
            result = 0xc000 + block + 1;
            break;
        }
        link = block + 1;
        block = data[link];
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    result
}
pub fn release(page: u16, start: u16, count: u16, address: u16) {
    if address <= 0xc000 + start + 1 {
        halt(0x7f81);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, page);
    }
    let mut data = words();
    let block = address - 0xc000 - 1;
    if block >= start + count || data[block] < 2 || data[block] > start + count - block {
        halt(0x7f81);
    }
    let mut previous = 0u16;
    let mut next = data[start];
    while next != 0 && next < block {
        previous = next;
        next = data[next + 1];
    }
    if (previous != 0 && previous + data[previous] > block)
        || (next != 0 && block + data[block] > next)
    {
        halt(0x7f81);
    }
    if next != 0 && block + data[block] == next {
        data[block] += data[next];
        next = data[next + 1];
    }
    data[block + 1] = next;
    if previous != 0 && previous + data[previous] == block {
        data[previous] += data[block];
        data[previous + 1] = next;
    } else if previous == 0 {
        data[start] = block;
    } else {
        data[previous + 1] = block;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}
pub fn used(page: u16, start: u16, count: u16) -> u16 {
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, page);
    }
    let data = words();
    let mut block = data[start];
    let mut previous = start;
    let mut available = 0u16;
    while block != 0 {
        if block <= previous
            || block >= start + count
            || data[block] < 2
            || data[block] > start + count - block
        {
            halt(0x7f80);
        }
        available += data[block];
        if available > count - 1 {
            halt(0x7f80);
        }
        previous = block;
        block = data[block + 1];
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    count - 1 - available
}
// A resource owner uses one external arena. CPU endpoints and same-bank
// payload copies remain direct; mapped data is never bounced per update.
pub fn copy(mut dst: Array<u16>, dst_page: u16, src: Array<u16>, src_page: u16, count: u16) {
    if dst_page != 65535 && src_page != 65535 && dst_page != src_page {
        halt(0x7f84);
    }
    let saved = read_dseg_page(3);
    let mut page = src_page;
    if dst_page != 65535 {
        page = dst_page;
    }
    if page != 65535 {
        unsafe {
            write_dseg_page(3, page);
        }
    }
    let mut i = 0u16;
    while i < count {
        dst[i] = src[i];
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}
