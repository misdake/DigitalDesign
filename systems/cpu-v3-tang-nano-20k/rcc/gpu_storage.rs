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
    if count > GPU_API_PUSH_CHUNK_WORDS
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

// Complete, disjoint lines in one physical DSEG3 page. Neither pointer escapes.
fn gpu_window_copy_lines(
    source: u16,
    source_word: u16,
    target: u16,
    target_word: u16,
    count: u16,
) -> bool {
    if count == 0 {
        return true;
    }
    if source >> 8 != target >> 8
        || (source_word | target_word | count) & 15 != 0
        || source_word > 16384
        || target_word > 16384
    {
        return false;
    }
    let from = ((source & 255) << 6) + source_word;
    let to = ((target & 255) << 6) + target_word;
    if from < source_word
        || to < target_word
        || from >= 16384
        || to >= 16384
        || count > 16384 - from
        || count > 16384 - to
    {
        return false;
    }
    if (from <= to && count > to - from) || (to < from && count > from - to) {
        return false;
    }
    let saved = read_dseg_page(3);
    dcache_wait();
    unsafe {
        write_dseg_page(3, source >> 8);
    }
    if read_dseg_page(3) != source >> 8 {
        unsafe {
            write_dseg_page(3, saved);
        }
        return false;
    }
    let mut at: u16 = 0;
    while at < count {
        unsafe {
            dcache_line_copy(
                Ptr::from_addr(0xc000 | (from + at)),
                Ptr::from_addr(0xc000 | (to + at)),
            );
        }
        dcache_wait();
        at += 16;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    read_dseg_page(3) == saved
}

// Prepared allocations and frame CBs need not share a DSEG3 page. Retain the
// direct LCOPY path when possible; otherwise bounce bounded aligned CPU lines.
fn gpu_window_copy_span(
    source: u16,
    source_word: u16,
    target: u16,
    target_word: u16,
    count: u16,
    staging: Array<u16>,
) -> bool {
    if count == 0 {
        return true;
    }
    if (source_word | target_word | count) & 15 != 0
        || source_word > 32768
        || target_word > 32768
        || count > 32768 - source_word
        || count > 32768 - target_word
    {
        return false;
    }
    if source >> 8 == target >> 8
        && source_word <= 16384 - ((source & 255) << 6)
        && target_word <= 16384 - ((target & 255) << 6)
        && count <= 16384 - ((source & 255) << 6) - source_word
        && count <= 16384 - ((target & 255) << 6) - target_word
    {
        return gpu_window_copy_lines(source, source_word, target, target_word, count);
    }
    let mut at: u16 = 0;
    while at < count {
        let mut words = count - at;
        if words > 64 {
            words = 64;
        }
        if !gpu_window_read_words(source, source_word + at, staging, words)
            || !gpu_window_write_words(target, target_word + at, staging, words)
        {
            return false;
        }
        at += words;
    }
    true
}
