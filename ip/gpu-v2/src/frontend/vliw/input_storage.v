// Positions are reused after T computation. Two indexed attribute epochs live
// at {1,attribute,epoch,vertex}; T coefficients occupy rows below 256.
// 每bank独立1W1R，Vertex与T原写地址和数据保持；读沿之后各Q独立保持。
module rp_results_split(input clk,reset,ce,
 input position_we,export_we,input[7:0]position_wa,input[8:0]export_wa,input[35:0]wd,
 input position_re,input[8:0]position_ra,input export_re,input[8:0]export_ra,
 output[35:0]position_q,export_q);
 iz_ram36 positions(clk,reset,ce,position_we,{1'b0,position_wa},wd,position_re,position_ra,position_q);
 iz_ram36 exported(clk,reset,ce,export_we,export_wa,wd,export_re,export_ra,export_q);
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset)begin
  if(position_we&&position_wa[7:6]>=3)$fatal(1,"position write outside planes 0..2");
  if(position_re&&position_ra[8:6]>=3)$fatal(1,"T位置读超出plane0..2");
 end
`endif
endmodule

// Immutable queue heads expose refs throughout export without a second copy.
module rp_triangle_descriptors #(parameter AB=4)(input clk,we,input[AB-1:0]wa,input[36:0]wd,
 input[AB-1:0]ra,output[36:0]q);
`ifdef PORTABLE
 reg[36:0]mem[0:(1<<AB)-1];
 always @(posedge clk)if(we)mem[wa]<=wd;
 assign q=mem[ra];
`else
 wire[39:0]data={3'b0,wd};wire[39:0]result;
 assign q=result[36:0];genvar i;
 generate for(i=0;i<10;i=i+1)begin:lanes
  if(AB<=4)begin:single
   RAM16SDP4 ram(.DO(result[i*4+:4]),.DI(data[i*4+:4]),
    .WAD({{(4-AB){1'b0}},wa}),.RAD({{(4-AB){1'b0}},ra}),.WRE(we),.CLK(clk));
  end else begin:double
   wire[3:0]lo,hi;
   RAM16SDP4 a(.DO(lo),.DI(data[i*4+:4]),.WAD(wa[3:0]),.RAD(ra[3:0]),.WRE(we&&!wa[4]),.CLK(clk));
   RAM16SDP4 b(.DO(hi),.DI(data[i*4+:4]),.WAD(wa[3:0]),.RAD(ra[3:0]),.WRE(we&&wa[4]),.CLK(clk));
   assign result[i*4+:4]=ra[4]?hi:lo;
  end
 end endgenerate
`endif
endmodule
