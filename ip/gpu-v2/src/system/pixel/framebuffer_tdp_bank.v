// A belongs only to ROP. B belongs only to maintenance; no shared return owner.
module gpu_v2_framebuffer_tdp_bank(
 input clk,reset,input a_en,a_we,input[9:0]a_addr,input[15:0]a_data,output[15:0]a_q,
 input b_en,b_we,input[9:0]b_addr,input[15:0]b_data,output[15:0]b_q
);
`ifdef GPU_VLIW_POOL_PORTABLE
 reg[15:0]memory[0:1023];reg[15:0]qa,qb;assign a_q=qa;assign b_q=qb;
 always @(posedge clk)begin
  if(reset)begin qa<=0;qb<=0;end
  else begin
   if(a_en)begin if(a_we)memory[a_addr]<=a_data;else qa<=memory[a_addr];end
   if(b_en)begin if(b_we)memory[b_addr]<=b_data;else qb<=memory[b_addr];end
  end
 end
`else
 DPB #(.BIT_WIDTH_0(16),.BIT_WIDTH_1(16),.READ_MODE0(0),.READ_MODE1(0),
 .WRITE_MODE0(2'b00),.WRITE_MODE1(2'b00),.RESET_MODE("SYNC")) ram(
 .DOA(a_q),.DOB(b_q),.DIA(a_data),.DIB(b_data),
 .ADA({a_addr,2'b00,{2{a_we}}}),.ADB({b_addr,2'b00,{2{b_we}}}),
 .CLKA(clk),.CLKB(clk),.CEA(a_en&&!reset),.CEB(b_en&&!reset),
 .WREA(a_we),.WREB(b_we),.OCEA(1'b1),.OCEB(1'b1),
 .RESETA(reset),.RESETB(reset),.BLKSELA(3'd0),.BLKSELB(3'd0));
`endif
`ifndef SYNTHESIS
 always @(posedge clk)if(!reset&&a_en&&b_en&&(a_we||b_we)&&a_addr==b_addr)
  $fatal(1,"framebuffer TDP cross-port collision");
`endif
endmodule
