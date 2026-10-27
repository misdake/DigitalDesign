 // Coissue immutable weight read with the actual texel-bank read, same paid lease.
 wire[6:0]weight_ra={head_payload[33:30],head_payload[35:34],head_payload[4]};
 wire[35:0]weight_do;
 generate if(NATIVE)begin:weights_native
  SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.RESET_MODE("SYNC")) bank(
   .DO(weight_do),.DI(weight_value),.BLKSELA(3'b0),.BLKSELB(3'b0),
   .ADA({2'b0,weight_address,1'b0,4'b1111}),.ADB({2'b0,weight_ra,5'b0}),
   .CLKA(clk),.CLKB(clk),.CEA(weight_write),.CEB(read_issue),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
 end else begin:weights_portable
  reg[35:0]mem[0:127];reg[35:0]q;
  always@(posedge clk)begin
   if(weight_write)mem[weight_address]<=weight_value;
   if(reset)q<=0;else if(read_issue)q<=mem[weight_ra];
  end
  assign weight_do=q;
 end endgenerate
 wire[35:0]masked_weights;
 genvar wt;generate for(wt=0;wt<4;wt=wt+1)begin:restore_mask
  assign masked_weights[wt*9+:9]=res_payload[36+wt]?res_weights[wt*9+:9]:9'd0;
 end endgenerate
 // Keep the already qualified inverse coefficient permutation and raw texels.
 wire y0=res_payload[25]^res_payload[23],y1=y0^res_payload[22];
 wire[8:0]v0=y0?masked_weights[18+:9]:masked_weights[0+:9];
 wire[8:0]v1=y1?masked_weights[27+:9]:masked_weights[9+:9];
 wire[8:0]v2=y0?masked_weights[0+:9]:masked_weights[18+:9];
 wire[8:0]v3=y1?masked_weights[9+:9]:masked_weights[27+:9];
 wire[35:0]physical_weights=res_payload[22]?{v2,v3,v0,v1}:{v3,v2,v1,v0};
 assign out_payload={res_payload[35:28],physical_weights,res_payload[27:0]};
 assign out_tex0=res_tex[0];assign out_tex1=res_tex[1];
 assign out_tex2=res_tex[2];assign out_tex3=res_tex[3];
