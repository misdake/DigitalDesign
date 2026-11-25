//! Ordered terrain queries over one resident 32-KiB collision cell at a time.
use crate::dsl_rt::*;
use crate::integer;
use crate::static_assets::AssetBytes;

pub struct Hit {
    pub height: fix32,
    pub node: fix32,
    pub face: fix32,
    pub nx: fix32,
    pub ny: fix32,
    pub nz: fix32,
    pub surface: fix32,
    pub special: fix32,
    pub extra0: fix32,
    pub extra1: fix32,
    pub extra2: fix32,
    pub extra3: fix32,
    pub flags: fix32,
}
pub struct Normal {
    pub x: fix32,
    pub y: fix32,
    pub z: fix32,
}
struct Selection {
    status: u16,
    node: u16,
    mesh: u16,
    face: u16,
    index: u16,
}
pub struct Plane {
    pub status: u16,
    pub height: fix32,
}
pub struct Terrain {
    saved: u16,
    page: u16,
    cell: u16,
    origin_x: fix32,
    origin_z: fix32,
    shift_x: u16,
    shift_z: u16,
    pub switches: u16,
}

fn words() -> Array<u16> {
    unsafe { Ptr::from_addr(0xc000).as_u16_array() }
}
fn i32_at(data: Array<u16>, at: u16) -> fix32 {
    fix32::from_words(data[at], data[at + 1])
}
fn signed(value: u16) -> fix32 {
    integer::from_i16(value as i16)
}
fn byte(data: Array<u16>, at: u16, index: u16) -> u16 {
    (data[at + (index >> 1)] >> ((index & 1) << 3)) & 255
}
fn miss_height() -> fix32 {
    fix32::from_words(1, 32768)
}
fn edge(x: fix32, z: fix32, data: Array<u16>, a: u16, b: u16) -> bool {
    fmul_lo32(
        x - signed(data[a]),
        signed(data[b + 2]) - signed(data[a + 2]),
    ) <= fmul_lo32(z - signed(data[a + 2]), signed(data[b]) - signed(data[a]))
}

impl Terrain {
    pub fn begin(first: AssetBytes) -> Self {
        let mut terrain = Self {
            saved: read_dseg_page(3),
            page: 65535,
            cell: 65535,
            origin_x: fix32::zero(),
            origin_z: fix32::zero(),
            shift_x: 0,
            shift_z: 0,
            switches: 0,
        };
        terrain.bind(first);
        let data = words();
        terrain.origin_x = i32_at(data, 4);
        terrain.origin_z = i32_at(data, 6);
        terrain.shift_x = data[8u16] & 255;
        terrain.shift_z = data[8u16] >> 8;
        terrain
    }

    pub fn bind(&mut self, asset: AssetBytes) {
        if asset.first_sector & 255 != 0
            || asset.sector_count != 256
            || asset.byte_len_lo != 32768
            || asset.byte_len_hi != 0
        {
            halt(0x7c01);
        }
        let page = asset.first_sector >> 8;
        if self.page == page && read_dseg_page(3) == page {
            return;
        }
        unsafe {
            write_dseg_page(3, page);
        }
        if read_dseg_page(3) != page {
            halt(0x7c02);
        }
        let data = words();
        if data[0u16] != 0x5443 || data[1u16] != 1 || data[3u16] > 16384 {
            halt(0x7c03);
        }
        self.page = page;
        self.cell = data[2u16];
        self.switches += 1;
    }

    // Re-select a previously validated cell after another stage borrowed DSEG3.
    pub fn rebind(&mut self) {
        if self.page == 65535 {
            halt(0x7c04);
        }
        if read_dseg_page(3) != self.page {
            unsafe {
                write_dseg_page(3, self.page);
            }
            self.switches += 1;
        }
    }
    pub fn finish(&mut self) {
        unsafe {
            write_dseg_page(3, self.saved);
        }
        self.page = 65535;
        self.cell = 65535;
    }

    pub fn cell_at(&self, x: fix32, z: fix32) -> u16 {
        let gx = integer::sar(integer::sar(x, 3) - self.origin_x, self.shift_x);
        let gz = integer::sar(integer::sar(z, 3) - self.origin_z, self.shift_z);
        if gx < fix32::zero()
            || gx >= fix32::from_words(8, 0)
            || gz < fix32::zero()
            || gz >= fix32::from_words(8, 0)
        {
            return 65535;
        }
        gx.lo_bits() + (gz.lo_bits() << 3)
    }

