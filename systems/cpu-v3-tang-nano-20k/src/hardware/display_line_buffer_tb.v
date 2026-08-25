`timescale 1ns/1ps
module tb;
reg write_clock=0,read_clock=0,write_enable=0;
reg [8:0] write_address=0,read_address=0; reg [31:0] write_data=0;
wire [31:0] read_data;
DisplayLineBuffer dut(.*);
always #7 write_clock=~write_clock; always #5 read_clock=~read_clock;
initial begin
  @(negedge write_clock); write_enable=1; write_address=479; write_data=32'h1234abcd;
  @(negedge write_clock); write_enable=0;
  @(negedge read_clock); read_address=479;
  @(negedge read_clock); #1;
  if(read_data!==32'h1234abcd) $fatal(1,"dual-clock read mismatch");
  $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
