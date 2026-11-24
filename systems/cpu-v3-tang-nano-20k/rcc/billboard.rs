//! Sequential Billboard batches; ordering and draw state belong to the caller.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;

const BILLBOARD_QUADS: u16 = 16;
const BILLBOARD_VERTEX_WORDS: u16 = 24;

#[derive(Copy, Clone)]
pub struct BillboardVertex {
    /// Signed offsets in 1/512 world units; UV uses unsigned Q0.16.
    pub x: i16,
    pub y: i16,
    pub u: u16,
    pub v: u16,
}

pub struct BillboardWriter {
    memory: GpuMemory,
    capacity: u16,
    next_sector: u16,
    batches: u16,
    quads: u16,
    origin: Vec3,
    quant_shift: u16,
    recording: bool,
}

// Quantize a nonnegative Q16.16 displacement to delta10, with ties to even.
// The sentinel rejects overflow before truncating the quotient to 16 bits.
fn billboard_delta(value: fix32, origin: fix32, shift: u16) -> u16 {
    if value == origin {
        return 0;
    }
    if value < origin {
        return 65535;
    }
    let delta = value - origin;
    if delta < fix32::zero() {
        return 65535;
    }
    let low = delta.lo_bits();
    let high = delta.hi_bits();
    let mut code = high;
    let mut above = false;
    let mut tie = false;
    if shift == 0 {
        if high != 0 {
            return 65535;
        }
        code = low;
    } else if shift < 16 {
        if high >> shift != 0 {
            return 65535;
        }
        code = (low >> shift) | (high << (16 - shift));
        let half = 1u16 << (shift - 1);
        let tail = low & ((half - 1) | half);
        above = tail > half;
        tie = tail == half;
    } else if shift == 16 {
        above = low > 32768;
        tie = low == 32768;
    } else {
        let bits = shift - 16;
        code = high >> bits;
        let half = 1u16 << (bits - 1);
        let tail = high & ((half - 1) | half);
        above = tail > half || (tail == half && low != 0);
        tie = tail == half && low == 0;
    }
    if code > 1023 {
        return 65535;
    }
    if above || (tie && code & 1 != 0) {
        code += 1;
    }
    code
}

impl BillboardWriter {
    /// Initialize at most 32 batch directory entries from a canonical 256-byte
    /// quad. Caller owns disjoint template/storage and discards retired readers.
    pub unsafe fn new(memory: GpuMemory, template: GpuMemory, capacity: u16) -> Self {
        assert!(capacity > 0 && capacity <= 32);
        assert!(memory.sector_count() >= 3 && template.sector_count() == 2);
        // The template transfer and directory construction never overlap.
        // Reuse their staging storage instead of keeping both buffers live.
        let mut scratch: Buf<u16, 128> = Buf::new([0; 128]);
        unsafe {
            template.read_words(0, scratch.as_array(), 128);
            memory.write_words(64, scratch.as_array(), 128);
        }
        let mut at: u16 = 0;
        while at < capacity {
            scratch[at * 2] = memory.first_sector() + 1;
            scratch[at * 2 + 1] = 1;
            at += 1;
        }
        unsafe {
            memory.write_words(0, scratch.as_array(), capacity * 2);
        }
        Self {
            memory,
            capacity,
            next_sector: 1,
            batches: 0,
            quads: 0,
            origin: Vec3::zero(),
            quant_shift: 0,
            recording: false,
        }
    }

    /// Register once and refresh after rebuilding. Draw only batch_count entries;
    /// the rest are dormant directory entries, not visible quads.
    pub fn mesh_desc(&self) -> MeshDesc {
        MeshDesc::from_memory(self.memory, 0, self.capacity)
    }
    pub fn batch_count(&self) -> u16 {
        self.batches
    }
    pub fn used_bytes(&self) -> u16 {
        self.next_sector * 128
    }

    /// Rebuild only after retiring executions and discarding old recordings.
    pub fn reset(&mut self) {
        assert!(!self.recording);
        self.next_sector = 1;
        self.batches = 0;
    }

    /// Begin one application-selected compatible run with a shared origin and
    /// power-of-two Q16.16 quantization step. No sorting or automatic flush.
    pub fn begin(&mut self, origin: Vec3, quant_shift: u16) {
        assert!(!self.recording && self.batches < self.capacity);
        assert!(quant_shift <= 31);
        assert!(self.next_sector <= self.memory.sector_count() - 2);
        self.origin = origin;
        self.quant_shift = quant_shift;
        self.quads = 0;
        self.recording = true;
    }

