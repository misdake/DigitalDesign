//! Pure table packing shared by the fitted line buffer and composition tests.
pub const TMDS_PRECODE_BITS: u32 = 9;

pub fn tmds_precode(data: u8) -> u32 {
    let ones = data.count_ones();
    let xnor = ones > 4 || (ones == 4 && data & 1 == 0);
    let mut qm = u32::from(data & 1);
    for bit in 1..8 {
        let next = ((qm >> (bit - 1)) ^ u32::from(data >> bit) ^ u32::from(xnor)) & 1;
        qm |= next << bit;
    }
    let count = (qm & 255).count_ones();
    qm |= u32::from(!xnor) << 8;
    if TMDS_PRECODE_BITS == 13 {
        qm |= count << 9;
    }
    qm
}

pub fn table_word(index: usize, linear_table: &[u8; 64]) -> u32 {
    tmds_precode(linear_table[index])
        | (tmds_precode(((index << 2) | (index >> 4)) as u8) << TMDS_PRECODE_BITS)
}

pub fn initialization(linear_table: &[u8; 64]) -> String {
    (416..512)
        .map(|address| {
            let value = if address < 448 {
                let index = address - 416;
                tmds_precode(((index << 3) | (index >> 2)) as u8)
            } else {
                table_word(address - 448, linear_table)
            };
            format!(
                "    memory_a[{address}]=32'h{value:08x}; memory_b[{address}]=32'h{value:08x};\n"
            )
        })
        .collect()
}
