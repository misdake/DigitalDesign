module tb;
reg clk=0;
reg [5:0] read_set=0,write_set_0=0,write_set_1=0;
reg write_0=0,write_1=0;
reg [15:0] write_data_0=0,write_data_1=0;
wire [15:0] q0,q1;
CpuV3DataCacheMetadata dut(.*);
always #5 clk=~clk;
reg [15:0] golden0[0:63],golden1[0:63];
reg [15:0] expected0=0,expected1=0;
reg [31:0] rng=32'h815acf29;
integer i,cycles=0;
always @(posedge clk) begin
 cycles<=cycles+1;
 if(cycles>2500)$fatal(1,"metadata cycle limit");
end
task edge_and_check;begin
 if(write_0)golden0[write_set_0]=write_data_0;else expected0=golden0[read_set];
 if(write_1)golden1[write_set_1]=write_data_1;else expected1=golden1[read_set];
 @(posedge clk);#1;
 if(q0!==expected0 || q1!==expected1)
  $fatal(1,"metadata normal-mode hold/read mismatch cycle=%0d read=%0d q=%h/%h expected=%h/%h",
   cycles,read_set,q0,q1,expected0,expected1);
end endtask
initial begin
 for(i=0;i<64;i=i+1)begin golden0[i]=0;golden1[i]=0;end
 // Every set/bit, independent port read/write addresses, and simultaneous
 // way-one install plus way-zero victim RMW are covered by distinct words.
 for(i=0;i<2048;i=i+1)begin
  @(negedge clk);
  rng=rng^(rng<<13);rng=rng^(rng>>17);rng=rng^(rng<<5);
  read_set=rng[5:0];write_set_0=i<64?i:rng[11:6];write_set_1=i<64?i:rng[17:12];
  write_0=i<64 || rng[18];write_1=i<64 || rng[19];
  write_data_0=rng[31:16]^16'h52ac;write_data_1=rng[15:0]^16'had37;
  edge_and_check();
 end
 // Reset/error/invalidate share a set-at-a-time metadata scrub. Output holds
 // on writes, so do not infer scrub completion from DO.
 for(i=0;i<64;i=i+1)begin
  @(negedge clk);write_0=1;write_1=1;write_set_0=i;write_set_1=i;
  write_data_0=0;write_data_1=0;edge_and_check();
 end
 for(i=0;i<64;i=i+1)begin
  @(negedge clk);write_0=0;write_1=0;read_set=i;edge_and_check();
  if(q0!==0 || q1!==0)$fatal(1,"scrub missed set %0d",i);
 end
 $display("DIGITAL_DESIGN_PASS");$finish;
end
endmodule
