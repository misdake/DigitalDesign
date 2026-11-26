// Immutable-command POT/RTT source-Q coordinate owner.
module gpu_v2_texture_serial_coord(
 input clk,ce,reset,in_valid,output in_ready,fault,out_valid,
 input texture_fb400240,texture_clamp,input[1:0]texture_min_n,input in_nearest,input[3:0]in_size_log2,
 input[15:0]in_wrapped_uv_0,in_wrapped_uv_1,
 output[7:0]out_f0_0,out_f0_1,output[9:0]out_t0_0_0,out_t0_1_0
);
 reg[35:0]result_bank;reg[1:0]phase;reg[2:0]valid;
 assign in_ready=phase[0];assign fault=1'b0;
 assign out_valid=valid[2];
 wire[3:0]distance=4'd10-in_size_log2;
 wire[9:0]mask0={(in_size_log2>9),(in_size_log2>8),(in_size_log2>7),
  (in_size_log2>6),(in_size_log2>5),(in_size_log2>4),(in_size_log2>3),
  (in_size_log2>2),(in_size_log2>1),(in_size_log2!=0)};
 wire[15:0]axis_uv=phase[0]?in_wrapped_uv_0:in_wrapped_uv_1;
 wire[17:0]scaled_uv={axis_uv,2'b0}>>distance;

 // U=25*q=q+8*q+16*q; V=15*q=16*q-q. Compress the
 // three operands before one carry-propagating add; no DSP or stage is added.
 wire view_subtract=!phase[0];
 wire[20:0]view_a={5'd0,axis_uv}^{21{view_subtract}};
 wire[20:0]view_b={2'd0,axis_uv,3'd0}&{21{phase[0]}};
 wire[20:0]view_c={1'b0,axis_uv,4'd0};
 wire[20:0]view_sum=view_a^view_b^view_c;
 wire[20:0]view_majority=(view_a&view_b)|(view_a&view_c)|(view_b&view_c);
 wire[20:0]view_carry={view_majority[19:0],view_subtract};
 wire[20:0]view_product=view_sum+view_carry;
 wire[16:0]view_scaled=view_product[20:4];
 // Select the scale before the one signed half-texel subtractor. Both modes
 // use the existing U/V phases and freeze all operands on a disabled CE.
 wire[17:0]shared_scaled=texture_fb400240?{1'b0,view_scaled}:scaled_uv;
 wire signed[18:0]fine=$signed({1'b0,shared_scaled})-(in_nearest?19'sd0:19'sd128);
 // Only virtual endpoint weights change; adjacent tap addresses remain intact.
 wire clamp_mode=texture_fb400240||texture_clamp;
 wire[3:0]cell_n=in_size_log2-{2'd0,texture_min_n};
 wire[9:0]cell_mask={(cell_n>9),(cell_n>8),(cell_n>7),(cell_n>6),(cell_n>5),
   (cell_n>4),(cell_n>3),(cell_n>2),(cell_n>1),(cell_n>0)};
 // The grid comes from high normalized UV bits. Detect the half-texel
 // borrow at the cell's left edge before it becomes a neighboring cell.
 wire cell_first=(scaled_uv[17:8]&cell_mask)==0;
 wire clamp_negative=clamp_mode&&((texture_fb400240&&fine[18])||
   (texture_clamp&&cell_first&&!scaled_uv[7]&&!in_nearest));
 wire[9:0]index=fine[17:8];
 wire pot_last=&(index|~cell_mask);
 // 399=0x18f and239=0x0ef share every bit except8,6,5.
 wire view_last=!(index[9]||index[4])&&index[7]&&(&index[3:0])&&
   (index[8]==phase[0])&&(index[6]==!phase[0])&&(index[5]==!phase[0]);
 wire clamp_fraction=clamp_negative||(clamp_mode&&((texture_fb400240&&view_last)||(texture_clamp&&pot_last)));
 wire[9:0]clamped_index=clamp_negative?(texture_fb400240?10'd0:scaled_uv[17:8]):index;
 wire[17:0]axis_result={clamped_index&mask0,fine[7:0]&{8{!clamp_fraction}}};
 assign out_f0_0=result_bank[7:0];assign out_f0_1=result_bank[15:8];
 assign out_t0_0_0=result_bank[25:16];assign out_t0_1_0=result_bank[35:26];
 always @(posedge clk)begin
  if(reset)begin phase<=2'b01;valid<=0;end
  else if(ce)begin
   phase<={phase[0],phase[1]};valid<={valid[1],in_valid&&in_ready,1'b0};
   if(in_valid&&in_ready){result_bank[25:16],result_bank[7:0]}<=axis_result;
   if(valid[1]){result_bank[35:26],result_bank[15:8]}<=axis_result;
  end
 end
`ifndef SYNTHESIS
 reg[24:0]source_guard;
 always @(posedge clk)if(ce&&!reset)begin
  if(in_valid&&in_ready)source_guard<={in_wrapped_uv_1,in_size_log2,in_nearest,texture_fb400240,texture_clamp,texture_min_n};
  if(valid[1]&&source_guard!=={in_wrapped_uv_1,in_size_log2,in_nearest,texture_fb400240,texture_clamp,texture_min_n})
   $fatal(1,"coordinate V input changed before its source-Q consumption");
 end
`endif

`ifndef SYNTHESIS
 always @(posedge clk)if(!reset&&((1'b1)&&(ce&&in_valid&&!in_ready)))$fatal(1,"GPU contract: fault");
`endif
endmodule
