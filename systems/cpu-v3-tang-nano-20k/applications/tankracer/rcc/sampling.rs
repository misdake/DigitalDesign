//! Auxiliary suspension contacts. Transform first, commit classification after query success.
use crate::dsl_rt::*;
use crate::integer;
use crate::terrain::{Hit, Terrain};

pub struct Auxiliary {
    pub x: fix32,
    pub y: fix32,
    pub z: fix32,
    pub kind: fix32,
    pub surface: fix32,
    pub cached: fix32,
}

pub fn raw(data: Array<u16>, index: u16) -> fix32 {
    fix32::from_words(data[index * 2], data[index * 2 + 1])
}

// Column-major Q12 basis at 0..9 followed by raw integer origin at 9..12.
pub fn axis(data: Array<u16>, index: u16, x: fix32, y: fix32, z: fix32) -> fix32 {
    integer::sar(
        fmul_lo32(raw(data, index), x)
            + fmul_lo32(raw(data, index + 3), y)
            + fmul_lo32(raw(data, index + 6), z),
        12,
    ) + raw(data, index + 9)
}

impl Auxiliary {
    /// The caller binds the page containing the transformed X/Z before sampling.
    /// Statuses use Terrain's contract; errors retain kind, surface and cache.
    pub fn sample(
        &mut self,
        terrain: &mut Terrain,
        transform: Array<u16>,
        local: Array<u16>,
        force_surface: bool,
    ) -> u16 {
        let x = raw(local, 0);
        let y = raw(local, 1);
        let z = raw(local, 2);
        self.x = axis(transform, 0, x, y, z);
        self.y = axis(transform, 1, x, y, z);
        self.z = axis(transform, 2, x, y, z);
        let mut hit = Hit {
            height: fix32::zero(),
            node: fix32::zero(),
            face: fix32::zero(),
            nx: fix32::zero(),
            ny: fix32::zero(),
            nz: fix32::zero(),
            surface: fix32::zero(),
            special: fix32::zero(),
            extra0: fix32::zero(),
            extra1: fix32::zero(),
            extra2: fix32::zero(),
            extra3: fix32::zero(),
            flags: fix32::zero(),
        };
        let status = terrain.query(self.x, self.z, &mut hit);
        if status >= 2 {
            return status;
        }
        if status == 1 {
            let gap = self.y - hit.height;
            self.kind = integer::from_i16(2);
            if gap < integer::from_i16(-75) {
                self.kind = fix32::zero();
            } else if gap < integer::from_i16(75) {
                self.kind = integer::from_i16(1);
            }
            self.surface = hit.surface;
        } else {
            self.kind = integer::from_i16(3);
            self.surface = fix32::zero();
        }
        if force_surface {
            self.surface = integer::from_i16(1);
        }
        self.cached = integer::from_i16(1);
        status
    }
}

pub fn put(data: Array<u16>, index: u16, value: fix32) {
    fix32::store_words(data, index * 2, value);
}
