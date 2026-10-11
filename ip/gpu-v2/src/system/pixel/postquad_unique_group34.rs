//! Closed immutable-draw Group34: slot is draw context, selected tile XY replaces mask4.
fn body(source: &str, name: &str) -> Result<String, String> {
    let begin = source
        .find(&format!("module {name} "))
        .or_else(|| source.find(&format!("module {name}(")))
        .ok_or(name)?;
    let end = begin + source[begin..].find("endmodule").ok_or("module end")? + 9;
    Ok(source[begin..end].into())
}
fn one(value: &mut String, old: &str, new: &str) -> Result<(), String> {
    if value.matches(old).count() != 1 {
        return Err(format!("Group34 anchor {old}"));
    }
    *value = value.replace(old, new);
    Ok(())
}
pub fn lower(mut source: String) -> Result<String, String> {
    source = source.replace("\r\n", "\n");
    for name in [
        "gpu_v2_texture_runtime_backend_unique",
        "gpu_v2_runtime_packet_backend_unique",
        "gpu_v2_packet_store_unique",
    ] {
        let old = body(&source, name)?;
        let mut new = old.replace("[39:0]", "[33:0]");
        // Pool lookahead exposes only key18; the parent restores immutable slot4.
        new = new
            .replace("[21:0] next_key", "[17:0] next_key")
            .replace("[21:0]next_key", "[17:0]next_key");
        if name == "gpu_v2_packet_store_unique" {
            new = new
                .replace("head0[21:0]", "head0[17:0]")
                .replace("head1[21:0]", "head1[17:0]")
                .replace("data_do[21:0]", "data_do[17:0]");
            let begin = new
                .find(" // One native header36 bank")
                .ok_or("pool column")?;
            let end = new.len() - 9;
            new.replace_range(
                begin..end,
                r#" generate if(NATIVE)begin:payload_hardware
  wire[35:0]q;
  SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.RESET_MODE("SYNC")) bank(
   .DO(q),.DI({2'd0,write_payload}),.BLKSELA(3'b0),.BLKSELB(3'b0),
   .ADA({3'b0,wptr,1'b0,4'b1111}),.ADB({3'b0,rptr,5'b0}),
   .CLKA(clk),.CLKB(clk),.CEA(pw),.CEB(pr),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
  assign data_do=q[33:0];
 end else begin:payload_model
  reg[33:0]mem[0:63];reg[33:0]q;
  always@(posedge clk)begin
   if(pw)mem[wptr]<=write_payload;
   if(reset)q<=0;else if(pr)q<=mem[rptr];
  end
  assign data_do=q;
 end endgenerate
"#,
            );
        }
        one(&mut source, &old, &new)?;
    }
    let old = body(&source, "gpu_v2_texture_work_unique")?;
    let mut new = old.clone();
    one(
        &mut new,
        "payload_rows[write_ptr] <= in_member[75:40];",
        "payload_rows[write_ptr] <= {in_member[75:64],4'd0,in_member[59:40]};",
    )?;
    one(&mut source, &old, &new)?;
    let old = body(&source, "packet_leaf_unique")?;
    let mut new = old.replace("[39:0]", "[33:0]");
    one(&mut new,"wire[33:0]result={masks,capture0[36:35],capture0[34:31],last,first,\n capture0[22:20],capture0[19:17],ty,tx,level,capture0[26:23]};","wire[33:0]result={y,x,capture0[36:35],capture0[34:31],last,first,\n capture0[22:20],capture0[19:17],ty,tx,level};")?;
    one(&mut source, &old, &new)?;
    let old = body(&source, "gpu_v2_texture_runtime_cache_unique")?;
    let mut new = old.clone();
    one(
        &mut new,
        "input wire in_valid,input wire[39:0]in_payload,",
        "input wire in_valid,input wire[33:0]in_payload,input wire[3:0]draw_slot,",
    )?;
    one(
        &mut new,
        "reg[39:0]res_payload,paid_payload;",
        "reg[33:0]res_payload,paid_payload;",
    )?;
    one(
        &mut new,
        "wire[39:0]head_payload=in_payload;",
        "wire[39:0]head_payload={4'd0,in_payload[31:0],draw_slot};",
    )?;
    one(
        &mut new,
        "paid_payload<=head_payload;",
        "paid_payload<=in_payload;",
    )?;
    one(&mut new,"wire[35:0]masked_weights;","wire[35:0]masked_weights;\n wire msx=(res_payload[3:0]<=3)||(res_payload[20:18]!=7);\n wire msy=(res_payload[3:0]<=3)||(res_payload[23:21]!=7);\n wire mx=res_payload[32],my=res_payload[33];\n wire[3:0]restored_mask={(mx||msx)&&(my||msy),(!mx||msx)&&(my||msy),(mx||msx)&&(!my||msy),(!mx||msx)&&(!my||msy)};")?;
    one(&mut new, "res_payload[36+wt]?", "restored_mask[wt]?")?;
    new = new
        .replace("res_payload[25]", "res_payload[21]")
        .replace("res_payload[23]", "res_payload[19]")
        .replace("res_payload[22]", "res_payload[18]");
    one(
        &mut new,
        "{res_payload[35:28],physical_weights,res_payload[27:0]}",
        "{res_payload[31:24],physical_weights,res_payload[23:0],draw_slot}",
    )?;
    one(&mut source, &old, &new)?;
    let old = body(&source, "gpu_v2_pixel_sampling_ingress")?;
    let mut new = old.clone();
    one(
        &mut new,
        "output wire[39:0]packet_out_payload,",
        "output wire[33:0]packet_out_payload,",
    )?;
    one(
        &mut new,
        "wire[21:0]packet_next_key;",
        "wire[17:0]packet_next_key;",
    )?;
    one(&mut new,".in_valid(packet_out_valid),.in_payload(packet_out_payload),.next_valid(packet_next_valid),.next_key(packet_next_key),",".draw_slot(draw_config[19:16]),.in_valid(packet_out_valid),.in_payload(packet_out_payload),.next_valid(packet_next_valid),.next_key({packet_next_key,draw_config[19:16]}),")?;
    one(&mut source, &old, &new)?;
    Ok(source)
}