    fn select(&self, x: fix32, z: fix32) -> Selection {
        let mut selected = Selection {
            status: 0,
            node: 0,
            mesh: 0,
            face: 0,
            index: 0,
        };
        let cell = self.cell_at(x, z);
        if cell == 65535 {
            return selected;
        }
        if cell != self.cell || self.page == 65535 {
            selected.status = 4;
            return selected;
        }
        let data = words();
        let sx = integer::sar(x, 3);
        let sz = integer::sar(z, 3);
        let mut index: u16 = 0;
        while index < data[9u16] {
            let node = data[data[10u16] + index];
            let mesh = data[node];
            let dx = sx - i32_at(data, mesh);
            let dz = sz - i32_at(data, mesh + 4);
            if dx.abs() <= signed(data[node + 5]) && dz.abs() <= signed(data[node + 6]) {
                let shifts = data[node + 4];
                let bucket = integer::sar(dx, 13 - (shifts & 255))
                    - integer::shl(integer::sar(dz, 13 - (shifts >> 8)), 3)
                    + fix32::from_words(28, 0);
                if bucket < fix32::zero() || bucket >= fix32::from_words(64, 0) {
                    selected.status = 3;
                    return selected;
                }
                let list = data[data[node + 1] + bucket.lo_bits()];
                if list != 65535 {
                    let mut offset = list;
                    while offset < data[node + 3] {
                        let face_index = byte(data, data[node + 2], offset);
                        if face_index == 255 {
                            break;
                        }
                        let face = data[mesh + 11] + face_index * 6;
                        let vertices = data[mesh + 9];
                        let a = vertices + (data[face + 3] & 255) * 3;
                        let b = vertices + (data[face + 3] >> 8) * 3;
                        let c = vertices + data[face + 4] * 3;
                        if edge(dx, dz, data, a, b)
                            && edge(dx, dz, data, b, c)
                            && edge(dx, dz, data, c, a)
                        {
                            selected.status = 1;
                            selected.node = node;
                            selected.mesh = mesh;
                            selected.face = face;
                            selected.index = face_index;
                            return selected;
                        }
                        offset += 1;
                    }
                }
            }
            index += 1;
        }
        selected
    }

    pub fn query(&self, x: fix32, z: fix32, out: &mut Hit) -> u16 {
        let selected = self.select(x, z);
        if selected.status == 0 {
            out.height = miss_height();
        }
        if selected.status != 1 {
            return selected.status;
        }
        let result = plane(&selected, x, z, false);
        if result.status != 1 {
            return result.status;
        }
        let data = words();
        let node = selected.node;
        let mesh = selected.mesh;
        let face = selected.face;
        out.height = result.height;
        out.node = i32_at(data, node + 13);
        out.face = i32_at(data, mesh + 14) + fix32::from_words(selected.index * 24, 0);
        out.nx = signed(data[face]);
        out.ny = signed(data[face + 1]);
        out.nz = signed(data[face + 2]);
        out.surface = fix32::from_words(data[node + 7], 0);
        out.special = fix32::from_words((data[face + 5] >> 8) & 1, 0);
        let mut flags = (data[face + 5] & 0x2000) | data[mesh + 6];
        if data[node + 12] != 0 {
            flags |= 0x20;
        }
        out.flags = fix32::from_words(flags, out.flags.hi_bits());
        if data[mesh + 6] & 0x10 != 0 {
            out.extra0 = signed(data[node + 8]);
            out.extra1 = signed(data[node + 9]);
            out.extra2 = signed(data[node + 10]);
            out.extra3 = signed(data[node + 11]);
        }
        1
    }

    pub fn height(&self, x: fix32, z: fix32) -> Plane {
        let selected = self.select(x, z);
        if selected.status != 1 {
            return Plane {
                status: selected.status,
                height: miss_height(),
            };
        }
        plane(&selected, x, z, true)
    }

    pub fn normal_height(&self, x: fix32, z: fix32, normal: &mut Normal) -> Plane {
        let selected = self.select(x, z);
        if selected.status != 1 {
            return Plane {
                status: selected.status,
                height: miss_height(),
            };
        }
        let data = words();
        normal.x = signed(data[selected.face]);
        normal.y = signed(data[selected.face + 1]);
        normal.z = signed(data[selected.face + 2]);
        plane(&selected, x, z, true)
    }

