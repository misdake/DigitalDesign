//! Original 8x8 mutable memberships over caller-owned ordered ID rows.
use crate::dsl_rt::*;
use crate::{attitude, integer, object_query, ordered};
pub struct Cell {
    pub x: i16,
    pub z: u16,
    pub present: bool,
}
pub struct Insert {
    pub status: u16,
    pub cell: Cell,
}
pub struct Grid {
    pub origin: Array<u16>,
    pub shift_x: u16,
    pub shift_z: u16,
    pub items: Array<u16>,
    // Per cell: word offset, live ID count, ID capacity.
    pub ranges: Array<u16>,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
pub fn list(grid: &Grid, index: u16) -> ordered::List {
    ordered::List {
        items: unsafe {
            grid.items
                .as_ptr()
                .add(grid.ranges[index * 3] as i16)
                .as_u16_array()
        },
        count: grid.ranges[index * 3 + 1],
        capacity: grid.ranges[index * 3 + 2],
    }
}
fn count(grid: &mut Grid, index: u16, value: u16) {
    let mut ranges = grid.ranges;
    ranges[index * 3 + 1] = value;
}
pub fn candidates(grid: &Grid, start: Array<u16>) -> ordered::List {
    let index = object_query::cell(start, grid.origin, grid.shift_x, grid.shift_z);
    list(grid, index)
}
pub fn insert(
    grid: &mut Grid,
    id: fix32,
    position: Array<u16>,
    shape_present: bool,
    shape_radius: u16,
) -> Insert {
    let center = object_query::cell(position, grid.origin, grid.shift_x, grid.shift_z);
    let mut result = Insert {
        status: 0,
        cell: Cell {
            x: (center & 7) as i16,
            z: center >> 3,
            present: true,
        },
    };
    let x = n(result.cell.x);
    let z = fix32::from_words(result.cell.z, 0);
    let mut radius = n(5632);
    if shape_present {
        radius = fmul_lo32(fix32::from_words(shape_radius, 0) + n(352), n(16));
    }
    let px = attitude::get(position, 0);
    let pz = attitude::get(position, 2);
    let mut row = z - n(1);
    while row <= z + n(1) {
        if row >= n(0) && row < n(8) {
            let lower = integer::shl(row, grid.shift_z) + attitude::get(grid.origin, 1);
            let top = lower + integer::shl(n(1), grid.shift_z);
            if pz - radius < top && lower < pz + radius {
                let mut col = x - n(1);
                while col <= x + n(1) {
                    if col >= n(0) && col < n(8) {
                        let left = integer::shl(col, grid.shift_x) + attitude::get(grid.origin, 0);
                        let right = left + integer::shl(n(1), grid.shift_x);
                        if px - radius < right && px + radius > left {
                            let index = col.lo_bits() + row.lo_bits() * 8;
                            let mut target = list(grid, index);
                            let status = ordered::prepend(&mut target, id);
                            count(grid, index, target.count);
                            if status != 0 {
                                result.status = status;
                                return result;
                            }
                        }
                    }
                    col = col + n(1);
                }
            }
        }
        row = row + n(1);
    }
    result
}
pub fn remove(grid: &mut Grid, id: fix32, cell: &Cell, limit: fix32) -> u16 {
    if limit == n(0) {
        return 2;
    }
    if !cell.present {
        return 0;
    }
    let x = n(cell.x);
    let z = fix32::from_words(cell.z, 0);
    let mut row = z - n(1);
    while row <= z + n(1) {
        if row >= n(0) && row < n(8) {
            let mut col = x - n(1);
            while col <= x + n(1) {
                if col >= n(0) && col < n(8) {
                    let index = col.lo_bits() + row.lo_bits() * 8;
                    let mut target = list(grid, index);
                    let status = ordered::remove_first(&mut target, id, limit);
                    count(grid, index, target.count);
                    if status != 0 {
                        return status;
                    }
                }
                col = col + n(1);
            }
        }
        row = row + n(1);
    }
    0
}
