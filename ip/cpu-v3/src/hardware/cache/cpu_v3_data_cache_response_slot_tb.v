module tb;
reg clk=0, reset=1, clean_all=0, invalidate_all=0;
reg line_copy_start=0, line_clean_start=0;
reg [21:0] line_copy_source=0, line_clean_address=0;
reg [7:0] line_copy_destination_page=0;
reg cpu_request_valid=0, cpu_write=0, cpu_response_ready=0;
reg [31:0] cpu_address=0;
reg [15:0] cpu_write_data=0;
reg memory_request_ready=1, memory_response_valid=0, memory_error=0;
reg memory_write_data_ready=0;
reg [63:0] memory_read_data=0;
wire line_copy_ready, cpu_request_ready, cpu_response_valid, cpu_error;
wire [15:0] cpu_read_data;
wire memory_request_valid, memory_write, memory_line, memory_response_ready;
wire [21:0] memory_address;
wire [63:0] memory_write_data;
wire maintenance_busy, maintenance_done, maintenance_error, valid_sweep;
CpuV3DataCache dut(.*);
always #5 clk=~clk;
integer cycles=0, word_index, prefix, beat, scenario=0;
reg [21:0] address, line_address;
reg [15:0] expected;
always @(posedge clk) begin
  cycles <= cycles+1;
  if(cycles>30000) $fatal(1,"response-slot cycle limit");
end

function [15:0] pattern;
  input [21:0] at;
  begin pattern=16'h5371 ^ at[15:0] ^ {at[6:0],9'b0}; end
endfunction

task cold_read;
  input integer word_at, fail_beat;
  begin
    scenario=scenario+1;
    address=22'h1000 + scenario*16 + word_at;
    line_address=address & 22'h3ffff0;
    expected=pattern(address);
    while(!cpu_request_ready) @(negedge clk);
    cpu_request_valid=1; cpu_write=0; cpu_address=address;
    @(posedge clk); @(negedge clk); cpu_request_valid=0;
    while(!memory_request_valid) @(negedge clk);
    if(memory_write || !memory_line || memory_address!==line_address)
      $fatal(1,"unexpected read request");
    @(posedge clk); @(negedge clk);
    for(beat=0; beat<4 && (fail_beat<0 || beat<=fail_beat); beat=beat+1) begin
      // Deliberate gaps: an early-captured halfword may not publish success.
      repeat(beat+1) begin
        if(cpu_response_valid) $fatal(1,"response published before terminal beat");
        @(posedge clk); @(negedge clk);
      end
      memory_response_valid=1;
      memory_error=(beat==fail_beat);
      memory_read_data={pattern(line_address+4*beat+3),
                        pattern(line_address+4*beat+2),
                        pattern(line_address+4*beat+1),
                        pattern(line_address+4*beat)};
      @(posedge clk); #1;
      if(beat==fail_beat) begin
        if(!cpu_response_valid || !cpu_error || cpu_read_data!==0)
          $fatal(1,"late error did not override captured word=%0d prefix=%0d",word_at,fail_beat);
      end else begin
        if(cpu_response_valid !== (beat==3)) $fatal(1,"response calendar changed");
        if(beat >= word_at/4 && cpu_read_data!==expected)
          $fatal(1,"final slot not captured directly word=%0d beat=%0d",word_at,beat);
      end
      @(negedge clk); memory_response_valid=0; memory_error=0;
    end
    if(!cpu_response_valid || cpu_error !== (fail_beat>=0))
      $fatal(1,"missing terminal result");
    // Backpressure holds the result and rejects another request.
    cpu_request_valid=1; cpu_address=address+16;
    repeat(6) begin
      if(cpu_request_ready || !cpu_response_valid ||
          cpu_read_data !== (fail_beat>=0 ? 16'd0 : expected))
        $fatal(1,"held response corrupted");
      @(posedge clk); @(negedge clk);
    end
    cpu_request_valid=0; cpu_response_ready=1;
    @(posedge clk); #1;
    if(cpu_response_valid) $fatal(1,"response not released");
    @(negedge clk); cpu_response_ready=0;
  end
endtask

initial begin
  repeat(2) @(posedge clk);
  @(negedge clk); reset=0;
  for(word_index=0;word_index<16;word_index=word_index+1)
    cold_read(word_index,-1);
  for(prefix=0;prefix<4;prefix=prefix+1)
    for(word_index=0;word_index<16;word_index=word_index+1)
      cold_read(word_index,prefix);
  $display("DIGITAL_DESIGN_PASS response_slot 16 success + 64 error-prefix scenarios");
  $finish;
end
endmodule
