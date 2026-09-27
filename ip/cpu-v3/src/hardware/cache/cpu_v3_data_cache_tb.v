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
wire [3:0] read_index = 4-read_remaining;
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
    read_remaining <= read_remaining-1;
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

  // Overlapped scan across windows: dirty entries 2 (window 0), 36 (window
  // 2), and 66 (window 4) must be written back while the scan runs ahead of
  // the in-flight write-back.
  access(1,32'h0022,16'h1111,0);
  access(1,32'h0242,16'h3333,0);
  access(1,32'h0422,16'h2222,0);
  i=line_writes;
  maintain(0);
  if(line_writes!=i+3) $fatal(1,"clean did not write back all dirty lines");
  if(memory[16'h022]!==16'h1111 || memory[16'h242]!==16'h3333 ||
     memory[16'h422]!==16'h2222)
    $fatal(1,"overlapped scan wrote back wrong data");
  i=line_reads;
  access(0,32'h0022,0,16'h1111);
  access(0,32'h0242,0,16'h3333);
  access(0,32'h0422,0,16'h2222);
  if(line_reads!=i) $fatal(1,"clean invalidated a resident line");

  // A dirty line outside window zero is found by the window scan alone.
  maintain(1);
  access(1,32'h0203,16'h4444,0);
  i=line_writes;
  maintain(0);
  if(line_writes!=i+1 || memory[16'h203]!==16'h4444)
    $fatal(1,"window scan missed a dirty line outside window zero");

  // Zero-dirty clean is a no-op; zero-dirty invalidate still sweeps valids.
  i=line_writes;
  maintain(0);
  if(line_writes!=i) $fatal(1,"clean of a clean cache reached memory");
  maintain(1);
  i=line_reads;
  access(0,32'h0203,0,16'h4444);
  if(line_reads!=i+1) $fatal(1,"swept invalidate left the line resident");

  // A cold source refills once, then reuses the ordinary 64-bit write-back
  // path at the same offset in another physical segment. The source remains
  // resident and a hot repeat produces no memory read.
  for(i=16'h310;i<16'h320;i=i+1) memory[i]=16'ha000+i;
  i=line_reads;
  copy_line(22'h000310,6'h01);
  if(line_reads!=i+1) $fatal(1,"cold line copy did not refill exactly once");
  if(line_writes==0) $fatal(1,"line copy did not use write-back path");
  for(i=0;i<16;i=i+1)
    if(memory[22'h004310+i] !== (16'ha310+i))
      $fatal(1,"cold line copy mismatch at word %0d: %h != %h",
        i,memory[22'h004310+i],16'ha310+i);
  i=line_reads;
  copy_line(22'h000310,6'h02);
  if(line_reads!=i) $fatal(1,"hot line copy unexpectedly refilled");
  for(i=0;i<16;i=i+1)
    if(memory[22'h008310+i] !== (16'ha310+i))
      $fatal(1,"hot line copy mismatch at word %0d",i);

  // A resident dirty destination alias is obsolete because the copy
  // overwrites the complete line. Invalidate it directly without writing its
  // stale contents or refilling it as part of the copy.
  access(0,32'h00008312,0,16'ha312);
  access(1,32'h00008312,16'hdead,0);
  i=line_writes;
  j=line_reads;
  copy_line(22'h000310,6'h02);
  if(line_writes!=i+1) $fatal(1,"copy wrote stale dirty destination alias");
  if(line_reads!=j) $fatal(1,"copy unnecessarily refreshed destination alias");
  i=line_reads;
  access(0,32'h00008312,0,16'ha312);
  if(line_reads!=i+1) $fatal(1,"copy did not invalidate destination alias");

  // Redirecting a dirty source must not clear its source dirty bit: a later
  // clean still writes the modified line to the original segment.
  access(1,32'h00000312,16'h5a5a,0);
  copy_line(22'h000310,6'h03);
  if(memory[22'h00c312]!==16'h5a5a) $fatal(1,"copy lost dirty source data");
  i=line_writes;
  maintain(0);
  if(line_writes!=i+1 || memory[22'h000312]!==16'h5a5a)
    $fatal(1,"redirected write-back incorrectly cleaned source line");

  // A line dirtied word-by-word must copy the complete resident contents,
  // not only the word left on the RAM read ports by the final store.
  for(i=0;i<16;i=i+1)
    access(1,32'h00000500+i,16'h6000+i,0);
  copy_line(22'h000500,6'h04);
  for(i=0;i<16;i=i+1)
    if(memory[22'h010500+i] !== (16'h6000+i))
      $fatal(1,"multiword dirty line copy mismatch at word %0d: %h != %h",
        i,memory[22'h010500+i],16'h6000+i);

  // A clean-line command writes one dirty hit while retaining it.
  access(1,32'h00000150,16'h6b6b,0);
  i=line_writes; j=line_reads;
  clean_line(22'h000150);
  if(line_writes!=i+1 || memory[22'h000150]!==16'h6b6b)
    $fatal(1,"clean-line hint did not write dirty hit");
  access(0,32'h00000150,0,16'h6b6b);
  if(line_reads!=j) $fatal(1,"clean-line hint invalidated its line");

  // Alignment is part of the hardware primitive's contract.
  @(negedge clk); line_copy_source=22'h311; line_copy_start=1;
  @(posedge clk); @(negedge clk); line_copy_start=0;
  while(!maintenance_done) @(posedge clk);
  #1;
  if(!maintenance_error) $fatal(1,"unaligned line copy was accepted");

  // Errors may arrive before the first data-ready edge. The cache must
  // leave STREAM, consume the held error and finish maintenance/CPU faults.
  access(1,32'h00000150,16'h9090,0);
  @(negedge clk); inject_write_error=1;
  line_clean_address=22'h000150; line_clean_start=1;
  @(posedge clk); @(negedge clk); line_clean_start=0;
  while(!maintenance_done) @(posedge clk);
  #1; if(!maintenance_error) $fatal(1,"early clean write error lost");
  @(negedge clk); inject_write_error=0;
  access(1,32'h00000150,16'h9191,0);
  @(negedge clk); inject_write_error=1;
  line_copy_source=22'h000150; line_copy_destination_page=8'h05; line_copy_start=1;
  @(posedge clk); @(negedge clk); line_copy_start=0;
  while(!maintenance_done) @(posedge clk);
  #1; if(!maintenance_error) $fatal(1,"early copy write error lost");
  @(negedge clk); inject_write_error=0;
  access(1,32'h00000150,16'h9292,0);
  access(1,32'h00000550,16'h9393,0);
  while(!cpu_request_ready) @(posedge clk);
  @(negedge clk); inject_write_error=1;
  cpu_address=32'h00000950; cpu_write=0; cpu_request_valid=1;
  @(posedge clk); @(negedge clk); cpu_request_valid=0;
  while(!cpu_response_valid) @(posedge clk);
  #1; if(!cpu_error) $fatal(1,"early eviction write error lost");
  cpu_response_ready=1; @(posedge clk); #1; cpu_response_ready=0;
  @(negedge clk); inject_write_error=0;
  access(0,32'h00000950,0,16'h8950);
  if(failed_writes!=3) $fatal(1,"early-error paths were not exercised");
  for(j=1;j<=2;j=j+1) begin
    access(1,32'h00000150,16'h9494+j,0);
    @(negedge clk); inject_write_error=1; error_after_beats=2*j;
    line_clean_address=22'h000150; line_clean_start=1;
    @(posedge clk); @(negedge clk); line_clean_start=0;
    while(!maintenance_done) @(posedge clk);
    #1; if(!maintenance_error) $fatal(1,"partial/final write error lost");
    @(negedge clk); inject_write_error=0;
  end
  if(failed_writes!=5 || failed_accepted_beats!=6)
    $fatal(1,"partial/final error coverage missing");

  if (request_stall_cycles == 0) $fatal(1,"request-stall path was not exercised");
  if (stalled_beats != 15 || accepted_beats != (line_writes - failed_writes) * 4 + failed_accepted_beats)
    $fatal(1,"missing stall/beat coverage mask=%h accepted=%0d writes=%0d",
      stalled_beats,accepted_beats,line_writes);
  $display("WRITEBACK_STALL_MASK=%h ACCEPTED_BEATS=%0d",stalled_beats,accepted_beats);
  $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
