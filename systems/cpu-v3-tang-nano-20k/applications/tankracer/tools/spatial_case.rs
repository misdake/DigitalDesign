// Preserve all 64 live candidate rows after each operation, including failures.
fn spatial_snapshot(
    out: Array<u16>,
    base: u16,
    status: u16,
    cell: &spatial::Cell,
    grid: &spatial::Grid,
) {
    attitude::put(out, base, fix32::from_words(status, 0));
    attitude::put(out, base + 1, integer::from_i16(cell.x));
    attitude::put(out, base + 2, fix32::from_words(cell.z, 0));
    let mut present = fix32::zero();
    if cell.present {
        present = integer::from_i16(1);
    }
    attitude::put(out, base + 3, present);
    let mut i = 0u16;
    while i < 64 {
        let list = spatial::list(grid, i);
        attitude::put(out, base + 4 + i * 5, fix32::from_words(list.count, 0));
        let mut j = 0u16;
        while j < 4 {
            let mut id = integer::from_i16(-1);
            if j < list.count {
                id = attitude::get(list.items, j);
            }
            attitude::put(out, base + 5 + i * 5 + j, id);
            j += 1;
        }
        i += 1;
    }
}
fn run_spatial(input: Array<u16>, out: Array<u16>) {
    let items: Buf<u16, 1024> = Buf::new([0; 1024]);
    let range_storage: Buf<u16, 192> = Buf::new([0; 192]);
    let mut ranges = range_storage.as_array();
    let mut i = 0u16;
    while i < 64 {
        ranges[i * 3] = i * 16;
        ranges[i * 3 + 2] = 8;
        i += 1;
    }
    ranges[1u16] = 2;
    ranges[4u16] = 2;
    ranges[190u16] = 1;
    attitude::put(items.as_array(), 8, integer::from_i16(1));
    let mut grid = spatial::Grid {
        origin: unsafe { input.as_ptr().add(6).as_u16_array() },
        shift_x: attitude::get(input, 5).lo_bits(),
        shift_z: attitude::get(input, 6).lo_bits(),
        items: items.as_array(),
        ranges: ranges,
    };
    let radius = attitude::get(input, 8).lo_bits();
    let present = attitude::get(input, 7) != fix32::zero();
    let limit = attitude::get(input, 9);
    let first = spatial::insert(&mut grid, integer::from_i16(2), input, present, radius);
    spatial_snapshot(out, 0, first.status, &first.cell, &grid);
    let status = spatial::remove(&mut grid, integer::from_i16(2), &first.cell, limit);
    spatial_snapshot(out, 324, status, &first.cell, &grid);
    let next = spatial::insert(&mut grid, integer::from_i16(3), input, present, radius);
    spatial_snapshot(out, 648, next.status, &next.cell, &grid);
    let cell = spatial::Cell {
        x: attitude::get(input, 10).lo_bits() as i16,
        z: attitude::get(input, 11).lo_bits(),
        present: attitude::get(input, 12) != fix32::zero(),
    };
    let status = spatial::remove(&mut grid, fix32::zero(), &cell, limit);
    spatial_snapshot(out, 972, status, &cell, &grid);
}
