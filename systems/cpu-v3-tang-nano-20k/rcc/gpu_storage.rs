//! Bounded CPU-owned external spans. No DSEG3 pointer leaves this module.
//! Scalar stores are published at submit; aligned line copies complete in DRAM here.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;

fn gpu_storage_copy_owned(low: Array<u16>, sector: u16, word: u16, count: u16, read: bool) -> bool {
    if count == 0 {
        return true;
    }
    let address = low.as_ptr().addr();
    if count > GPU_API_STORAGE_CHUNK_WORDS
        || address >= 0xc000
        || count > 0xc000 - address
        || word >> 6 > 65535 - sector
    {
        return false;
    }
    let first = sector + (word >> 6);
    if ((word & 63) + count - 1) >> 6 > 65535 - first {
        return false;
    }
    let saved = read_dseg_page(3);
    let mut page = first >> 8;
    let mut offset = ((first & 255) << 6) | (word & 63);
    let mut done: u16 = 0;
    while done < count {
        // Complete any previous local-copy operation before changing its window.
        dcache_wait();
        unsafe {
            write_dseg_page(3, page);
        }
        if read_dseg_page(3) != page {
            unsafe {
                write_dseg_page(3, saved);
            }
            return false;
        }
        let window = Ptr::from_addr(0xc000 | offset);
        let local = low.as_ptr().add(done as i16);
        let mut chunk = count - done;
        if chunk > 16384 - offset {
            chunk = 16384 - offset;
        }
        let mut n: u16 = 0;
        while n < chunk {
            let from_low = local.add(n as i16);
            let external = window.add(n as i16);
            if (from_low.addr() | external.addr()) & 15 == 0 && chunk - n >= 16 {
                // Both complete lines are CPU-owned. LCOPY reads the newest
                // source cache value, writes DRAM and discards an old target alias.
                let mut source = from_low;
                let mut destination = external;
                if read {
                    source = external;
                    destination = from_low;
                }
                unsafe {
                    dcache_line_copy(source, destination);
                }
                dcache_wait();
                n += 16;
            } else {
                unsafe {
                    if read {
                        from_low.write(0, external.read(0));
                    } else {
                        external.write(0, from_low.read(0));
                    }
                }
                n += 1;
            }
        }
        dcache_wait();
        done += chunk;
        page += 1;
        offset = 0;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    read_dseg_page(3) == saved
}
// A zero count succeeds without inspecting addresses or changing the mapping.
fn gpu_window_read_words(sector: u16, word: u16, dst: Array<u16>, count: u16) -> bool {
    gpu_storage_copy_owned(dst, sector, word, count, true)
}
fn gpu_window_write_words(sector: u16, word: u16, src: Array<u16>, count: u16) -> bool {
    gpu_storage_copy_owned(src, sector, word, count, false)
}
