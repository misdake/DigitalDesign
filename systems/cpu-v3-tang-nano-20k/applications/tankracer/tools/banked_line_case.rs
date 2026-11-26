// Compare the separately banked path with the resident path already covered by
// the original impact fixtures. Inputs straddle a window edge or occupy DSEG2.
fn check_pool_window(banked: &mut effect_pool::Pool, resident_lines: Array<u16>) {
    let aliases: Buf<u16, 16> = Buf::new([0; 16]);
    let mut resident = effect_pool::Pool {
        lines: resident_lines,
        page: 65535,
        count: fix32::zero(),
        overflow: effect_pool::line_zero(),
        exhausted: aliases.as_array(),
    };
    let incoming = read_dseg_page(2);
    let saved = effect_pool::select(banked);
    let mut records = banked.lines;
    let mut clear = 0u16;
    while clear < 1600 {
        records[clear] = 0;
        clear += 1;
    }
    effect_pool::restore(saved);
    let mut placement = 0u16;
    while placement < 2 {
        let mut address = 0x8400u16;
        if placement == 1 {
            address = 0x7ffc;
        }
        let mut input = unsafe { Ptr::from_addr(address).as_u16_array() };
        let colors = unsafe { input.as_ptr().add(12).as_u16_array() };
        let mut word = 0u16;
        while word < 16 {
            input[word] = 50000 + word * 67;
            word += 1;
        }
        let mut handle = 0u16;
        while handle < 2 {
            let mut id = fix32::zero();
            if handle == 1 {
                id = integer::from_i16(100);
            }
            if effect_pool::update(&mut resident, id, input, colors) != 0
                || effect_pool::update(banked, id, input, colors) != 0
                || read_dseg_page(2) != incoming
            {
                halt(0x7f73);
            }
            let mut expected = resident.lines;
            let mut actual = banked.lines;
            if handle == 1 {
                expected = unsafe { view_of(&mut resident.overflow).as_ptr().as_u16_array() };
                actual = unsafe { view_of(&mut banked.overflow).as_ptr().as_u16_array() };
            }
            let saved = effect_pool::select(banked);
            word = 0;
            while word < 16 {
                if actual[word] != expected[word] {
                    halt(0x7f74);
                }
                word += 1;
            }
            effect_pool::restore(saved);
            handle += 1;
        }
        word = 0;
        while word < 16 {
            if input[word] != 50000 + word * 67 {
                halt(0x7f75);
            }
            word += 1;
        }
        placement += 1;
    }
    let quad = quad_pool::Pool {
        records: resident_lines,
        exhausted: aliases.as_array(),
        count: fix32::zero(),
    };
    let mut index = 2u16;
    while index < 16 {
        let expected = attitude::get(resident.lines, index - 2);
        let observed = quad_pool::get(&quad, banked, integer::from_i16(160), index);
        if observed != expected {
            let diagnostic = unsafe { Ptr::from_addr(0x1fe0).as_u16_array() };
            attitude::put(diagnostic, 0, expected);
            attitude::put(diagnostic, 1, observed);
            attitude::put(diagnostic, 2, fix32::from_words(index, 0));
            attitude::put(
                diagnostic,
                3,
                fix32::from_words(resident.lines.as_ptr().addr(), 0),
            );
            attitude::put(
                diagnostic,
                4,
                fix32::from_words(banked.lines.as_ptr().addr(), 0),
            );
            attitude::put(diagnostic, 5, fix32::from_words(read_dseg_page(2), 0));
            halt(0x7f78);
        }
        quad_pool::put(
            &quad,
            banked,
            integer::from_i16(160),
            index,
            expected + integer::from_i16(1),
        );
        if effect_pool::get(banked, index - 2) != expected + integer::from_i16(1)
            || read_dseg_page(2) != incoming
        {
            halt(0x7f79);
        }
        index += 1;
    }
    let invalid = unsafe { Ptr::from_addr(0xffff).as_u16_array() };
    if effect_pool::update(banked, integer::from_i16(101), invalid, invalid) != 1 {
        halt(0x7f76);
    }
    banked.count = integer::from_i16(100);
    if effect_pool::allocate(banked, invalid, invalid) != integer::from_i16(-1)
        || read_dseg_page(2) != incoming
    {
        halt(0x7f77);
    }
}
