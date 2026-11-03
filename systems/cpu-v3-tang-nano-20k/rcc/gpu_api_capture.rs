//! Private explicit-record capture. Never publishes, submits or retires work.
use crate::dsl_rt::*;
mod gpu_capture_abi;
use gpu_capture_abi::*;
mod gpu_capture_config;
use gpu_capture_config::*;
mod gpu_execute;
use gpu_execute::*;
mod gpu_present;
use gpu_present::*;
mod gpu_storage;
use gpu_storage::*;
mod profiler_gate;
use profiler_gate::*;
mod gpu_depth;
use gpu_depth::*;

static GPU_CAPTURE_HEADER: Buf<u16, 64> = Buf::new([0; 64]);
static GPU_CAPTURE_RANGES: Buf<u16, 512> = Buf::new([0; 512]);
static GPU_CAPTURE_CACHE: Buf<u16, 64> = Buf::new([0; 64]);
// Frozen source12 is an interop snapshot, never A's private record layout.
static GPU_CAPTURE_SOURCE: Buf<u16, 12> = Buf::new([0; 12]);
// live,error,token32,submitted32,cache range+1,offset32,count,slot,ranges,
// Global/Render validity,color/depth range+1,loaded pages,uniform mask,
// two page uniform/UV requirements,and the installed Render texture words.
// Final pair counts aggregate serialized bytes, including overlapping ranges.
static GPU_CAPTURE_CONTROL: Buf<u16, 24> = Buf::new([0; 24]);
// Per local32B slot66..79: source sector, word offset, initialized flag.
// This is native DMA provenance, not a copy of A's allocation facts.
static GPU_CAPTURE_LOCAL: Buf<u16, 42> = Buf::new([0; 42]);
static GPU_CAPTURE_PRESENT: Buf<u16, 4> = Buf::new([0; 4]);

