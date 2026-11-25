//! Application-selected compatible runs, written after transparent sorting.
use crate::billboard::{BillboardVertex, BillboardWriter};
use crate::boot_resources::{
    SCENE_ANIMATION, SCENE_FRAME_STARTS, SCENE_LAYERS, SCENE_QUAD, SCENE_SPRITE_CAPACITY,
    SCENE_TRANSPARENT_FIRST,
};
use crate::dsl_rt::*;
use crate::gpu_api::Mesh;

const SPRITE_LIMIT: u16 = SCENE_SPRITE_CAPACITY;
// 0: kind/shade; 1..6: XYZ Q16.16; 7..8: source half-extents;
// 9..10: signed Q12 sin/cos; 11..12: packed u0/v0 and u1/v1 texel coordinates.
const PARAMETERS: u16 = 13;

pub fn sprite_writer(storage: GpuMemory) -> BillboardWriter {
    unsafe { BillboardWriter::new(storage, SCENE_QUAD, SPRITE_LIMIT) }
}

fn round_even(value: fix32) -> i16 {
    let mut whole = value.hi_bits() as i16;
    let fraction = value.lo_bits();
    if fraction > 32768 || (fraction == 32768 && whole & 1 != 0) {
        whole += 1;
    }
    whole
}

fn q8_center(low: u16, high: u16) -> u16 {
    let mut value = (low >> 8) | (high << 8);
    let fraction = low & 255;
    if fraction > 128 || (fraction == 128 && value & 1 != 0) {
        value += 1;
    }
    value
}

fn uv(value: u16, smoke: bool) -> u16 {
    let mut extent: u16 = 128;
    let mut shift: u16 = 9;
    if smoke {
        extent = 64;
        shift = 10;
    }
    if value >= extent {
        return 65535;
    }
    value << shift
}

fn write_sprite(writer: &mut BillboardWriter, parameters: Array<u16>) -> bool {
    let width = fix32::from_words(parameters[7u16], 0);
    let height = fix32::from_words(parameters[8u16], 0);
    let sine = fix32::from_int(parameters[9u16] as i16);
    let cosine = fix32::from_int(parameters[10u16] as i16);
    let smoke = parameters[0u16] >> 8 == 3;
    let mut vertices: Buf<BillboardVertex, 4> = Buf::new(
        [BillboardVertex {
            x: 0,
            y: 0,
            u: 0,
            v: 0,
        }; 4],
    );
    let mut corner: u16 = 0;
    while corner < 4 {
        let mut x = -width;
        let mut y = height;
        let mut u = parameters[11u16] & 255;
        let mut v = parameters[11u16] >> 8;
        if corner == 1 || corner == 2 {
            x = width;
            u = parameters[12u16] & 255;
        }
        if corner >= 2 {
            y = -height;
            v = parameters[12u16] >> 8;
        }
        // Source extents are 1/1024, scene scale is 1/8, rotation is Q12,
        // and native offsets are 1/512: the combined divisor is 65536.
        vertices[corner] = BillboardVertex {
            x: round_even(-(x * cosine + y * sine)),
            y: round_even(-x * sine + y * cosine),
            u: uv(u, smoke),
            v: uv(v, smoke),
        };
        corner += 1;
    }
    let center = sprite_center(parameters);
    unsafe { writer.append(center, vertices.as_array()) }
}

fn sprite_center(parameters: Array<u16>) -> Vec3 {
    Vec3::new(
        fix32::from_words(parameters[1u16], parameters[2u16]),
        fix32::from_words(parameters[3u16], parameters[4u16]),
        fix32::from_words(parameters[5u16], parameters[6u16]),
    )
}

fn sprite_parameters(parameters: Array<u16>, slot: u16) -> Array<u16> {
    unsafe {
        parameters
            .as_ptr()
            .add((slot * PARAMETERS) as i16)
            .as_u16_array()
    }
}

pub fn sprite_frame(frame: u16, mut layers: Array<u16>, parameters: Array<u16>) -> u16 {
    let mut n: u16 = 0;
    while n < 8 {
        layers[n] = SCENE_LAYERS[n];
        n += 1;
    }
    let first = SCENE_FRAME_STARTS[frame];
    let count = SCENE_FRAME_STARTS[frame + 1] - first;
    assert!(count <= SPRITE_LIMIT);
    let animation = SCENE_ANIMATION;
    let mut slot: u16 = 0;
    while slot < count {
        let data = sprite_parameters(parameters, slot);
        unsafe {
            animation.read_words((first + slot) * PARAMETERS, data, PARAMETERS);
        }
        let at = (slot + 2) * 4;
        layers[at] = data[0u16];
        n = 0;
        while n < 3 {
            layers[at + 1 + n] = q8_center(data[1 + n * 2], data[2 + n * 2]);
            n += 1;
        }
        slot += 1;
    }
    count + 2
}

struct SpriteBounds {
    min: Vec3,
    max: Vec3,
}

fn extend_bounds(bounds: &mut SpriteBounds, center: Vec3) -> bool {
    let mut min = bounds.min;
    let mut max = bounds.max;
    if center.x < min.x {
        min.x = center.x;
    }
    if center.y < min.y {
        min.y = center.y;
    }
    if center.z < min.z {
        min.z = center.z;
    }
    if center.x > max.x {
        max.x = center.x;
    }
    if center.y > max.y {
        max.y = center.y;
    }
    if center.z > max.z {
        max.z = center.z;
    }
    // Captured centers share a 1/1024 lattice. Delta10 at shift6 covers
    // 1023/1024 world units without adding any source-center quantization.
    let limit = fix32::from_words(65472, 0);
    if max.x - min.x > limit || max.y - min.y > limit || max.z - min.z > limit {
        return false;
    }
    bounds.min = min;
    bounds.max = max;
    true
}