    pub fn fallback(&self, x: fix32, y: fix32, z: fix32, mode: u16, normal: &mut Normal) -> u16 {
        let cell = self.cell_at(x, z);
        if cell == 65535 {
            return 0;
        }
        if cell != self.cell || self.page == 65535 {
            return 4;
        }
        let data = words();
        let sx = integer::sar(x, 3);
        let sy = integer::sar(y, 3);
        let sz = integer::sar(z, 3);
        let mut best = fix32::from_words(37856, 4);
        let mut selected: u16 = 0;
        let mut index: u16 = 0;
        while index < data[11u16] {
            let node = data[data[12u16] + index];
            let mesh = data[node];
            let flag = data[mesh + 6] & 4;
            if (mode != 1 || flag == 0) && (mode != 2 || flag != 0) {
                let dx = sx - i32_at(data, mesh);
                let dz = sz - i32_at(data, mesh + 4);
                let radius = fix32::from_words(data[mesh + 8] << 1, data[mesh + 8] >> 15);
                if dx.abs() <= radius && dz.abs() <= radius {
                    let lx = integer::sar(
                        fmul_lo32(signed(data[node + 4]), dz)
                            + fmul_lo32(signed(data[node + 3]), dx),
                        12,
                    );
                    let lz = integer::sar(
                        fmul_lo32(signed(data[node + 6]), dz)
                            + fmul_lo32(signed(data[node + 5]), dx),
                        12,
                    );
                    if lx >= signed(data[node + 7])
                        && lx <= signed(data[node + 8])
                        && lz >= signed(data[node + 9])
                        && lz <= signed(data[node + 10])
                    {
                        let dy = sy - i32_at(data, mesh + 2);
                        let mut at: u16 = 0;
                        while at < data[node + 2] {
                            let face_index = byte(data, data[node + 1], at);
                            if face_index == 255 {
                                break;
                            }
                            let center = data[mesh + 13] + face_index * 3;
                            let distance = approximate_distance(
                                dx - signed(data[center]),
                                dy - signed(data[center + 1]),
                                dz - signed(data[center + 2]),
                            );
                            if distance < best {
                                best = distance;
                                selected = data[mesh + 11] + face_index * 6;
                            }
                            at += 1;
                        }
                    }
                }
            }
            index += 1;
        }
        if selected == 0 {
            return 0;
        }
        normal.x = signed(data[selected]);
        normal.y = signed(data[selected + 1]);
        normal.z = signed(data[selected + 2]);
        1
    }
}

fn plane(selection: &Selection, x: fix32, z: fix32, vertical: bool) -> Plane {
    let data = words();
    let mesh = selection.mesh;
    let face = selection.face;
    let vertex = data[mesh + 9] + (data[face + 3] & 255) * 3;
    let ox = i32_at(data, mesh);
    let oy = i32_at(data, mesh + 2);
    let oz = i32_at(data, mesh + 4);
    let vx = signed(data[vertex]);
    let vy = signed(data[vertex + 1]);
    let vz = signed(data[vertex + 2]);
    let nx = signed(data[face]);
    let ny = signed(data[face + 1]);
    let nz = signed(data[face + 2]);
    let mut result = Plane {
        status: 1,
        height: fix32::zero(),
    };
    if ny == fix32::zero() && vertical {
        result.height = integer::shl(oy + vx, 3);
        return result;
    }
    let numerator = integer::shl(
        fmul_lo32(oz + vz - integer::sar(z, 3), nz) + fmul_lo32(ox + vx - integer::sar(x, 3), nx),
        3,
    );
    if ny == fix32::zero()
        || (numerator == fix32::from_words(0, 32768) && ny == fix32::from_words(65535, 65535))
    {
        result.status = 2;
        return result;
    }
    result.height = integer::div(numerator, ny) + integer::shl(oy + vy, 3);
    result
}

fn magnitude(value: fix32) -> fix32 {
    if value < fix32::zero() {
        return -value;
    }
    value
}
fn unsigned_greater(a: fix32, b: fix32) -> bool {
    a.hi_bits() > b.hi_bits() || (a.hi_bits() == b.hi_bits() && a.lo_bits() > b.lo_bits())
}
fn quarter(value: fix32) -> fix32 {
    fix32::from_words(
        (value.lo_bits() >> 2) | (value.hi_bits() << 14),
        value.hi_bits() >> 2,
    )
}
fn approximate_distance(x: fix32, y: fix32, z: fix32) -> fix32 {
    let a = magnitude(x);
    let b = magnitude(y);
    let c = magnitude(z);
    if unsigned_greater(a, b) && unsigned_greater(a, c) {
        return a + quarter(b) + quarter(c);
    }
    if !unsigned_greater(a, b) && unsigned_greater(b, c) {
        return b + quarter(a) + quarter(c);
    }
    c + quarter(a) + quarter(b)
}
