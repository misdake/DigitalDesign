module tb;
reg clk=0, reset=1, clean_all=0, invalidate_all=0;
reg line_copy_start=0;
reg [21:0] line_copy_source=0;
reg [7:0] line_copy_destination_page=0;
reg line_clean_start=0;
reg [21:0] line_clean_address=0;
wire line_copy_ready;
reg cpu_request_valid=0, cpu_write=0, cpu_response_ready=0;
reg [31:0] cpu_address=0; reg [15:0] cpu_write_data=0;
reg memory_request_ready=1, memory_response_valid=0, memory_error=0;
reg [63:0] memory_read_data=0;
wire cpu_request_ready, cpu_response_valid, cpu_error;
wire memory_write_data_ready;
wire [15:0] cpu_read_data;
wire memory_request_valid, memory_write, memory_line, memory_response_ready;
wire [21:0] memory_address; wire [63:0] memory_write_data;
wire maintenance_busy, maintenance_done, maintenance_error, valid_sweep;
CpuV3DataCache dut(.*);
always #5 clk=~clk;

reg [15:0] memory [0:262143];
integer i, j, cycles=0, line_reads=0, line_writes=0;
integer copy_start_cycle=0;
always @(negedge clk) memory_request_ready = cycles % 7 >= 3;
reg request_stalled=0;
reg [21:0] held_request_address;
reg [63:0] held_request_data;
integer request_stall_cycles=0;
reg [3:0] read_remaining=0, write_remaining=0;
reg [21:0] transfer_base=0;
reg write_response_pending=0;
reg inject_write_error=0, write_error_pending=0;
integer error_after_beats=0, failed_accepted_beats=0;
reg inject_read_error=0; integer read_error_index=0; wire [3:0] read_index = 4-read_remaining;
wire [3:0] write_index = 4-write_remaining;
// Burst-ready windows exercise all beat stalls and consecutive transfers.
assign memory_write_data_ready = write_remaining != 0 &&
    (line_writes % 2 == 1 ? cycles % 5 == 4 : cycles % 11 >= 4);
reg [63:0] stalled_data;
reg was_stalled=0;
integer stalled_beats=0, accepted_beats=0, failed_writes=0;

always @(posedge clk) begin
  cycles <= cycles+1;
  if(cycles>20000) $fatal(1,"data-cache test cycle limit state=%0d addr=%h valid=%b hit=%b resp=%b rr=%0d wr=%0d",
      dut.state,dut.pending_address,dut.pending_address_valid,dut.pending_hit,
      dut.response_valid,read_remaining,write_remaining);
  if (request_stalled && (!memory_request_valid || !memory_write ||
      memory_address !== held_request_address || memory_write_data !== held_request_data))
    $fatal(1,"write request/data changed before address acceptance");
  request_stalled <= memory_request_valid && memory_write && !memory_request_ready;
  held_request_address <= memory_address;
  held_request_data <= memory_write_data;
  if (memory_request_valid && memory_write && !memory_request_ready)
    request_stall_cycles <= request_stall_cycles + 1;
  memory_response_valid <= 0; memory_error <= 0;
  if(read_remaining!=0) begin
    memory_response_valid <= 1;
    memory_read_data <= {memory[transfer_base+4*read_index+3],
                         memory[transfer_base+4*read_index+2],
                         memory[transfer_base+4*read_index+1],
                         memory[transfer_base+4*read_index]};
    if(inject_read_error && read_index==read_error_index) begin
      memory_error<=1;read_remaining<=0;
    end else read_remaining <= read_remaining-1;
  end else if(write_error_pending) begin
    memory_response_valid <= 1;
    memory_error <= 1;
    if (memory_response_ready) write_error_pending <= 0;
  end else if(write_response_pending) begin
    memory_response_valid <= 1;
    write_response_pending <= 0;
  end
  if (was_stalled && memory_write_data !== stalled_data)
    $fatal(1,"write-back data changed during a stall");
  was_stalled <= write_remaining != 0 && !memory_write_data_ready;
  stalled_data <= memory_write_data;
  if (write_remaining != 0 && !memory_write_data_ready)
    stalled_beats <= stalled_beats | (1 << write_index);
  if(write_remaining!=0 && memory_write_data_ready) begin
    accepted_beats <= accepted_beats + 1;
    memory[transfer_base+4*write_index] <= memory_write_data[15:0];
    memory[transfer_base+4*write_index+1] <= memory_write_data[31:16];
    memory[transfer_base+4*write_index+2] <= memory_write_data[47:32];
    memory[transfer_base+4*write_index+3] <= memory_write_data[63:48];
    if (inject_write_error && write_index + 1 == error_after_beats) begin
      write_error_pending <= 1;
      failed_accepted_beats <= failed_accepted_beats + error_after_beats;
      write_remaining <= 0;
    end else begin
      if(write_remaining==1) write_response_pending <= 1;
      write_remaining <= write_remaining-1;
    end
  end
  if(memory_request_valid && memory_request_ready) begin
    if(!memory_line) $fatal(1,"D-cache emitted a word transaction");
    transfer_base <= memory_address;
    if(memory_write) begin
      line_writes <= line_writes+1;
      if (inject_write_error) begin
        if (error_after_beats == 0) write_error_pending <= 1;
        else write_remaining <= 4;
        failed_writes <= failed_writes + 1;
      end
      else write_remaining <= 4;
    end else begin
      line_reads <= line_reads+1;
      read_remaining <= 4;
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

