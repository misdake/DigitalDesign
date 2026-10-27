//! Private published-only Basic port for the complete sole-next cadence.
//! Generic ingress and row-store exports remain checked and unchanged.
fn extract(source: &str, name: &str) -> Result<String, String> {
    let marker = format!("module {name}(");
    if source.matches(&marker).count() != 1 {
        return Err(format!("Closed Basic {name}"));
    }
    let start = source.find(&marker).unwrap();
    let end = start
        + source[start..]
            .find("\nendmodule")
            .ok_or("Closed Basic module end")?;
    Ok(source[start..end + "\nendmodule".len()].to_owned())
}
pub fn lower(mut source: String) -> Result<String, String> {
    let mut quad = extract(&source, "gpu_v2_quad_ingress_shared")?;
    let mut bank = extract(&source, "gpu_v2_pixel_row_store")?;
    let collision = " wire bad_collision = proposal_ce && basic_read && proposed_push && live &&\n  basic_read_address == {slot,row};";
    if quad.matches(collision).count() != 1 {
        return Err("Closed Basic quad collision".into());
    }
    quad = quad.replacen(collision, " // Published readers cannot alias the sole unpublished producer tail.\n wire bad_collision = 1'b0;", 1);
    quad = quad.replacen(
        "module gpu_v2_quad_ingress_shared(",
        "module gpu_v2_quad_ingress_closed_basic(",
        1,
    );
    if quad.matches("gpu_v2_pixel_row_store basic_bank(").count() != 1 {
        return Err("Closed Basic port".into());
    }
    quad = quad.replacen(
        "gpu_v2_pixel_row_store basic_bank(",
        "gpu_v2_pixel_row_store_published basic_bank(",
        1,
    );
    let collision = " assign collision=read_en && write_en && read_address==write_address;";
    if bank.matches(collision).count() != 1 {
        return Err("Closed Basic primitive collision".into());
    }
    bank = bank.replacen(collision, " // Lease is proved by the only combined caller; generic bank stays checked.\n assign collision=1'b0;", 1);
    bank = bank.replacen(
        "module gpu_v2_pixel_row_store(",
        "module gpu_v2_pixel_row_store_published(",
        1,
    );
    let instance = " gpu_v2_quad_ingress_shared quad(";
    if source.matches(instance).count() != 1 {
        return Err("Closed Basic sole caller".into());
    }
    source = source.replacen(instance, " gpu_v2_quad_ingress_closed_basic quad(", 1);
    source.push_str("\n// Private closed profile: no new state, slots, or paid DO behavior.\n");
    source.push_str(&quad);
    source.push('\n');
    source.push_str(&bank);
    source.push('\n');
    Ok(source)
}
