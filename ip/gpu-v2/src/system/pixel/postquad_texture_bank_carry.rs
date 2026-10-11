//! Exact required-bit inverse texture-bank address, only private unique cache.
pub fn lower(mut source: String) -> Result<String, String> {
    let marker = "module gpu_v2_texture_runtime_cache_unique #";
    if source.matches(marker).count() != 1 {
        return Err("Unique cache carry module".into());
    }
    let start = source.find(marker).unwrap();
    let end = start
        + source[start..]
            .find("\nendmodule")
            .ok_or("Unique cache end")?;
    let mut cache = source[start..end].to_owned();
    let begin = cache
        .find("  // Inverse tap map for each fixed bank, including7->0 repeat wrap.")
        .ok_or("Bank carry start")?;
    let finish = cache.find(" wire data_write=").ok_or("Bank carry end")?;
    let mut rows =
        String::from("  // Direct four address bits; all64 local origins preserve repeat wrap.\n");
    for bank in 0..4 {
        let hi = bank / 2;
        let lo = bank % 2;
        rows.push_str(&format!(
            "  wire bank_low{bank}=1'b{hi}^head_tlx[1]^(head_tlx[0]&1'b{});\n",
            1 - lo
        ));
        rows.push_str(&format!(
            "  wire bank_carry{bank}=head_tly[0]&!bank_low{bank};\n"
        ));
        rows.push_str(&format!("  wire[3:0]bank_row{bank}={{head_tly[2]^(head_tly[1]&bank_carry{bank}),head_tly[1]^bank_carry{bank},bank_low{bank},head_tlx[2]^(head_tlx[1]&head_tlx[0]&1'b{})}};\n",1-lo));
    }
    cache.replace_range(begin..finish, &rows);
    for bank in 0..4 {
        let old = format!("assign data_ra[{bank}]={{hit_line,bank_y{bank},bank_x{bank}[2]}};");
        let new = format!("assign data_ra[{bank}]={{hit_line,bank_row{bank}}};");
        if cache.matches(&old).count() != 1 {
            return Err(format!("Bank carry address {bank}"));
        }
        cache = cache.replacen(&old, &new, 1);
    }
    source.replace_range(start..end, &cache);
    Ok(source)
}

///Independent host formula for the four physical row bits, with no adder.
pub fn physical_row(x: u8, y: u8, bank: u8) -> u8 {
    assert!(x < 8 && y < 8 && bank < 4);
    let hi = bank >> 1;
    let lo = bank & 1;
    let low = hi ^ ((x >> 1) & 1) ^ ((x & 1) & (lo ^ 1));
    let carry = (y & 1) & (low ^ 1);
    let y1 = ((y >> 1) & 1) ^ carry;
    let y2 = ((y >> 2) & 1) ^ (((y >> 1) & 1) & carry);
    let x2 = ((x >> 2) & 1) ^ (((x >> 1) & 1) & (x & 1) & (lo ^ 1));
    (y2 << 3) | (y1 << 2) | (low << 1) | x2
}
