//! Bounded DSEG3 access. Window pointers never leave this module.
use crate::dsl_rt::*;
mod gpu_layout;
use gpu_layout::*;
static GPU_WINDOW_FAULT: u16 = 0;
pub fn gpu_memory_faulted() -> bool {
    GPU_WINDOW_FAULT != 0
}
fn gpu_window_fail() {
    unsafe {
        addr_of(&GPU_WINDOW_FAULT).write(0, 1);
    }
}

// Separate the small hot loop from mapping/validation live values. Constant
// offsets amortize branch and pointer-update costs without changing word bits.
fn gpu_window_words(dst: Ptr, src: Ptr, words: u16) {
    let mut destination = dst;
    let mut source = src;
    let mut count = words;
    while count >= 4 {
        unsafe {
            destination.write(0, source.read(0));
            destination.write(1, source.read(1));
            destination.write(2, source.read(2));
            destination.write(3, source.read(3));
        }
        destination = destination.add(4);
        source = source.add(4);
        count -= 4;
    }
    while count != 0 {
        unsafe {
            destination.write(0, source.read(0));
        }
        destination = destination.add(1);
        source = source.add(1);
        count -= 1;
    }
}
fn gpu_window_low_valid(low: Array<u16>, count: u16) -> bool {
    low.as_ptr().addr() < 0xc000 && count <= 0xc000 - low.as_ptr().addr()
}
pub struct GpuWindowParts {
    pub first: Ptr,
    pub count: u16,
    pub tail: Ptr,
    pub tail_count: u16,
}
fn gpu_window_copy(low: Array<u16>, sector: u16, word: u16, count: u16, read: bool) -> bool {
    let mut parts: GpuWindowParts = GpuWindowParts {
        first: low.as_ptr(),
        count: count,
        tail: low.as_ptr(),
        tail_count: 0,
    };
    gpu_window_parts(view_of(&mut parts), sector, word, read)
}
// The two source spans form one contiguous high-memory snapshot. They share
// page fences, but do not require another full low-memory staging allocation.
fn gpu_window_upload_pair(
    first: Array<u16>,
    count: u16,
    tail: Array<u16>,
    tail_count: u16,
    sector: u16,
    word: u16,
) -> bool {
    let mut parts: GpuWindowParts = GpuWindowParts {
        first: first.as_ptr(),
        count: count,
        tail: tail.as_ptr(),
        tail_count: tail_count,
    };
    gpu_window_parts(view_of(&mut parts), sector, word, false)
}
// Callers validate allocation ownership. Reads/writes use CPU low-memory
// storage; page switches affect only logical 0xc000..0xffff, never the stack.
fn gpu_window_parts(parts: Array<GpuWindowParts>, sector: u16, word: u16, read: bool) -> bool {
    gpu_window_parts_mode(parts, sector, word, read, true)
}
// Internal CPU-owned input / immutable-asset scopes only. Physical cache tags
// and latched scalar/FST physical addresses allow remapping without publishing.
// The final submit still performs blocking clean+invalidate before either SEND.
fn gpu_window_parts_mode(
    parts: Array<GpuWindowParts>,
    sector: u16,
    word: u16,
    read: bool,
    fenced: bool,
) -> bool {
    let low = unsafe { parts[0u16].first.as_u16_array() };
    let count = parts[0u16].count;
    let tail = unsafe { parts[0u16].tail.as_u16_array() };
    let tail_count = parts[0u16].tail_count;
    if gpu_memory_faulted()
        || sector < GPU_DOMAIN_FIRST_SECTOR
        || count == 0
        || count > 32768
        || tail_count > 32768 - count
        || !gpu_window_low_valid(low, count)
        || (tail_count != 0 && !gpu_window_low_valid(tail, tail_count))
        || word >> 6 > 65535 - sector
    {
        return false;
    }
    let first = sector + (word >> 6);
    let total = count + tail_count;
    if ((word & 63) + (total - 1)) >> 6 > 65535 - first {
        return false;
    }
    let mut page = first >> 8;
    let mut offset = ((first & 255) << 6) | (word & 63);
    let mut copied: u16 = 0;
    while copied < total {
        let saved = read_dseg_page(3);
        if fenced && dcache_invalidate_all() != 0 {
            gpu_window_fail();
            return false;
        }
        unsafe {
            write_dseg_page(3, page);
        }
        if read_dseg_page(3) != page {
            unsafe {
                write_dseg_page(3, saved);
            }
            gpu_window_fail();
            return false;
        }
        let mut chunk = total - copied;
        if chunk > 16384 - offset {
            chunk = 16384 - offset;
        }
        let p = Ptr::from_addr(0xc000 | offset);
        let mut n: u16 = 0;
        if copied < count {
            n = count - copied;
            if n > chunk {
                n = chunk;
            }
            let q = low.as_ptr().add(copied as i16);
            if read {
                gpu_window_words(q, p, n);
            } else {
                gpu_window_words(p, q, n);
            }
        }
        if n < chunk {
            // Only upload_pair supplies a tail; the read wrapper sets its count to zero.
            let q = tail.as_ptr().add((copied + n - count) as i16);
            gpu_window_words(p.add(n as i16), q, chunk - n);
        }
        // The outgoing mapping stays installed until all writes/FST and
        // accepted responses drain. Restore even when maintenance reports error.
        let mut status: u16 = 0;
        if fenced {
            status = dcache_invalidate_all();
        }
        unsafe {
            write_dseg_page(3, saved);
        }
        if status != 0 || read_dseg_page(3) != saved {
            gpu_window_fail();
            return false;
        }
        copied += chunk;
        page += 1;
        offset = 0;
    }
    true
}

fn gpu_window_asset_read(low: Array<u16>, sector: u16, word: u16, count: u16) -> bool {
    if sector < GPU_ASSET_FIRST_SECTOR
        || sector >= GPU_ASSET_END_SECTOR
        || count == 0
        || count > 32768
        || word >> 6 >= GPU_ASSET_END_SECTOR - sector
    {
        return false;
    }
    let first = sector + (word >> 6);
    if ((word & 63) + (count - 1)) >> 6 >= GPU_ASSET_END_SECTOR - first {
        return false;
    }
    let mut parts: GpuWindowParts = GpuWindowParts {
        first: low.as_ptr(),
        count: count,
        tail: low.as_ptr(),
        tail_count: 0,
    };
    gpu_window_parts_mode(view_of(&mut parts), sector, word, true, false)
}
fn gpu_window_record_pair(
    first: Array<u16>,
    count: u16,
    tail: Array<u16>,
    tail_count: u16,
    sector: u16,
    word: u16,
) -> bool {
    if (sector != GPU_OBJECT_FIRST_SECTOR && sector != GPU_SLOT1_OBJECT_FIRST_SECTOR)
        || word > 32768
        || count > 32768 - word
        || tail_count > 32768 - word - count
    {
        return false;
    }
    let mut parts: GpuWindowParts = GpuWindowParts {
        first: first.as_ptr(),
        count: count,
        tail: tail.as_ptr(),
        tail_count: tail_count,
    };
    gpu_window_parts_mode(view_of(&mut parts), sector, word, false, false)
}
