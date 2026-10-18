// Position planes0..2 are read by T; N36/attr32 planes3..4 and low coefficients by R/I.
// 每bank独立1W1R，Vertex与T原写地址和数据保持；读沿之后各Q独立保持。
module rp_results_split(input clk,reset,ce,
 input position_we,export_we,input[8:0]wa,input[35:0]wd,
 input position_re,input[8:0]position_ra,input export_re,input[8:0]export_ra,
 output[35:0]position_q,export_q);
 iz_ram36 positions(clk,reset,ce,position_we,wa,wd,position_re,position_ra,position_q);
 iz_ram36 exported(clk,reset,ce,export_we,wa,wd,export_re,export_ra,export_q);
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset)begin
  if(position_we&&wa[8:6]>=3)$fatal(1,"位置写超出plane0..2");
  if(export_we&&(wa[8:6]<3||wa[8:6]==5))$fatal(1,"导出写侵入位置plane");
  if(position_re&&position_ra[8:6]>=3)$fatal(1,"T位置读超出plane0..2");
  if(export_re&&(export_ra[8:6]<3||export_ra[8:6]==5))$fatal(1,"导出读侵入位置plane");
 end
`endif
endmodule