task copy_line;
  input [21:0] source; input [7:0] destination_page;
  begin
    copy_start_cycle=cycles;
    @(negedge clk);
    line_copy_source=source;
    line_copy_destination_page=destination_page;
    line_copy_start=1;
    @(posedge clk); @(negedge clk); line_copy_start=0;
    while(!maintenance_done) @(posedge clk);
    #1;
    if(maintenance_error) $fatal(1,"line copy failed");
    $display("LINE_COPY source=%h destination_page=%h cycles=%0d",
      source,destination_page,cycles-copy_start_cycle);
  end
endtask

task clean_line;
  input [21:0] address;
  begin
    @(negedge clk);
    line_clean_address=address; line_clean_start=1;
    @(posedge clk); @(negedge clk);
    line_clean_start=0;
    while(!maintenance_done) @(posedge clk);
    #1;
    if(maintenance_error) $fatal(1,"clean-line command failed");
  end
endtask


integer e,w,before_reads;
reg [15:0] want;
always @(posedge clk) if(valid_sweep && cpu_request_ready) $fatal(1,"request allowed during scrub");
task reset_cache;begin
 @(negedge clk);reset=1;@(posedge clk);@(negedge clk);reset=0;
 while(!cpu_request_ready) @(posedge clk);
end endtask
initial begin
 for(i=0;i<262144;i=i+1)memory[i]=16'hb241 ^ i;
 reset_cache();
 for(e=0;e<4;e=e+1)begin
  access(1,32'h20,16'h65ac,0);
  @(negedge clk);inject_read_error=1;read_error_index=e;
  cpu_write=0;cpu_address=32'h100+16*e;cpu_request_valid=1;
  @(posedge clk);@(negedge clk);cpu_request_valid=0;
  while(!cpu_response_valid)@(posedge clk);
  #1;if(!cpu_error)$fatal(1,"refill error lost at beat %0d",e);
  cpu_response_ready=1;@(posedge clk);#1;cpu_response_ready=0;
  @(negedge clk);inject_read_error=0;
  before_reads=line_reads;access(0,32'h20,0,16'hb261);
  if(line_reads!=before_reads+1)$fatal(1,"refill error did not scrub dirty resident");
  before_reads=line_reads;access(0,32'h100+16*e,0,16'hb241^(16'h100+16*e));
  if(line_reads!=before_reads+1)$fatal(1,"partial refill became resident");
 end
 for(e=0;e<=4;e=e+1)begin
  reset_cache();
  for(w=0;w<16;w=w+1)memory[16'h300+w]=16'h3000+w;
  for(w=0;w<16;w=w+1)access(1,32'h300+w,16'h6000+w,0);
  @(negedge clk);inject_write_error=1;error_after_beats=e;
  line_clean_start=1;line_clean_address=22'h300;
  @(posedge clk);@(negedge clk);line_clean_start=0;
  while(!maintenance_done)@(posedge clk);
  #1;if(!maintenance_error)$fatal(1,"WB prefix error lost at %0d",e);
  @(negedge clk);inject_write_error=0;
  for(w=0;w<16;w=w+1)begin
   want=w<4*e?16'h6000+w:16'h3000+w;
   if(memory[16'h300+w]!==want)$fatal(1,"WB prefix side effect mismatch e=%0d word=%0d",e,w);
  end
  before_reads=line_reads;access(0,32'h300,0,e>0?16'h6000:16'h3000);
  if(line_reads!=before_reads+1)$fatal(1,"WB error did not scrub line");
 end
 $display("ERROR_REFILL_POSITIONS=4 ERROR_WB_PREFIXES=5 DIGITAL_DESIGN_PASS");$finish;
end
endmodule
