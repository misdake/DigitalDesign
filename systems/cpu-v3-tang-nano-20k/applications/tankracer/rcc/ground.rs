//! Ordered primary contacts, surface noise, shared effects and fallback sampling.
use crate::dsl_rt::*;
use crate::static_assets::AssetBytes;
use crate::{deformation, integer, random, sampling, terrain};

pub struct Point {
    pub local_x: fix32,
    pub local_y: fix32,
    pub local_z: fix32,
    pub x: fix32,
    pub y: fix32,
    pub z: fix32,
    pub kind: fix32,
    pub gap: fix32,
    pub previous_gap: fix32,
    pub raw_height: fix32,
    pub travel: fix32,
    pub flags: fix32,
    pub nx: fix32,
    pub ny: fix32,
    pub nz: fix32,
    pub surface: fix32,
    pub node: fix32,
}
pub struct State {
    pub points: Buf<Point, 4>,
    pub auxiliary: Buf<sampling::Auxiliary, 2>,
    // Original surface effect: timer, restore, active, phase, two impulses, pose, clear.
    pub effect: Buf<u16, 16>,
    pub load: fix32,
    pub airborne: fix32,
    pub special: fix32,
    pub flagged: fix32,
    pub has_extra: fix32,
    pub extra: Buf<u16, 8>,
    pub flagged_ground: fix32,
    pub material0: fix32,
    pub material1: fix32,
    pub fallback_mode: fix32,
    pub feedback: fix32,
    pub clear_slot: fix32,
}
pub struct World {
    pub terrain: terrain::Terrain,
    pub cells: AssetBytes,
    pub directory: Array<u16>,
    pub seed: fix32,
    pub noise: Buf<u16, 16>,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn zero_hit() -> terrain::Hit {
    terrain::Hit {
        height: n(0),
        node: n(-1),
        face: n(-1),
        nx: n(0),
        ny: n(0),
        nz: n(0),
        surface: n(0),
        special: n(0),
        extra0: n(0),
        extra1: n(0),
        extra2: n(0),
        extra3: n(0),
        flags: n(0),
    }
}
pub fn zero() -> State {
    State {
        points: Buf::new(
            [Point {
                local_x: fix32::zero(),
                local_y: fix32::zero(),
                local_z: fix32::zero(),
                x: fix32::zero(),
                y: fix32::zero(),
                z: fix32::zero(),
                kind: fix32::zero(),
                gap: fix32::zero(),
                previous_gap: fix32::zero(),
                raw_height: fix32::zero(),
                travel: fix32::zero(),
                flags: fix32::zero(),
                nx: fix32::zero(),
                ny: fix32::zero(),
                nz: fix32::zero(),
                surface: fix32::zero(),
                node: fix32::zero(),
            }; 4],
        ),
        auxiliary: Buf::new(
            [sampling::Auxiliary {
                x: fix32::zero(),
                y: fix32::zero(),
                z: fix32::zero(),
                kind: fix32::zero(),
                surface: fix32::zero(),
                cached: fix32::zero(),
            }; 2],
        ),
        effect: Buf::new([0; 16]),
        load: n(0),
        airborne: n(0),
        special: n(0),
        flagged: n(0),
        has_extra: n(0),
        extra: Buf::new([0; 8]),
        flagged_ground: n(0),
        material0: n(0),
        material1: n(0),
        fallback_mode: n(0),
        feedback: n(0),
        clear_slot: n(-1),
    }
}
impl World {
    pub fn begin(cells: AssetBytes, seed: fix32, directory: Array<u16>) -> Self {
        let first = AssetBytes {
            first_sector: cells.first_sector,
            sector_count: 256,
            byte_len_lo: 32768,
            byte_len_hi: 0,
        };
        let mut world = Self {
            terrain: terrain::Terrain::begin(first),
            cells: cells,
            directory: directory,
            seed: seed,
            noise: Buf::new([0; 16]),
        };
        let mut i = 0u16;
        while i < 16 {
            world.seed = random::next(world.seed);
            world.noise[i] = (world.seed.lo_bits() >> 4) | (world.seed.hi_bits() << 12);
            i += 1;
        }
        world
    }
    pub fn bind(&mut self, x: fix32, z: fix32) {
        let cell = self.terrain.cell_at(x, z);
        if cell != 65535 && self.directory[cell] != 65535 {
            self.terrain.bind(AssetBytes {
                first_sector: self.cells.first_sector + self.directory[cell] * 256,
                sector_count: 256,
                byte_len_lo: 32768,
                byte_len_hi: 0,
            });
        } else {
            self.terrain.rebind();
        }
    }
    // Single-query scenery calls borrow DSEG3, then return to the caller's
    // property/geometry stage. Four-contact sampling keeps its batched binding.
    pub fn height(&mut self, x: fix32, z: fix32) -> terrain::Plane {
        let saved = read_dseg_page(3);
        self.bind(x, z);
        let result = self.terrain.height(x, z);
        if read_dseg_page(3) != saved {
            unsafe {
                write_dseg_page(3, saved);
            }
        }
        result
    }
    pub fn normal_height(
        &mut self,
        x: fix32,
        z: fix32,
        normal: &mut terrain::Normal,
    ) -> terrain::Plane {
        let saved = read_dseg_page(3);
        self.bind(x, z);
        let result = self.terrain.normal_height(x, z, normal);
        if read_dseg_page(3) != saved {
            unsafe {
                write_dseg_page(3, saved);
            }
        }
        result
    }
    pub fn fallback(
        &mut self,
        x: fix32,
        y: fix32,
        z: fix32,
        mode: u16,
        normal: &mut terrain::Normal,
    ) -> u16 {
        let saved = read_dseg_page(3);
        self.bind(x, z);
        let result = self.terrain.fallback(x, y, z, mode, normal);
        if read_dseg_page(3) != saved {
            unsafe {
                write_dseg_page(3, saved);
            }
        }
        result
    }
}
pub fn effect_step(state: &mut State, world: &mut World, dt: fix32, frame: u16) {
    let e = state.effect.as_array();
    sampling::put(e, 7, n(0));
    if sampling::raw(e, 0) <= n(0) {
        return;
    }
    let timer = (sampling::raw(e, 0).lo_bits() as i16) - (dt.lo_bits() as i16);
    sampling::put(e, 0, n(timer));
    world.seed = random::next(world.seed);
    let random = (world.seed.lo_bits() >> 4) | (world.seed.hi_bits() << 12);
    sampling::put(
        e,
        3,
        sampling::raw(e, 3) + fix32::from_words(random % 4000, 0) - n(2000),
    );
    if frame & 3 == 0 {
        sampling::put(e, 4, sampling::raw(e, 4) - n(100));
        sampling::put(e, 5, sampling::raw(e, 5) - n(900));
    }
    let mut change = n(200);
    if frame & 1 == 0 {
        change = n(-200);
    }
    sampling::put(e, 6, sampling::raw(e, 6) + change);
    if timer <= 0 {
        sampling::put(e, 6, sampling::raw(e, 1));
        if sampling::raw(e, 2) != n(0) {
            sampling::put(e, 7, n(1));
        }
        sampling::put(e, 2, n(0));
    }
}
// env: dt, frame, player, force_surface. Config: ten packed bump gains,
// effect gain multiplier/shift, then load base/floor/multiplier/shift (raw i32).
pub fn sample(
    state: &mut State,
    world: &mut World,
    tracks: Array<u16>,
    config: Array<u16>,
    env: Array<u16>,
    deformable: &mut deformation::Surface,
) -> u16 {
    state.clear_slot = n(-1);
    state.load = n(0);
    state.airborne = n(0);
    state.special = n(0);
    state.flagged = n(0);
    state.has_extra = n(0);
    state.flagged_ground = n(0);
    let active = sampling::raw(state.effect.as_array(), 0) > n(0);
    let force = sampling::raw(env, 3) != n(0);
    let mut points = state.points.as_array();
    let mut i = 0u16;
    while i < 4 {
        let t = unsafe { tracks.as_ptr().add(((i / 2) * 24) as i16).as_u16_array() };
        points[i].x = sampling::axis(
            t,
            0,
            points[i].local_x,
            points[i].local_y,
            points[i].local_z,
        );
        points[i].y = sampling::axis(
            t,
            1,
            points[i].local_x,
            points[i].local_y,
            points[i].local_z,
        );
        points[i].z = sampling::axis(
            t,
            2,
            points[i].local_x,
            points[i].local_y,
            points[i].local_z,
        );
        world.bind(points[i].x, points[i].z);
        let mut hit = zero_hit();
        let status = world.terrain.query(points[i].x, points[i].z, &mut hit);
        if status >= 2 {
            return status;
        }
        if status == 0 {
            points[i].gap = points[i].previous_gap;
            points[i].kind = n(3);
            points[i].surface = n(0);
            points[i].node = n(-1);
        } else {
            let mut surface = hit.surface;
            if force {
                surface = n(1);
            }
            points[i].surface = surface;
            points[i].node = hit.node;
            points[i].raw_height = hit.height;
            points[i].flags = fix32::from_words(hit.flags.lo_bits(), 0);
            if surface < n(0) || surface >= n(10) {
                return 5;
            }
            let mut x = points[i].x;
            let mut z = points[i].z;
            if x < n(0) {
                x = -x;
            }
            if z < n(0) {
                z = -z;
            }
            let key = (integer::sar(x, 9) + integer::sar(z, 9)).lo_bits() & 15;
            let noise = fix32::from_words(world.noise[key], 0) - n(127);
            let at = surface.lo_bits() * 2;
            let mut height = hit.height
                + integer::trunc_shift(
                    fmul_lo32(noise, fix32::from_words(config[at], 0)),
                    config[at + 1],
                );
            if active {
                height = height
                    + integer::trunc_shift(
                        fmul_lo32(noise, fix32::from_words(config[20u16], 0)),
                        config[21u16],
                    );
            }
            if deformable.enabled {
                let point: Buf<u16, 6> = Buf::new([0; 6]);
                let v = point.as_array();
                sampling::put(v, 0, points[i].x);
                sampling::put(v, 1, points[i].y);
                sampling::put(v, 2, points[i].z);
                let extra = deformation::sample(deformable, v);
                height = height + extra.offset;
            }
            if hit.flags.lo_bits() & 4 != 0 {
                state.flagged_ground = state.flagged_ground + n(1);
            }
            if hit.special == n(1) {
                state.special = state.special + n(1);
            }
            if hit.flags.lo_bits() & 0x2000 != 0 {
                state.flagged = state.flagged + n(1);
            }
            if hit.flags.lo_bits() & 0x10 != 0 {
                state.has_extra = n(1);
                let e = state.extra.as_array();
                sampling::put(e, 0, hit.extra0);
                sampling::put(e, 1, hit.extra1);
                sampling::put(e, 2, hit.extra2);
                sampling::put(e, 3, hit.extra3);
            }
            points[i].gap = points[i].y - height;
            points[i].previous_gap = points[i].gap;
            points[i].kind = n(2);
            if points[i].gap < n(-75) {
                points[i].kind = n(0);
            } else if points[i].gap < n(75) {
                points[i].kind = n(1);
            }
            if points[i].kind == n(2) {
                state.airborne = state.airborne + n(1);
            }
        }
        i += 1;
    }
    i = 0;
    while i < 4 {
        if points[i].kind == n(0) || points[i].kind == n(1) {
            let value = sampling::raw(config, 11) + points[i].travel;
            if value > n(0) {
                state.load = state.load
                    + integer::trunc_shift(
                        fmul_lo32(value, sampling::raw(config, 13)),
                        sampling::raw(config, 14).lo_bits(),
                    );
            }
            state.load = state.load + sampling::raw(config, 12);
        }
        i += 1;
    }
    effect_step(
        state,
        world,
        sampling::raw(env, 0),
        sampling::raw(env, 1).lo_bits(),
    );
    if sampling::raw(state.effect.as_array(), 7) != n(0) {
        state.clear_slot = fix32::from_words(sampling::raw(env, 2).lo_bits() & 255, 0);
    }
    state.fallback_mode = n(0);
    if state.flagged_ground == n(4) {
        state.fallback_mode = n(2);
    } else if state.flagged_ground == n(0) {
        state.fallback_mode = n(1);
    }
    if points[0u16].surface == points[2u16].surface {
        state.material0 = points[0u16].surface;
        state.material1 = points[0u16].surface;
    } else {
        i = 0;
        while i < 2 {
            let status = auxiliary(state, world, tracks, env, i);
            if status >= 2 {
                return status;
            }
            i += 1;
        }
        state.material0 = state.auxiliary[0u16].surface;
        state.material1 = state.auxiliary[1u16].surface;
    }
    i = 0;
    while i < 4 {
        if points[i].kind == n(3) {
            world.bind(points[i].x, points[i].z);
            let mut normal = terrain::Normal {
                x: points[i].nx,
                y: points[i].ny,
                z: points[i].nz,
            };
            let status = world.terrain.fallback(
                points[i].x,
                points[i].y,
                points[i].z,
                state.fallback_mode.lo_bits(),
                &mut normal,
            );
            if status >= 2 {
                return status;
            }
            if status == 1 {
                points[i].kind = n(4);
                points[i].nx = normal.x;
                points[i].ny = normal.y;
                points[i].nz = normal.z;
            }
        }
        i += 1;
    }
    state.feedback = n(0);
    if sampling::raw(env, 2) != n(0) {
        state.feedback = n(1);
    }
    0
}
// env also stores the two local auxiliary XYZ values at i32 slots 4..10.
pub fn auxiliary(
    state: &mut State,
    world: &mut World,
    tracks: Array<u16>,
    env: Array<u16>,
    side: u16,
) -> u16 {
    let t = unsafe { tracks.as_ptr().add((side * 24) as i16).as_u16_array() };
    let local = unsafe { env.as_ptr().add((8 + side * 6) as i16).as_u16_array() };
    let x = sampling::axis(
        t,
        0,
        sampling::raw(local, 0),
        sampling::raw(local, 1),
        sampling::raw(local, 2),
    );
    let z = sampling::axis(
        t,
        2,
        sampling::raw(local, 0),
        sampling::raw(local, 1),
        sampling::raw(local, 2),
    );
    world.bind(x, z);
    state.auxiliary[side].sample(&mut world.terrain, t, local, sampling::raw(env, 3) != n(0))
}