    /// Append in draw order. False leaves bytes/cursors unchanged when capacity
    /// or center range does not fit. Supply four readable corners, with no live
    /// recorded reader of this storage; caller chooses the next batch boundary.
    pub unsafe fn append(&mut self, center: Vec3, vertices: Array<BillboardVertex>) -> bool {
        assert!(self.recording);
        let count = self.quads + 1;
        let payload = BILLBOARD_VERTEX_WORDS * count + ((2 * count + 2) / 3) * 4;
        let sectors = 1 + (payload + 63) / 64;
        if count > BILLBOARD_QUADS || sectors > self.memory.sector_count() - self.next_sector {
            return false;
        }
        let x = billboard_delta(center.x, self.origin.x, self.quant_shift);
        let y = billboard_delta(center.y, self.origin.y, self.quant_shift);
        let z = billboard_delta(center.z, self.origin.z, self.quant_shift);
        if x > 1023 || y > 1023 || z > 1023 {
            return false;
        }
        let mut words: Buf<u16, 24> = Buf::new([0; 24]);
        let mut corner: u16 = 0;
        while corner < 4 {
            let vertex = vertices[corner];
            debug_assert!(vertex.x >= -512 && vertex.x <= 511);
            debug_assert!(vertex.y >= -512 && vertex.y <= 511);
            let dx = vertex.x as u16 & 1023;
            let dy = vertex.y as u16 & 1023;
            words[corner * 6] = x | (y << 10);
            words[corner * 6 + 1] = (y >> 6) | (z << 4);
            words[corner * 6 + 2] = dx | (dy << 10);
            words[corner * 6 + 3] = dy >> 6;
            words[corner * 6 + 4] = vertex.u;
            words[corner * 6 + 5] = vertex.v;
            corner += 1;
        }
        unsafe {
            self.memory.write_words(
                (self.next_sector + 1) * 64 + self.quads * BILLBOARD_VERTEX_WORDS,
                words.as_array(),
                BILLBOARD_VERTEX_WORDS,
            );
        }
        self.quads = count;
        true
    }

    /// Publish the nonempty batch and return its directory index. Refresh the
    /// registered Mesh directory once after all batches have been written.
    pub unsafe fn finish(&mut self) -> u16 {
        assert!(self.recording && self.quads != 0);
        let triangles = self.quads * 2;
        let index_words = ((triangles + 2) / 3) * 4;
        let payload_words = self.quads * BILLBOARD_VERTEX_WORDS + index_words;
        let payload_sectors = (payload_words + 63) / 64;
        let mut indices: Buf<u16, 44> = Buf::new([0; 44]);
        let mut triangle: u16 = 0;
        while triangle < triangles {
            let base = (triangle >> 1) * 4;
            let mut b = base + 1;
            let mut c = base + 2;
            if triangle & 1 != 0 {
                b += 1;
                c += 1;
            }
            let low = base | (b << 6) | (c << 12);
            let high = c >> 4;
            let lane = triangle % 3;
            let at = (triangle / 3) * 4 + lane;
            let shift = lane * 2;
            indices[at] |= low << shift;
            if shift == 0 {
                indices[at + 1] |= high;
            } else {
                indices[at + 1] |= (low >> (16 - shift)) | (high << shift);
            }
            triangle += 1;
        }
        let metadata: Buf<u16, 16> = Buf::new([
            self.quads * 4 | (triangles << 8),
            20 | ((self.quads * 6) << 8),
            0,
            0,
            1,
            0,
            0,
            0,
            self.origin.x.lo_bits(),
            self.origin.x.hi_bits(),
            self.origin.y.lo_bits(),
            self.origin.y.hi_bits(),
            self.origin.z.lo_bits(),
            self.origin.z.hi_bits(),
            self.quant_shift,
            0,
        ]);
        let entry: Buf<u16, 2> = Buf::new([
            self.memory.first_sector() + self.next_sector,
            payload_sectors,
        ]);
        unsafe {
            self.memory.write_words(
                (self.next_sector + 1) * 64 + self.quads * BILLBOARD_VERTEX_WORDS,
                indices.as_array(),
                index_words,
            );
            self.memory
                .write_words(self.next_sector * 64, metadata.as_array(), 16);
            self.memory
                .write_words(self.batches * 2, entry.as_array(), 2);
        }
        let batch = self.batches;
        self.batches += 1;
        self.next_sector += 1 + payload_sectors;
        self.recording = false;
        batch
    }
}
