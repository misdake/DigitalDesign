module tb;
reg clk=0,reset=1,invalidate_all=0,prefetch_request_valid=0,prefetch_cancel=0;
reg [31:0] prefetch_address=0,cpu_address=0;
reg cpu_request_valid=0,cpu_response_ready=0,memory_request_ready=1;
reg memory_response_valid=0,memory_error=0; reg [31:0] memory_read_data=0;
wire cpu_request_ready,cpu_response_valid,cpu_error,memory_request_valid,memory_response_ready;
wire [15:0] cpu_read_data; wire [21:0] memory_address;
wire [31:0] prefetch_issued,prefetch_useful,prefetch_useless,prefetch_dropped;
CpuV3InstructionCache dut(.*);
always #5 clk=~clk;
integer beat,cycles=0;
always @(posedge clk) begin cycles<=cycles+1; if(cycles>1000) $fatal(1,"timeout"); end
initial begin
  repeat(2) @(posedge clk); reset=0;
  @(negedge clk); cpu_address=32'h24; cpu_request_valid=1;
  @(posedge clk); @(negedge clk); cpu_request_valid=0;
  while(!memory_request_valid) @(posedge clk);
  @(posedge clk);
  for(beat=0;beat<8;beat=beat+1) begin
    @(negedge clk); memory_read_data=32'h90019000+beat*32'h00020002;
    memory_response_valid=1; @(posedge clk);
  end
  @(negedge clk); memory_response_valid=0;
  while(!cpu_response_valid) @(posedge clk);
  #1; if(cpu_error || cpu_read_data!==16'h9004) $fatal(1,"bad I-cache refill");
  $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