/// Preserve sorted order, splitting only at state, range or 16-quad boundaries.
/// Each plan entry is (directory index, kind/shade); bit15 selects static Mesh.
pub fn prepare_batches(
    writer: &mut BillboardWriter,
    parameters: Array<u16>,
    layers: Array<u16>,
    order: Array<u16>,
    count: u16,
    mut plan: Array<u16>,
) -> u16 {
    writer.reset();
    let mut at: u16 = 0;
    let mut draws: u16 = 0;
    while at < count {
        let layer = order[at];
        let entry = layers[layer * 4];
        plan[draws * 2 + 1] = entry;
        if layer < 2 {
            plan[draws * 2] = 32768 | (SCENE_TRANSPARENT_FIRST + layer);
            at += 1;
        } else {
            let center = sprite_center(sprite_parameters(parameters, layer - 2));
            let mut bounds = SpriteBounds {
                min: center,
                max: center,
            };
            let mut end = at + 1;
            while end < count && end - at < 16 {
                let next = order[end];
                if next < 2 || layers[next * 4] != entry {
                    break;
                }
                let point = sprite_center(sprite_parameters(parameters, next - 2));
                if !extend_bounds(&mut bounds, point) {
                    break;
                }
                end += 1;
            }
            writer.begin(bounds.min, 6);
            while at < end {
                let row = sprite_parameters(parameters, order[at] - 2);
                assert!(write_sprite(writer, row));
                at += 1;
            }
            plan[draws * 2] = unsafe { writer.finish() };
        }
        draws += 1;
    }
    draws
}

// Captured data and the encoded plan belong to the application. Camera-only
// frames can reuse native geometry when their sorted order is unchanged.
pub struct SpriteStream {
    layers: Buf<u16, 48>,
    previous_order: Buf<u16, 12>,
    plan: Buf<u16, 24>,
    frame: u16,
    count: u16,
    draws: u16,
}
impl SpriteStream {
    pub fn new() -> Self {
        Self {
            layers: Buf::new([0; 48]),
            previous_order: Buf::new([0; 12]),
            plan: Buf::new([0; 24]),
            frame: 65535,
            count: 0,
            draws: 0,
        }
    }
    pub fn plan(&self) -> Array<u16> {
        self.plan.as_array()
    }
    pub fn prepare(
        &mut self,
        writer: &mut BillboardWriter,
        mesh: &mut Mesh,
        frame: u16,
        direction: Vec3,
    ) -> u16 {
        if frame != self.frame {
            return self.rebuild(writer, mesh, frame, direction);
        }
        let mut order: Buf<u16, 12> = Buf::new([0; 12]);
        self.sorted_order(direction, order.as_array());
        let mut at: u16 = 0;
        while at < self.count {
            if order[at] != self.previous_order[at] {
                return self.rebuild(writer, mesh, frame, direction);
            }
            at += 1;
        }
        self.draws
    }

    fn sorted_order(&self, direction: Vec3, mut order: Array<u16>) {
        let mut depths: Buf<i16, 12> = Buf::new([0; 12]);
        let mut at: u16 = 0;
        while at < self.count {
            order[at] = at;
            depths[at] = layer_depth(at, &direction, self.layers.as_array());
            at += 1;
        }
        sort_layer_order(order, depths.as_array(), self.count);
    }

    fn rebuild(
        &mut self,
        writer: &mut BillboardWriter,
        mesh: &mut Mesh,
        frame: u16,
        direction: Vec3,
    ) -> u16 {
        // Source rows are needed only while rebuilding. Keep them out of the
        // persistent stream and the main loop's lifetime.
        let parameters: Buf<u16, 130> = Buf::new([0; 130]);
        let mut order: Buf<u16, 12> = Buf::new([0; 12]);
        self.count = sprite_frame(frame, self.layers.as_array(), parameters.as_array());
        self.frame = frame;
        self.sorted_order(direction, order.as_array());
        let mut at: u16 = 0;
        while at < self.count {
            self.previous_order[at] = order[at];
            at += 1;
        }
        self.draws = prepare_batches(
            writer,
            parameters.as_array(),
            self.layers.as_array(),
            order.as_array(),
            self.count,
            self.plan.as_array(),
        );
        if writer.batch_count() != 0 {
            mesh.refresh_directory();
        }
        self.draws
    }
}

fn q8_8(word: u16) -> fix32 {
    fix32::from_words(word << 8, ((word as i16) >> 8) as u16)
}

fn layer_depth(layer: u16, direction: &Vec3, layers: Array<u16>) -> i16 {
    let at = layer * 4;
    let depth = fdot(
        vec3::new(
            q8_8(layers[at + 1]),
            q8_8(layers[at + 2]),
            q8_8(layers[at + 3]),
        ),
        vec3::new(direction.x, direction.y, direction.z),
    );
    (depth * fix!(256.0)).to_int()
}

fn sort_layer_order(mut order: Array<u16>, depths: Array<i16>, count: u16) {
    // Use Knuth's gaps, growing 1, 4, 13, ... until the next gap would exceed one third of the list.
    let mut gap: u16 = 1;
    while gap * 3 + 1 <= count / 3 {
        gap = gap * 3 + 1;
    }
    loop {
        let mut i = gap;
        while i < count {
            let slot = order[i];
            let depth = depths[slot];
            let mut at = i;
            while at >= gap {
                let previous = order[at - gap];
                let previous_depth = depths[previous];
                // Use the original slot to preserve input order at equal depths.
                if previous_depth < depth || (previous_depth == depth && previous < slot) {
                    break;
                }
                order[at] = previous;
                at -= gap;
            }
            order[at] = slot;
            i += 1;
        }
        if gap == 1 {
            break;
        }
        gap /= 3;
    }
}
