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

// Callers validate allocation ownership. Reads/writes use CPU low-memory
// storage; page switches affect only logical 0xc000..0xffff, never the stack.
fn gpu_window_copy(mut low: Array<u16>, sector: u16, word: u16, count: u16, read: bool) -> bool {
    if gpu_memory_faulted()
        || sector < GPU_DOMAIN_FIRST_SECTOR
        || count == 0
        || count > 32768
        || low.as_ptr().addr() >= 0xc000
        || count > 0xc000 - low.as_ptr().addr()
        || word >> 6 > 65535 - sector
    {
        return false;
    }
    let first = sector + (word >> 6);
    if ((word & 63) + (count - 1)) >> 6 > 65535 - first {
        return false;
    }
    let mut page = first >> 8;
    let mut offset = ((first & 255) << 6) | (word & 63);
    let mut copied: u16 = 0;
    while copied < count {
        let saved = read_dseg_page(3);
        if dcache_invalidate_all() != 0 {
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
        let mut chunk = count - copied;
        if chunk > 16384 - offset {
            chunk = 16384 - offset;
        }
        let p = Ptr::from_addr(0xc000 | offset);
        let mut i: u16 = 0;
        while i < chunk {
            if read {
                low[copied + i] = unsafe { p.read(i as i16) };
            } else {
                unsafe {
                    p.write(i as i16, low[copied + i]);
                }
            }
            i += 1;
        }
        // The outgoing mapping stays installed until all writes/FST and
        // accepted responses drain. Restore even when maintenance reports error.
        let status = dcache_invalidate_all();
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
