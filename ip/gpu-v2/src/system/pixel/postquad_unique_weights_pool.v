 // One native header36 bank and one 64x4 distributed tap-mask column.
 (* syn_ramstyle="distributed_ram" *)reg[3:0]mask_rows[0:63];reg[3:0]mask_q;
 always@(posedge clk)begin
  if(pw)mask_rows[wptr]<=write_payload[39:36];
  if(reset)mask_q<=0;else if(pr)mask_q<=mask_rows[rptr];
 end
 assign data_do[39:36]=mask_q;
 generate if(NATIVE)begin:payload_hardware
  SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.RESET_MODE("SYNC")) bank(
   .DO(data_do[35:0]),.DI(write_payload[35:0]),.BLKSELA(3'b0),.BLKSELB(3'b0),
   .ADA({3'b0,wptr,1'b0,4'b1111}),.ADB({3'b0,rptr,5'b0}),
   .CLKA(clk),.CLKB(clk),.CEA(pw),.CEB(pr),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
 end else begin:payload_model
  reg[35:0]mem[0:63];reg[35:0]q;
  always@(posedge clk)begin
   if(pw)mem[wptr]<=write_payload[35:0];
   if(reset)q<=0;else if(pr)q<=mem[rptr];
  end
  assign data_do[35:0]=q;
 end endgenerate
