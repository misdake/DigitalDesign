module tb;
reg clk=0, write_enable=0, write_way=0, write_value=0, clear_all=0;
reg [5:0] write_set=0;
wire [63:0] way_0, way_1;
CpuV3DataCacheDirtyRam dut(.*);
always #5 clk=~clk;
initial begin
  clear_all=1; @(posedge clk); #1; clear_all=0;
  write_enable=1; write_way=1; write_set=6'd37; write_value=1;
  @(posedge clk); #1; write_enable=0;
  if(way_0!=0 || !way_1[37]) $fatal(1,"dirty set failed");
  write_enable=1; write_value=0; @(posedge clk); #1; write_enable=0;
  if(way_1!=0) $fatal(1,"dirty clear failed");
  $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
