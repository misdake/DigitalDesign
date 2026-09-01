module tb;
reg clk=0, reset=1, clean_all=0, invalidate_all=0;
reg cpu_request_valid=0, cpu_write=0, cpu_response_ready=0;
reg [31:0] cpu_address=0; reg [15:0] cpu_write_data=0;
reg memory_request_ready=1, memory_response_valid=0, memory_error=0;
reg [31:0] memory_read_data=0;
wire cpu_request_ready, cpu_response_valid, cpu_error;
wire [15:0] cpu_read_data;
wire memory_request_valid, memory_write, memory_line, memory_response_ready;
wire [21:0] memory_address; wire [31:0] memory_write_data;
wire maintenance_busy, maintenance_done, maintenance_error;
CpuV3DataCache dut(.*);
always #5 clk=~clk;

reg [15:0] memory [0:4095];
integer i, cycles=0, line_reads=0, line_writes=0;
reg [3:0] read_remaining=0, write_remaining=0;
reg [21:0] transfer_base=0;
reg write_response_pending=0;
wire [3:0] read_index = 8-read_remaining;
wire [3:0] write_index = 8-write_remaining;

always @(posedge clk) begin
  cycles <= cycles+1;
  if(cycles>20000) $fatal(1,"data-cache test cycle limit state=%0d addr=%h valid=%b hit=%b resp=%b rr=%0d wr=%0d",
      dut.state,dut.pending_address,dut.pending_address_valid,dut.pending_hit,
      dut.response_valid,read_remaining,write_remaining);
  memory_response_valid <= 0; memory_error <= 0;
  if(read_remaining!=0) begin
    memory_response_valid <= 1;
    memory_read_data <= {memory[transfer_base+2*read_index+1],
                         memory[transfer_base+2*read_index]};
    read_remaining <= read_remaining-1;
  end else if(write_response_pending) begin
    memory_response_valid <= 1;
    write_response_pending <= 0;
  end
  if(write_remaining!=0) begin
    memory[transfer_base+2*write_index] <= memory_write_data[15:0];
    memory[transfer_base+2*write_index+1] <= memory_write_data[31:16];
    if(write_remaining==1) write_response_pending <= 1;
    write_remaining <= write_remaining-1;
  end
  if(memory_request_valid && memory_request_ready) begin
    if(!memory_line) $fatal(1,"D-cache emitted a word transaction");
    transfer_base <= memory_address;
    if(memory_write) begin
      line_writes <= line_writes+1;
      memory[memory_address] <= memory_write_data[15:0];
      memory[memory_address+1] <= memory_write_data[31:16];
      write_remaining <= 7;
    end else begin
      line_reads <= line_reads+1;
      read_remaining <= 8;
    end
  end
end

task access;
  input wr; input [31:0] address; input [15:0] value; input [15:0] expected;
  begin
    while(!cpu_request_ready) @(posedge clk);
    @(negedge clk); cpu_write=wr; cpu_address=address;
    cpu_write_data=value; cpu_request_valid=1;
    @(posedge clk); @(negedge clk); cpu_request_valid=0;
    while(!cpu_response_valid) @(posedge clk);
    #1;
    if(cpu_error) $fatal(1,"CPU cache access failed at %h",address);
    if(!wr && cpu_read_data!==expected)
      $fatal(1,"read mismatch at %h: %h != %h",address,cpu_read_data,expected);
    cpu_response_ready=1; @(posedge clk); #1; cpu_response_ready=0;
  end
endtask

task maintain;
  input invalidate;
  begin
    @(negedge clk);
    if(invalidate) invalidate_all=1; else clean_all=1;
    @(posedge clk); @(negedge clk); invalidate_all=0; clean_all=0;
    while(!maintenance_done) @(posedge clk);
    #1;
    if(maintenance_error) $fatal(1,"maintenance failed");
  end
endtask

initial begin
  for(i=0;i<4096;i=i+1) memory[i]=16'h8000^i;
  repeat(2) @(posedge clk); reset=0;
  access(0,32'h20,0,16'h8020);
  access(1,32'h20,16'hdead,0);
  if(line_writes!=0) $fatal(1,"store hit reached memory");
  access(0,32'h20,0,16'hdead);
  access(0,32'h420,0,16'h8420);
  access(1,32'h420,16'hbeef,0);
  access(0,32'h820,0,16'h8820);
  if(line_writes!=1 || memory[16'h20]!==16'hdead)
    $fatal(1,"dirty victim was not written before refill");
  maintain(0);
  if(line_writes!=2 || memory[16'h420]!==16'hbeef)
    $fatal(1,"clean did not write the remaining dirty line");
  i=line_reads; access(0,32'h420,0,16'hbeef);
  if(line_reads!=i) $fatal(1,"clean invalidated a resident line");
  access(1,32'h820,16'hcafe,0);
  maintain(1);
  if(memory[16'h820]!==16'hcafe) $fatal(1,"invalidate dropped dirty data");
  i=line_reads; access(0,32'h820,0,16'hcafe);
  if(line_reads!=i+1) $fatal(1,"invalidate left the line resident");
  $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
