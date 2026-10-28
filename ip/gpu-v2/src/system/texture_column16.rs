//! Pure layout experiment; no framebuffer-texture view or NPOT coordinate logic.
//! Tile-internal row order, cache capacity and128B transport remain unchanged.

fn replace(source: &mut String, old: &str, new: &str) -> Result<(), String> {
    if source.matches(old).count() != 1 {
        return Err(format!("Texture column16 anchor: {old}"));
    }
    *source = source.replacen(old, new, 1);
    Ok(())
}

pub fn lower(mut source: String) -> Result<String, String> {
    replace(&mut source,
        "wire[11:0]macro_row={6'd0,ty[6:1]}<<((level<4)?4'd0:(level-4'd4));\n wire[11:0]macro_tile=macro_row|{6'd0,tx};",
        "wire[11:0]macro_column={6'd0,tx}<<((level<4)?4'd0:(level-4'd4));\n wire[11:0]macro_tile=macro_column|{6'd0,ty[6:1]};")?;
    let start = source
        .find(" wire[8:0]mt_tile_y={5'd0,mt_tile[9:6]};")
        .ok_or("Texture column16 FB row conversion")?;
    let end = start
        + source[start..]
            .find(" wire mt_half=")
            .ok_or("Texture column16 FB address end")?;
    source.replace_range(
        start..end,
        r#" // Raw half-tile tag remains unchanged. UI keeps its logical row-order ABI.
 function[8:0]column_ui_tile;
  input[8:0]logical_tile;reg[8:0]remainder;reg[3:0]row;
  begin
   remainder=logical_tile;row=0;
   if(remainder>=9'd200)begin remainder=remainder-9'd200;row[3]=1;end
   if(remainder>=9'd100)begin remainder=remainder-9'd100;row[2]=1;end
   if(remainder>=9'd50)begin remainder=remainder-9'd50;row[1]=1;end
   if(remainder>=9'd25)begin remainder=remainder-9'd25;row[0]=1;end
   column_ui_tile={remainder[4:0],row};
  end
 endfunction
 wire[8:0]mt_linear_tile=ui_active?column_ui_tile(ui_tile):{mt_tile[4:0],mt_tile[9:6]};
"#,
    );
    // Keep the existing UI tile ownership/lifetime. This experiment changes
    // external addressing only; no UI write is added to the shared mt_target.
    replace(&mut source, "clear_sector==1499", "clear_sector==1599")?;
    replace(
        &mut source,
        "cp_q[13:0]>16009||cp_q[27:14]>16009",
        "cp_q[13:0]>15984||cp_q[27:14]>15984",
    )?;
    replace(&mut source,
            "!(({1'b0,cp_q[13:0]}+15'd375<={1'b0,cp_q[27:14]})||({1'b0,cp_q[27:14]}+15'd375<={1'b0,cp_q[13:0]}))",
            "!(({1'b0,cp_q[13:0]}+15'd400<={1'b0,cp_q[27:14]})||({1'b0,cp_q[27:14]}+15'd400<={1'b0,cp_q[13:0]}))")?;
    Ok(source)
}
