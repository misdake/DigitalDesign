`timescale 1ns/1ps
module tb;
reg write_clock=0,read_clock=0,write_enable=0;
reg [8:0] write_address=0,read_address_a=0,read_address_b=0;
reg [63:0] write_data=0;
wire [31:0] read_data_a,read_data_b;
DisplayLineBuffer dut(.*);
always #7 write_clock=~write_clock; always #5 read_clock=~read_clock;
integer i;
real linear, encoded;
integer expected;
initial begin repeat(2000) @(posedge read_clock); $fatal(1,"RAM watchdog"); end
initial begin
  for(i=0;i<400;i=i+1) begin
    @(negedge write_clock); write_enable=1; write_address=i;
    write_data={32'h98760000+i,32'h12340000+i};
  end
  @(negedge write_clock); write_enable=0;
  for(i=0;i<400;i=i+1) begin
    @(negedge read_clock); read_address_a=i; read_address_b=399-i;
    @(negedge read_clock);
    if(read_data_a!==32'h12340000+i || read_data_b!==32'h98760000+399-i)
      $fatal(1,"dual-clock data mismatch at %0d",i);
  end
  for(i=0;i<64;i=i+1) begin
    linear=i/63.0;
    if(linear<=0.0031308) encoded=12.92*linear;
    else encoded=1.055*(linear**(1.0/2.4))-0.055;
    expected=$rtoi(255.0*encoded+0.5);
    @(negedge read_clock); read_address_a=448+i; read_address_b=448+i;
    @(negedge read_clock);
    if(read_data_a!==expected || read_data_b!==expected)
      $fatal(1,"sRGB ROM mismatch at %0d",i);
  end
  $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