fn gpu_capture_fail(code: u16) -> u16 {
    if GPU_CAPTURE_CONTROL[1u16] == 0 {
        GPU_CAPTURE_CONTROL[1u16] = code;
    }
    GPU_CAPTURE_CONTROL[1u16]
}
fn gpu_capture_reject(code: u16) -> bool {
    gpu_capture_fail(code);
    false
}
fn gpu_capture_pair(at: u16, low: u16, high: u16) {
    GPU_CAPTURE_HEADER[at] = low;
    GPU_CAPTURE_HEADER[at + 1] = high;
}
fn gpu_capture_base(at: u16, sector: u16) {
    gpu_capture_pair(at, sector << 7, sector >> 9);
}
fn gpu_capture_extent(first: u16, count: u16) -> bool {
    count != 0 && count - 1 <= 65535 - first
}
fn gpu_capture_overlap(a: u16, an: u16, b: u16, bn: u16) -> bool {
    a <= b + bn - 1 && b <= a + an - 1
}
fn gpu_capture_range_add(tag: u16, flags: u16, sector: u16, low: u16, high: u16) -> u16 {
    let base = sector << 7;
    let end = base + low;
    let mut upper = (sector >> 9) + high;
    if end < base {
        upper += 1;
    }
    if (low == 0 && high == 0)
        || low & 1 != 0
        || high > 128
        || upper > 128
        || (upper == 128 && end != 0)
    {
        gpu_capture_fail(GPU_CAPTURE_BOUNDS);
        return 0;
    }
    let count = GPU_CAPTURE_CONTROL[11u16];
    let mut i: u16 = 0;
    while i < count {
        let at = i * 8;
        if GPU_CAPTURE_RANGES[at + 1] == tag
            && GPU_CAPTURE_RANGES[at + 2] == base
            && GPU_CAPTURE_RANGES[at + 3] == sector >> 9
            && GPU_CAPTURE_RANGES[at + 4] == low
            && GPU_CAPTURE_RANGES[at + 5] == high
        {
            if tag == GPU_CAPTURE_TAG_DEPTH
                && (GPU_CAPTURE_RANGES[at] & GPU_CAPTURE_DEPTH_SHIFT_MASK)
                    != (flags & GPU_CAPTURE_DEPTH_SHIFT_MASK)
            {
                gpu_capture_fail(GPU_CAPTURE_DEPENDENCY);
                return 0;
            }
            GPU_CAPTURE_RANGES[at] |= flags;
            return i + 1;
        }
        i += 1;
    }
    if count >= GPU_CAPTURE_MAX_RANGES {
        gpu_capture_fail(GPU_CAPTURE_CAPACITY);
        return 0;
    }
    let total = GPU_CAPTURE_CONTROL[22u16] + low;
    let mut total_high = GPU_CAPTURE_CONTROL[23u16] + high;
    if total < GPU_CAPTURE_CONTROL[22u16] {
        total_high += 1;
    }
    if total_high > GPU_CAPTURE_MAX_TOTAL_HIGH
        || (total_high == GPU_CAPTURE_MAX_TOTAL_HIGH && total != 0)
    {
        gpu_capture_fail(GPU_CAPTURE_CAPACITY);
        return 0;
    }
    let at = count * 8;
    GPU_CAPTURE_RANGES[at] = flags;
    GPU_CAPTURE_RANGES[at + 1] = tag;
    GPU_CAPTURE_RANGES[at + 2] = base;
    GPU_CAPTURE_RANGES[at + 3] = sector >> 9;
    GPU_CAPTURE_RANGES[at + 4] = low;
    GPU_CAPTURE_RANGES[at + 5] = high;
    GPU_CAPTURE_RANGES[at + 6] = 0;
    GPU_CAPTURE_RANGES[at + 7] = 0;
    if tag <= GPU_CAPTURE_TAG_UI {
        GPU_CAPTURE_RANGES[at + 6] = GPU_CAPTURE_CONTROL[2u16];
        GPU_CAPTURE_RANGES[at + 7] = GPU_CAPTURE_CONTROL[3u16];
    }
    GPU_CAPTURE_CONTROL[11u16] = count + 1;
    GPU_CAPTURE_CONTROL[22u16] = total;
    GPU_CAPTURE_CONTROL[23u16] = total_high;
    count + 1
}
fn gpu_capture_health() -> u16 {
    if GPU_CAPTURE_CONTROL[0u16] == 0 {
        return GPU_CAPTURE_NO_LEASE;
    }
    if GPU_CAPTURE_CONTROL[1u16] != 0 {
        return GPU_CAPTURE_CONTROL[1u16];
    }
    if gpu_execute_capture_health(GPU_CAPTURE_CONTROL[10u16]) != 0 {
        return gpu_capture_fail(GPU_CAPTURE_GPU_ERROR);
    }
    // This stage explicitly omits Front. A pending/torn Display observation is
    // not a frozen input, and never causes an implicit Display wait or open.
    if gpu_present_capture_state(GPU_CAPTURE_PRESENT.as_array()) == 65535 {
        return gpu_capture_fail(GPU_CAPTURE_DISPLAY_ERROR);
    }
    GPU_CAPTURE_OK
}
fn gpu_capture_range_value(range: u16, field: u16) -> u16 {
    if gpu_capture_health() != 0 {
        return 0;
    }
    if range >= GPU_CAPTURE_CONTROL[11u16] || field >= 8 {
        gpu_capture_fail(GPU_CAPTURE_BOUNDS);
        return 0;
    }
    GPU_CAPTURE_RANGES[range * 8 + field]
}
fn gpu_capture_read_value(range: u16, offset_low: u16, offset_high: u16) -> u16 {
    if gpu_capture_health() != 0 {
        return 0;
    }
    if range >= GPU_CAPTURE_CONTROL[11u16] {
        gpu_capture_fail(GPU_CAPTURE_BOUNDS);
        return 0;
    }
    let at = range * 8;
    let low = GPU_CAPTURE_RANGES[at + 4];
    let high = GPU_CAPTURE_RANGES[at + 5];
    let words = (low >> 1) | (high << 15);
    let words_high = high >> 1;
    if offset_high > words_high || (offset_high == words_high && offset_low >= words) {
        gpu_capture_fail(GPU_CAPTURE_BOUNDS);
        return 0;
    }
    let chunk = offset_low & 65472;
    if GPU_CAPTURE_CONTROL[6u16] != range + 1
        || GPU_CAPTURE_CONTROL[7u16] != chunk
        || GPU_CAPTURE_CONTROL[8u16] != offset_high
    {
        let base_low = GPU_CAPTURE_RANGES[at + 2];
        let base_high = GPU_CAPTURE_RANGES[at + 3];
        let base_word = (base_low >> 1) | (base_high << 15);
        let physical = base_word + chunk;
        let mut physical_high = (base_high >> 1) + offset_high;
        if physical < base_word {
            physical_high += 1;
        }
        let remaining = words - chunk;
        let mut remaining_high = words_high - offset_high;
        if words < chunk {
            remaining_high -= 1;
        }
        let mut count: u16 = 64;
        if remaining_high == 0 && remaining < count {
            count = remaining;
        }
        if !gpu_window_read_words(
            (physical_high << 10) | (physical >> 6),
            physical & 63,
            GPU_CAPTURE_CACHE.as_array(),
            count,
        ) {
            gpu_capture_fail(GPU_CAPTURE_MEMORY);
            return 0;
        }
        GPU_CAPTURE_CONTROL[6u16] = range + 1;
        GPU_CAPTURE_CONTROL[7u16] = chunk;
        GPU_CAPTURE_CONTROL[8u16] = offset_high;
        GPU_CAPTURE_CONTROL[9u16] = count;
    }
    GPU_CAPTURE_CACHE[offset_low & 63]
}
// Allocation facts authorize bounds only; they are not copied into a second
// directory and do not certify immutability or replace native-stream decoding.
fn gpu_capture_plane(facts: Array<u16>, kind: u16, sector: u16) -> u16 {
    let mut i: u16 = 0;
    let mut allocation: u16 = 0;
    let mut depth_flags: u16 = 0;
    while i < GPU_CAPTURE_SOURCE[9u16] {
        let at = i * 4;
        if facts[at] == sector
            && facts[at + 2] == kind
            && facts[at + 3] & 2 != 0
            && facts[at + 1] >= 1600
        {
            if facts[at + 1] > allocation {
                allocation = facts[at + 1];
            }
            if kind == 6 {
                depth_flags =
                    gpu_execute_capture_depth_shift(facts[at + 3], facts[at], facts[at + 1]) << 5;
            }
        }
        i += 1;
    }
    if allocation == 0
        || sector & 511 != 0
        || gpu_capture_overlap(
            sector,
            1600,
            GPU_CAPTURE_SOURCE[2u16],
            GPU_CAPTURE_SOURCE[3u16] >> 6,
        )
        || gpu_capture_overlap(
            sector,
            1600,
            GPU_CAPTURE_SOURCE[6u16],
            GPU_CAPTURE_SOURCE[7u16] >> 6,
        )
    {
        gpu_capture_fail(GPU_CAPTURE_DEPENDENCY);
        return 0;
    }
    let tag = kind + 2;
    let range = gpu_capture_range_add(
        tag,
        GPU_CAPTURE_WRITE | GPU_CAPTURE_PREIMAGE | depth_flags,
        sector,
        8192,
        3,
    );
    if range == 0 {
        return 0;
    }
    if allocation > 1600 {
        let tail = allocation - 1600;
        if gpu_capture_range_add(
            tag,
            GPU_CAPTURE_READ | GPU_CAPTURE_PREIMAGE | depth_flags,
            sector + 1600,
            tail << 7,
            tail >> 9,
        ) == 0
        {
            return 0;
        }
    }
    range
}
fn gpu_capture_finish_target() {
    let mut i: u16 = 14;
    while i < 16 {
        let range = GPU_CAPTURE_CONTROL[i];
        if range != 0 {
            let at = (range - 1) * 8;
            if GPU_CAPTURE_RANGES[at] & GPU_CAPTURE_FULL_CLEAR == 0 {
                GPU_CAPTURE_RANGES[at] |= GPU_CAPTURE_READ;
            }
        }
        i += 1;
    }
}
fn gpu_capture_target(facts: Array<u16>, local: u16) -> bool {
    if local < 66 || local >= 80 || GPU_CAPTURE_LOCAL[local - 66 + 28] == 0 {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    let a = gpu_capture_descriptor_word(local - 66, 0);
    let b = gpu_capture_descriptor_word(local - 66, 1);
    let color = (a & 16383) << 2;
    let depth = ((a >> 12) & 12) | (b << 4);
    if b >> 12 != 0 || gpu_capture_overlap(color, 1600, depth, 1600) {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    let mut i: u16 = 2;
    while i < 16 {
        if gpu_capture_descriptor_word(local - 66, i) != 0 {
            return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
        }
        i += 1;
    }
    gpu_capture_finish_target();
    let color_range = gpu_capture_plane(facts, 5, color);
    let depth_range = gpu_capture_plane(facts, 6, depth);
    if color_range == 0 || depth_range == 0 {
        return false;
    }
    GPU_CAPTURE_CONTROL[14u16] = color_range;
    GPU_CAPTURE_CONTROL[15u16] = depth_range;
    gpu_capture_base(GPU_CAPTURE_H_COLOR_BASE, color);
    gpu_capture_base(GPU_CAPTURE_H_DEPTH_BASE, depth);
    gpu_capture_pair(GPU_CAPTURE_H_PLANE_BYTES, 8192, 3);
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_FLAGS] |= GPU_CAPTURE_HAS_TARGET;
    true
}
// Facts bound allocations; actual native operands select the captured subspan.
// kind0 accepts any explicitly declared access (raw copy/local staging).
fn gpu_capture_fact(facts: Array<u16>, kind: u16, first: u16, count: u16, access: u16) -> bool {
    if !gpu_capture_extent(first, count) {
        return false;
    }
    let mut i: u16 = 0;
    while i < GPU_CAPTURE_SOURCE[9u16] {
        let at = i * 4;
        let base = facts[at];
        let capacity = facts[at + 1];
        if (kind == 0 || facts[at + 2] == kind)
            && facts[at + 3] & access == access
            && first >= base
            && first - base < capacity
            && count <= capacity - (first - base)
        {
            return true;
        }
        i += 1;
    }
    false
}
fn gpu_capture_resource(
    facts: Array<u16>,
    kind: u16,
    tag: u16,
    first: u16,
    count: u16,
    flags: u16,
) -> bool {
    if !gpu_capture_fact(facts, kind, first, count, flags & 3) {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    gpu_capture_range_add(tag, flags, first, count << 7, count >> 9) != 0
}
fn gpu_capture_written(first: u16, count: u16) -> bool {
    let mut i: u16 = 0;
    while i < GPU_CAPTURE_CONTROL[11u16] {
        let at = i * 8;
        let base = (GPU_CAPTURE_RANGES[at + 2] >> 7) | (GPU_CAPTURE_RANGES[at + 3] << 9);
        let sectors = (GPU_CAPTURE_RANGES[at + 4] >> 7) | (GPU_CAPTURE_RANGES[at + 5] << 9);
        if GPU_CAPTURE_RANGES[at] & GPU_CAPTURE_WRITE != 0
            && sectors != 0
            && gpu_capture_overlap(first, count, base, sectors)
        {
            return true;
        }
        i += 1;
    }
    false
}
// Return a containing READ range+1 for a word span within one physical sector.
fn gpu_capture_read_range(first: u16, word: u16, words: u16) -> u16 {
    let mut i: u16 = 0;
    while i < GPU_CAPTURE_CONTROL[11u16] {
        let at = i * 8;
        let base = (GPU_CAPTURE_RANGES[at + 2] >> 7) | (GPU_CAPTURE_RANGES[at + 3] << 9);
        if first >= base && GPU_CAPTURE_RANGES[at] & GPU_CAPTURE_READ != 0 {
            let delta = first - base;
            let low = (delta << 6) + word;
            let mut high = delta >> 10;
            if low < (delta << 6) {
                high += 1;
            }
            let end = low + words;
            if end < low {
                high += 1;
            }
            let bytes_low = GPU_CAPTURE_RANGES[at + 4];
            let bytes_high = GPU_CAPTURE_RANGES[at + 5];
            let limit = (bytes_low >> 1) | (bytes_high << 15);
            let upper = bytes_high >> 1;
            if high < upper || (high == upper && end <= limit) {
                return i + 1;
            }
        }
        i += 1;
    }
    0
}
fn gpu_capture_physical_word(first: u16, word: u16) -> u16 {
    let sector = first + (word >> 6);
    let local = word & 63;
    let range = gpu_capture_read_range(sector, local, 1);
    if range == 0 {
        gpu_capture_fail(GPU_CAPTURE_DEPENDENCY);
        return 0;
    }
    let at = (range - 1) * 8;
    let base = (GPU_CAPTURE_RANGES[at + 2] >> 7) | (GPU_CAPTURE_RANGES[at + 3] << 9);
    let delta = sector - base;
    gpu_capture_read_value(range - 1, (delta << 6) | local, delta >> 10)
}
fn gpu_capture_descriptor_word(index: u16, word: u16) -> u16 {
    let slot = index + (word >> 4);
    if slot >= 14 || GPU_CAPTURE_LOCAL[slot + 28] == 0 {
        gpu_capture_fail(GPU_CAPTURE_DEPENDENCY);
        return 0;
    }
    gpu_capture_physical_word(
        GPU_CAPTURE_LOCAL[slot],
        GPU_CAPTURE_LOCAL[slot + 14] + (word & 15),
    )
}
fn gpu_capture_local_dma(facts: Array<u16>, low: u16, high: u16) -> bool {
    let slot = (low >> 3) & 127;
    let code = (low >> 1) & 3;
    if low & 1 != 0 || code == 3 {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    let slots: u16 = 1 << code;
    let first = (low >> 12) | (high << 4);
    let word = (low >> 6) & 48;
    if word & ((slots << 4) - 1) != 0
        || !((slot >= 66 && slot + slots <= 80)
            || (slots == 4 && slot >= 80 && slot <= 92 && slot & 3 == 0))
        || (slot < 80 && gpu_capture_written(first, 1))
    {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    let mut own: bool = false;
    let mut i: u16 = 0;
    while i < 2 {
        let mut base = GPU_CAPTURE_SOURCE[2u16];
        let mut used = GPU_CAPTURE_SOURCE[5u16];
        if i == 1 {
            base = GPU_CAPTURE_SOURCE[6u16];
            used = GPU_CAPTURE_SOURCE[8u16];
        }
        if first >= base && first - base <= 511 {
            let offset = ((first - base) << 6) + word;
            if offset <= used && (slots << 4) <= used - offset {
                own = true;
            }
        }
        i += 1;
    }
    if !own && !gpu_capture_fact(facts, 0, first, 1, 1) {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    if gpu_capture_read_range(first, word, slots << 4) == 0
        && !gpu_capture_resource(
            facts,
            0,
            GPU_CAPTURE_TAG_BUFFER,
            first,
            1,
            GPU_CAPTURE_READ | GPU_CAPTURE_PREIMAGE,
        )
    {
        return false;
    }
    if slot >= 80 {
        GPU_CAPTURE_CONTROL[17u16] |= 1 << ((slot - 80) >> 2);
    } else {
        let mut i: u16 = 0;
        while i < slots {
            let index = slot + i - 66;
            GPU_CAPTURE_LOCAL[index] = first;
            GPU_CAPTURE_LOCAL[index + 14] = word + i * 16;
            GPU_CAPTURE_LOCAL[index + 28] = 1;
            i += 1;
        }
    }
    true
}
fn gpu_capture_meshlet(facts: Array<u16>, low: u16, high: u16) -> bool {
    let first = (low >> 12) | (high << 4);
    let sectors = ((low >> 1) & 7) + 2;
    if low & 4080 != 0
        || gpu_capture_written(first, 1)
        || !gpu_capture_resource(
            facts,
            1,
            GPU_CAPTURE_TAG_MESH,
            first,
            sectors,
            GPU_CAPTURE_READ | GPU_CAPTURE_PREIMAGE,
        )
    {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    let counts = gpu_capture_physical_word(first, 0);
    let uniform = gpu_capture_physical_word(first, 1) & 63;
    let mode = gpu_capture_physical_word(first, 4);
    if uniform < 20 || uniform > 23 {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    let mut needed: u16 = 1 << (uniform - 20);
    if counts & 32512 != 0 && mode & 16 == 0 {
        needed |= 16;
    }
    let slot = low & 1;
    GPU_CAPTURE_CONTROL[18 + slot] = needed;
    GPU_CAPTURE_CONTROL[16u16] |= 1 << slot;
    GPU_CAPTURE_CONTROL[1u16] == 0
}
fn gpu_capture_texture(facts: Array<u16>) -> bool {
    let first = GPU_CAPTURE_CONTROL[20u16];
    let flags = GPU_CAPTURE_CONTROL[21u16];
    let exponent = flags & 15;
    if first & 1 != 0 || exponent > 10 || flags & 65280 != 0 || ((flags >> 5) & 3) > 2 {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    let mut sectors: u16 = 0;
    if flags & 128 != 0 {
        if exponent != 9 || flags & 16 != 0 || flags & 64 != 0 {
            return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
        }
        sectors = 1600;
    } else {
        let mut level: u16 = 0;
        while level <= exponent {
            let mut size: u16 = 4;
            if level > 4 {
                size = 4 << ((level - 4) << 1);
            }
            if flags & 16 == 0 {
                sectors = size;
            } else {
                sectors += size;
            }
            level += 1;
        }
    }
    if !gpu_capture_resource(
        facts,
        2,
        GPU_CAPTURE_TAG_TEXTURE,
        first,
        sectors,
        GPU_CAPTURE_READ | GPU_CAPTURE_PREIMAGE,
    ) {
        return false;
    }
    // Fixed-view allocation padding has independent read-only provenance.
    if flags & 128 != 0 {
        let mut i: u16 = 0;
        while i < GPU_CAPTURE_SOURCE[9u16] {
            let at = i * 4;
            if facts[at] == first
                && facts[at + 2] == 2
                && facts[at + 1] > 1600
                && gpu_capture_range_add(
                    GPU_CAPTURE_TAG_TEXTURE,
                    GPU_CAPTURE_READ | GPU_CAPTURE_PREIMAGE,
                    first + 1600,
                    (facts[at + 1] - 1600) << 7,
                    (facts[at + 1] - 1600) >> 9,
                ) == 0
            {
                return false;
            }
            i += 1;
        }
    }
    true
}
fn gpu_capture_ui(facts: Array<u16>, low: u16, high: u16) -> bool {
    let local = low & 127;
    if local < 66 || local >= 80 || GPU_CAPTURE_CONTROL[14u16] == 0 {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    let a = gpu_capture_descriptor_word(local - 66, 0);
    let b = gpu_capture_descriptor_word(local - 66, 1);
    let mut padding: u16 = 2;
    while padding < 16 {
        if gpu_capture_descriptor_word(local - 66, padding) != 0 {
            return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
        }
        padding += 1;
    }
    let mut first: u16 = 0;
    let mut last: u16 = 749;
    if high & 2048 != 0 {
        first = (low >> 7) | ((high & 1) << 9);
        last = (high >> 1) & 1023;
        if first > last || last >= 750 {
            return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
        }
    } else if low & 65408 != 0 || high != 0xb000 {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    if b & 65408 != 0 {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    let grid = a;
    let atlas = (b & 127) << 9;
    let mut atlas_sectors: u16 = 0;
    let mut fact: u16 = 0;
    while fact < GPU_CAPTURE_SOURCE[9u16] {
        let at = fact * 4;
        if facts[at + 2] == 3 && facts[at + 3] & 1 != 0 && atlas >= facts[at] {
            let offset = atlas - facts[at];
            if offset < facts[at + 1] && facts[at + 1] - offset > atlas_sectors {
                atlas_sectors = facts[at + 1] - offset;
            }
        }
        fact += 1;
    }
    let count = (last >> 7) - (first >> 7) + 1;
    if !gpu_capture_extent(grid, (last >> 7) + 1)
        || !gpu_capture_resource(
            facts,
            4,
            GPU_CAPTURE_TAG_UI,
            grid + (first >> 7),
            count,
            GPU_CAPTURE_READ | GPU_CAPTURE_PREIMAGE,
        )
        || !gpu_capture_resource(
            facts,
            3,
            GPU_CAPTURE_TAG_ATLAS,
            atlas,
            atlas_sectors,
            GPU_CAPTURE_READ | GPU_CAPTURE_PREIMAGE,
        )
    {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    // Every non-FF glyph fits a 510-sector atlas. For smaller allocations,
    // certify only the selected bytes; masked pair neighbors are not read.
    if atlas_sectors < 510 {
        if gpu_capture_written(grid + (first >> 7), count) {
            return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
        }
        let mut cell = first;
        while cell <= last {
            let word = gpu_capture_physical_word(grid, cell >> 1);
            let glyph = if cell & 1 == 0 { word & 255 } else { word >> 8 };
            if glyph != 255 && (glyph + 1) * 2 > atlas_sectors {
                return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
            }
            cell += 1;
        }
    }
    gpu_capture_base(GPU_CAPTURE_H_UI_BASE, grid + (first >> 7));
    gpu_capture_pair(GPU_CAPTURE_H_UI_BYTES, count << 7, 0);
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_FLAGS] |= GPU_CAPTURE_HAS_UI;
    GPU_CAPTURE_CONTROL[16u16] = 0;
    GPU_CAPTURE_CONTROL[1u16] == 0
}
fn gpu_capture_memcpy(facts: Array<u16>, low: u16, high: u16) -> bool {
    let local = low & 127;
    if high != 0xc000 || low & 65408 != 0 || local < 66 || local >= 80 {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    let a = gpu_capture_descriptor_word(local - 66, 0);
    let b = gpu_capture_descriptor_word(local - 66, 1);
    let c = gpu_capture_descriptor_word(local - 66, 2);
    let source = (a & 16383) << 2;
    let destination = ((a >> 12) & 12) | (b << 4);
    let count = (((b >> 12) | ((c & 127) << 4)) + 1) << 2;
    if c >> 7 != 0
        || gpu_capture_descriptor_word(local - 66, 3) != 0
        || !gpu_capture_extent(source, count)
        || !gpu_capture_extent(destination, count)
        || gpu_capture_overlap(source, count, destination, count)
        || gpu_capture_overlap(
            destination,
            count,
            GPU_CAPTURE_SOURCE[2u16],
            GPU_CAPTURE_SOURCE[3u16] >> 6,
        )
        || !gpu_capture_resource(
            facts,
            0,
            GPU_CAPTURE_TAG_BUFFER,
            source,
            count,
            GPU_CAPTURE_READ | GPU_CAPTURE_PREIMAGE,
        )
        || !gpu_capture_resource(
            facts,
            0,
            GPU_CAPTURE_TAG_BUFFER,
            destination,
            count,
            GPU_CAPTURE_WRITE | GPU_CAPTURE_PREIMAGE,
        )
    {
        return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
    }
    GPU_CAPTURE_CONTROL[1u16] == 0
}
// Ordered data-dependency projection only: no second GPU owner or scheduler.
fn gpu_capture_commands(facts: Array<u16>) -> bool {
    let mut word: u16 = 0;
    let valid = GPU_CAPTURE_SOURCE[4u16];
    while word < valid {
        let low = gpu_capture_read_value(0, word, 0);
        let high = gpu_capture_read_value(0, word + 1, 0);
        let opcode = high >> 12;
        if GPU_CAPTURE_CONTROL[1u16] != 0 {
            return false;
        }
        if opcode == 1 {
            if !gpu_capture_local_dma(facts, low, high) {
                return false;
            }
        } else if opcode == 2 {
            if !gpu_capture_meshlet(facts, low, high) {
                return false;
            }
        } else if opcode == 3 {
            if high != 0x3000
                || low > 1
                || GPU_CAPTURE_CONTROL[12u16] == 0
                || GPU_CAPTURE_CONTROL[13u16] == 0
                || GPU_CAPTURE_CONTROL[14u16] == 0
                || GPU_CAPTURE_CONTROL[15u16] == 0
                || GPU_CAPTURE_CONTROL[16u16] & (1 << low) == 0
                || GPU_CAPTURE_CONTROL[17u16] & GPU_CAPTURE_CONTROL[18 + low] & 15 == 0
            {
                return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
            }
            if GPU_CAPTURE_CONTROL[18 + low] & 16 != 0 && !gpu_capture_texture(facts) {
                return false;
            }
            let depth_flags = GPU_CAPTURE_RANGES[(GPU_CAPTURE_CONTROL[15u16] - 1) * 8];
            if (depth_flags >> 5) & 3 != (GPU_CAPTURE_CONTROL[12u16] >> 2) & 3 {
                return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
            }
        } else if opcode == 5 || opcode == 6 {
            if high & 4095 != 0 || low < 66 || low >= 80 || GPU_CAPTURE_LOCAL[low - 66 + 28] == 0 {
                return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
            }
            if opcode == 5 {
                if low >= 79 || GPU_CAPTURE_LOCAL[low - 66 + 29] == 0 {
                    return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
                }
                let far_low = gpu_capture_descriptor_word(low - 66, 0);
                let far_high = gpu_capture_descriptor_word(low - 66, 1);
                if !gpu_depth_native_valid(far_low, far_high)
                    || (GPU_CAPTURE_HEADER[GPU_CAPTURE_H_COMMAND_ABI]
                        == GPU_CAPTURE_UNIFORM_RTT400_COMMAND_ABI
                        && far_high & 768 != 0)
                {
                    return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
                }
                GPU_CAPTURE_CONTROL[12u16] = 1 | (((far_high >> 8) & 3) << 2);
            } else {
                GPU_CAPTURE_CONTROL[13u16] = 1;
                GPU_CAPTURE_CONTROL[20u16] = gpu_capture_descriptor_word(low - 66, 0);
                GPU_CAPTURE_CONTROL[21u16] = gpu_capture_descriptor_word(low - 66, 1);
            }
        } else if opcode == 10 {
            if high != 0xa000 || low > 127 || !gpu_capture_target(facts, low) {
                return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
            }
        } else if opcode == 11 {
            if !gpu_capture_ui(facts, low, high) {
                return false;
            }
        } else if opcode == 12 {
            if !gpu_capture_memcpy(facts, low, high) {
                return false;
            }
        } else if opcode == 8 {
            if high != 0x8000 && high != 0x8001 {
                return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
            }
            let range = GPU_CAPTURE_CONTROL[14 + (high & 1)];
            if range == 0 {
                return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
            }
            GPU_CAPTURE_RANGES[(range - 1) * 8] |= GPU_CAPTURE_FULL_CLEAR;
        } else if opcode == 4 {
            if high != 0x4000 || low > 3 {
                return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
            }
        } else if opcode == 7 || opcode == 9 || opcode == 0 {
            if low != 0 || high & 4095 != 0 {
                return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
            }
        } else if opcode == 15 {
            if low != 0 || high != 0xf000 || word + 2 != valid {
                return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
            }
        } else {
            return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
        }
        if word + 2 == valid && opcode != 15 {
            return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
        }
        word += 2;
    }
    gpu_capture_finish_target();
    while word < GPU_CAPTURE_SOURCE[5u16] {
        if gpu_capture_read_value(0, word, 0) != 0 {
            return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
        }
        word += 1;
    }
    GPU_CAPTURE_CONTROL[1u16] == 0
}
fn gpu_capture_begin(source: Array<u16>, facts: Array<u16>) -> bool {
    if GPU_CAPTURE_CONFIG_ENABLED == 0 {
        return true;
    }
    if GPU_CAPTURE_CONTROL[0u16] != 0 {
        return gpu_capture_reject(GPU_CAPTURE_GATE_ERROR);
    }
    GPU_CAPTURE_CONTROL[0u16] = 1;
    GPU_CAPTURE_CONTROL[1u16] = 0;
    GPU_CAPTURE_CONTROL[6u16] = 0;
    GPU_CAPTURE_CONTROL[11u16] = 0;
    let mut i: u16 = 0;
    while i < 42 {
        GPU_CAPTURE_LOCAL[i] = 0;
        i += 1;
    }
    i = 12;
    while i < 24 {
        GPU_CAPTURE_CONTROL[i] = 0;
        i += 1;
    }
    i = 0;
    while i < 12 {
        GPU_CAPTURE_SOURCE[i] = source[i];
        i += 1;
    }
    GPU_CAPTURE_CONTROL[10u16] = source[1u16];
    if source[0u16] != 1
        || source[1u16] > 1
        || (source[10u16] != GPU_CAPTURE_UNIFORM_RTT400_COMMAND_ABI
            && source[10u16] != GPU_CAPTURE_RTT400_COMMAND_ABI)
        || source[11u16] != 2
        || GPU_CAPTURE_CONFIG_LAYOUT_ABI != 2
        || source[9u16] > 64
    {
        return gpu_capture_reject(GPU_CAPTURE_IDENTITY_ERROR);
    }
    let capacity = source[3u16];
    let valid = source[4u16];
    let padded = source[5u16];
    let push = source[7u16];
    if capacity == 0
        || capacity > 32768
        || capacity & 63 != 0
        || push == 0
        || push > 32768
        || push & 63 != 0
        || source[8u16] > push
        || valid < 2
        || valid & 1 != 0
        || padded != ((valid + 63) & 65472)
        || padded > capacity
        || valid > capacity
        || !gpu_capture_extent(source[2u16], capacity >> 6)
        || !gpu_capture_extent(source[6u16], push >> 6)
        || gpu_capture_overlap(source[2u16], capacity >> 6, source[6u16], push >> 6)
    {
        return gpu_capture_reject(GPU_CAPTURE_BOUNDS);
    }
    i = 0;
    while i < source[9u16] {
        let at = i * 4;
        if !gpu_capture_extent(facts[at], facts[at + 1])
            || facts[at + 2] == 0
            || facts[at + 2] > 7
            || (facts[at + 2] == 6 && !gpu_depth_fact_flags_valid(facts[at + 3]))
            || (facts[at + 2] != 6 && (facts[at + 3] == 0 || facts[at + 3] > 3))
        {
            return gpu_capture_reject(GPU_CAPTURE_DEPENDENCY);
        }
        i += 1;
    }
    if GPU_CAPTURE_CONTROL[2u16] == 65535 && GPU_CAPTURE_CONTROL[3u16] == 65535
        || GPU_CAPTURE_CONTROL[4u16] == 65535 && GPU_CAPTURE_CONTROL[5u16] == 65535
    {
        return gpu_capture_reject(GPU_CAPTURE_IDENTITY_ERROR);
    }
    GPU_CAPTURE_CONTROL[2u16] += 1;
    if GPU_CAPTURE_CONTROL[2u16] == 0 {
        GPU_CAPTURE_CONTROL[3u16] += 1;
    }
    i = 0;
    while i < 64 {
        GPU_CAPTURE_HEADER[i] = 0;
        i += 1;
    }
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_VERSION] = GPU_CAPTURE_EXPLICIT_VERSION;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_WORDS] = 64;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_FLAGS] = GPU_CAPTURE_PUBLISHED;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_BINDINGS] = GPU_CAPTURE_BINDINGS_SELF_CONTAINED;
    gpu_capture_pair(
        GPU_CAPTURE_H_TOKEN,
        GPU_CAPTURE_CONTROL[2u16],
        GPU_CAPTURE_CONTROL[3u16],
    );
    let submission = GPU_CAPTURE_CONTROL[4u16] + 1;
    let mut upper = GPU_CAPTURE_CONTROL[5u16];
    if submission == 0 {
        upper += 1;
    }
    gpu_capture_pair(GPU_CAPTURE_H_SUBMISSION, submission, upper);
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_SLOT] = source[1u16];
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_COUNT] = (valid >> 1) - 1;
    gpu_capture_base(GPU_CAPTURE_H_COMMAND_BASE, source[2u16]);
    gpu_capture_pair(GPU_CAPTURE_H_COMMAND_BYTES, valid << 1, valid >> 15);
    gpu_capture_pair(
        GPU_CAPTURE_H_COMMAND_PADDED_BYTES,
        padded << 1,
        padded >> 15,
    );
    if source[8u16] != 0 {
        gpu_capture_base(GPU_CAPTURE_H_OBJECT_BASE, source[6u16]);
        gpu_capture_pair(
            GPU_CAPTURE_H_OBJECT_BYTES,
            source[8u16] << 1,
            source[8u16] >> 15,
        );
    }
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_RANGE_WORDS] = 8;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_RANGE_VERSION] = GPU_CAPTURE_RANGE_VERSION;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_LAYOUT_ABI] = 2;
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_COMMAND_ABI] = source[10u16];
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_MANIFEST_WORDS] = 16;
    i = 0;
    while i < 16 {
        GPU_CAPTURE_HEADER[48 + i] = gpu_capture_manifest_word(i);
        i += 1;
    }
    if gpu_capture_range_add(
        GPU_CAPTURE_TAG_COMMANDS,
        GPU_CAPTURE_READ,
        source[2u16],
        padded << 1,
        padded >> 15,
    ) == 0
    {
        return false;
    }
    if source[8u16] != 0
        && gpu_capture_range_add(
            GPU_CAPTURE_TAG_OBJECTS,
            GPU_CAPTURE_READ,
            source[6u16],
            source[8u16] << 1,
            source[8u16] >> 15,
        ) == 0
    {
        return false;
    }
    if !gpu_capture_commands(facts) {
        return false;
    }
    GPU_CAPTURE_HEADER[GPU_CAPTURE_H_RANGE_COUNT] = GPU_CAPTURE_CONTROL[11u16];
    gpu_capture_health() == GPU_CAPTURE_OK
}
fn gpu_capture_gate() -> u16 {
    if GPU_CAPTURE_CONFIG_ENABLED == 0 {
        return GPU_CAPTURE_CONTINUE;
    }
    if gpu_capture_health() != 0 {
        return GPU_CAPTURE_FATAL;
    }
    let action = profiler_gate(GPU_CAPTURE_HEADER.as_array());
    if action > GPU_CAPTURE_RESUMED {
        gpu_capture_fail(GPU_CAPTURE_GATE_ERROR);
        return GPU_CAPTURE_FATAL;
    }
    if gpu_capture_health() != 0 {
        return GPU_CAPTURE_FATAL;
    }
    action
}
// Pending request bit0, lazy input-baseline reset bit1. No frame scheduler.
static GPU_CAPTURE_REQUEST: Buf<u16, 1> = Buf::new([0; 1]);
fn gpu_snapshot_reset() {
    GPU_CAPTURE_REQUEST[0u16] = 2;
}
fn gpu_snapshot_request() {
    if GPU_CAPTURE_CONFIG_ENABLED != 0 {
        GPU_CAPTURE_REQUEST[0u16] |= 1;
    }
}
fn gpu_snapshot_poll() {
    if GPU_CAPTURE_CONFIG_ENABLED != 0 {
        if GPU_CAPTURE_REQUEST[0u16] & 2 != 0 {
            gpu_capture_input_reset();
            GPU_CAPTURE_REQUEST[0u16] &= 1;
        }
        if gpu_capture_input_poll() != 0 {
            gpu_snapshot_request();
        }
    }
}
fn gpu_snapshot_take() -> bool {
    if GPU_CAPTURE_CONFIG_ENABLED == 0 || GPU_CAPTURE_REQUEST[0u16] & 1 == 0 {
        return false;
    }
    GPU_CAPTURE_REQUEST[0u16] &= 2;
    gpu_capture_input_arm();
    true
}
// Only the common driver calls this, once after an actual successful START.
fn gpu_capture_started() {
    GPU_CAPTURE_CONTROL[4u16] += 1;
    if GPU_CAPTURE_CONTROL[4u16] == 0 {
        GPU_CAPTURE_CONTROL[5u16] += 1;
    }
}
// Lease-only finalization must never read a stale prospective header count.
fn gpu_capture_submitted() {
    if GPU_CAPTURE_CONFIG_ENABLED != 0 {
        GPU_CAPTURE_CONTROL[0u16] = 0;
        GPU_CAPTURE_CONTROL[6u16] = 0;
    }
}
