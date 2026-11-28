//! Stream original collision records and class slots from immutable scene assets.
use crate::dsl_rt::*;
use crate::gpu_window::gpu_window_asset_read;
use crate::static_assets::AssetBytes;
use crate::{attitude, collision, integer, object_query, ordered, scene_world, spatial};
pub struct Record {
    pub collider: object_query::Collider,
    pub class: fix32,
    pub weight: fix32,
    pub threshold: fix32,
    pub activity: fix32,
    pub flags: fix32,
    pub mesh_bounds: Buf<i16, 6>,
}
pub struct Scene {
    pub asset: AssetBytes,
    pub header: Buf<u16, 16>,
    pub cells: Buf<u16, 128>,
    pub valid: bool,
}
pub struct Method {
    pub status: u16,
    pub id: u16,
    pub maximum: fix32,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
pub fn record_zero() -> Record {
    Record {
        collider: object_query::collider_zero(),
        class: n(-1),
        weight: n(0),
        threshold: n(0),
        activity: n(0),
        flags: n(0),
        mesh_bounds: Buf::new([0; 6]),
    }
}
fn read(scene: &Scene, offset: u16, dst: Array<u16>, count: u16) -> bool {
    let len = (scene.asset.byte_len_lo >> 1) | (scene.asset.byte_len_hi << 15);
    if count == 0 || offset > len || count > len - offset {
        return false;
    }
    gpu_window_asset_read(
        dst,
        scene.asset.first_sector + (offset >> 6),
        offset & 63,
        count,
    )
}
impl Scene {
    pub fn begin(asset: AssetBytes) -> Self {
        let mut scene = Self {
            asset: asset,
            header: Buf::new([0; 16]),
            cells: Buf::new([0; 128]),
            valid: false,
        };
        if asset.byte_len_hi > 1
            || asset.byte_len_lo & 1 != 0
            || !read(&scene, 0, scene.header.as_array(), 16)
        {
            return scene;
        }
        let h = scene.header.as_array();
        let len = (asset.byte_len_lo >> 1) | (asset.byte_len_hi << 15);
        if h[0u16] != 0x5343
            || h[1u16] != 1
            || h[2u16] != len
            || h[5u16] > len
            || h[3u16] > (len - h[5u16]) / 76
            || h[6u16] > len
            || h[4u16] > (len - h[6u16]) / 4
            || h[8u16] > len
            || h[9u16] > len - h[8u16]
        {
            return scene;
        }
        if !read(&scene, h[7u16], scene.cells.as_array(), 128) {
            return scene;
        }
        let mut i = 0u16;
        while i < 64 {
            let base = scene.cells[i * 2];
            if base > len || scene.cells[i * 2 + 1] > len - base {
                return scene;
            }
            i += 1;
        }
        scene.valid = true;
        scene
    }
    pub fn load(&self, index: u16, out: &mut Record) -> bool {
        if !self.valid || index >= self.header[3u16] {
            return false;
        }
        let dst = unsafe { view_of(&mut *out).as_ptr().as_u16_array() };
        read(&self, self.header[5u16] + index * 76, dst, 76)
    }
    pub fn load_world(&self, world: &mut scene_world::World) -> u16 {
        spatial::select(&world.grid);
        if !self.valid {
            return 7;
        }
        if world.count < self.header[3u16] || world.deformables.capacity < self.header[9u16] {
            return 4;
        }
        let mut record = record_zero();
        let mut colliders = world.colliders;
        let mut properties = world.properties;
        let mut i = 0u16;
        while i < self.header[3u16] {
            if !self.load(i, &mut record) {
                return 7;
            }
            let raw = unsafe { view_of(&mut record).as_ptr().as_u16_array() };
            let mut j = 0u16;
            while j < 60 {
                colliders[i * 60 + j] = raw[j];
                j += 1;
            }
            j = 0;
            while j < 16 {
                properties[i * 16 + j] = raw[60 + j];
                j += 1;
            }
            i += 1;
        }
        world.count = self.header[3u16];
        let mut origin = world.grid.origin;
        i = 0;
        while i < 4 {
            origin[i] = self.header[10 + i];
            i += 1;
        }
        world.grid.shift_x = self.header[14u16];
        world.grid.shift_z = self.header[15u16];
        i = 0;
        while i < 64 {
            let mut row = spatial::list(&world.grid, i);
            let status = load_ids(&self, self.cells[i * 2], self.cells[i * 2 + 1], &mut row);
            let mut ranges = world.grid.ranges;
            ranges[i * 3 + 1] = row.count;
            if status != 0 {
                return status;
            }
            i += 1;
        }
        load_ids(
            &self,
            self.header[8u16],
            self.header[9u16],
            &mut world.deformables,
        )
    }
    pub fn method(&self, class: u16, slot: u16) -> Method {
        let mut result = Method {
            status: 4,
            id: 0,
            maximum: n(0),
        };
        if !self.valid || class >= self.header[4u16] {
            return result;
        }
        let entry: Buf<u16, 4> = Buf::new([0; 4]);
        let row = entry.as_array();
        if !read(&self, self.header[6u16] + class * 4, row, 4) {
            return result;
        }
        result.maximum = fix32::from_words(row[0u16], row[1u16]);
        if fix32::from_words(slot, 0) > result.maximum {
            result.status = 2;
            return result;
        }
        if slot >= row[2u16]
            || row[3u16] > self.header[2u16]
            || slot >= self.header[2u16] - row[3u16]
        {
            return result;
        }
        let value: Buf<u16, 1> = Buf::new([0; 1]);
        if !read(&self, row[3u16] + slot, value.as_array(), 1) {
            return result;
        }
        result.status = 0;
        if value[0u16] != 65535 {
            result.status = 1;
            result.id = value[0u16];
        }
        result
    }
    pub fn query(&self, q: &object_query::Query, out: &mut object_query::Hit) -> u16 {
        if !self.valid {
            return 7;
        }
        let mut base = self.header[8u16];
        let mut count = self.header[9u16];
        if !q.deformable {
            let origin = unsafe { self.header.as_array().as_ptr().add(10).as_u16_array() };
            let cell = object_query::cell(q.start, origin, self.header[14u16], self.header[15u16]);
            base = self.cells[cell * 2];
            count = self.cells[cell * 2 + 1];
        }
        let delta: Buf<u16, 6> = Buf::new([0; 6]);
        let d = delta.as_array();
        let mut i = 0u16;
        while i < 3 {
            attitude::put(d, i, attitude::get(q.end, i) - attitude::get(q.start, i));
            i += 1;
        }
        let length = collision::distance(d);
        let ids: Buf<u16, 32> = Buf::new([0; 32]);
        let list = ids.as_array();
        let mut object = record_zero();
        let mut cursor = 0u16;
        while cursor < count {
            let mut chunk = count - cursor;
            if chunk > 32 {
                chunk = 32;
            }
            if !read(&self, base + cursor, list, chunk) {
                return 7;
            }
            i = 0;
            while i < chunk {
                let index = list[i];
                if index >= self.header[3u16] {
                    return 4;
                }
                if !self.load(index, &mut object) {
                    return 7;
                }
                let status =
                    object_query::obstacle_candidate(&object.collider, index, q, out, d, length);
                if status != 0 {
                    return status;
                }
                i += 1;
            }
            cursor += chunk;
        }
        0
    }
}
fn load_ids(scene: &Scene, base: u16, count: u16, out: &mut ordered::List) -> u16 {
    if count > out.capacity {
        return 4;
    }
    out.count = 0;
    let buffer: Buf<u16, 32> = Buf::new([0; 32]);
    let mut cursor = 0u16;
    while cursor < count {
        let mut chunk = count - cursor;
        if chunk > 32 {
            chunk = 32;
        }
        if !read(scene, base + cursor, buffer.as_array(), chunk) {
            return 7;
        }
        let mut i = 0u16;
        while i < chunk {
            attitude::put(out.items, cursor + i, fix32::from_words(buffer[i], 0));
            out.count += 1;
            i += 1;
        }
        cursor += chunk;
    }
    0
}
