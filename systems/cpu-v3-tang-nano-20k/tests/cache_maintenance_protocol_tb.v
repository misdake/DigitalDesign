// Actual CPU, cache and device control. Delayed memory is an independent
// handshake model; expected values come from the program's constants.
module tb;
localparam integer CASE = __CASE__;
localparam [15:0] EXPECTED_RESULT = __RESULT__;
reg clk=0, reset=1;
always #5 clk=~clk;
integer cycles=0;
reg [15:0] rom[0:255];
reg [15:0] memory[0:65535];
wire ireq, irespready;
wire [31:0] iaddr;
wire dreq,dwrite,dready,dresp,drespready,derr;
wire [31:0] daddr;
wire [15:0] dwdata,drdata;
wire copy_start,copy_ready,clean_start;
wire [21:0] copy_source,clean_address;
wire [7:0] copy_page;
wire [2:0] device_index;
wire [3:0] device_channel;
wire device_read_enable,device_write_enable;
wire [15:0] device_write_data,device_read_data;
wire held,dc_clean,dc_invalidate,maint_ready,maint_busy,maint_done,maint_error,sweep;
wire halted,fault;
wire [15:0] halt_signal,pc,fault_pc,code_segment,data_segment;
wire [7:0] fault_code;
wire [31:0] retired_words;
integer pause_count=0;
wire finite_pause = CASE==1 && dresp && pause_count<5;
wire mreq,mwrite,mline,mready,mwready,mresp,mrespready,merror;
wire [21:0] maddr;
wire [63:0] mwdata,mrdata;

__CORE__ core(
 .clk(clk),.reset(reset),.hold(held || sweep || finite_pause),
 .instruction_request_ready(1'b1),.instruction_response_valid(ireq),
 .instruction_data(rom[iaddr[7:0]]),.instruction_error(1'b0),
 .instruction_request_valid(ireq),.instruction_address(iaddr),.instruction_response_ready(irespready),
 .data_request_ready(dready),.data_response_valid(dresp),.data_read_data(drdata),.data_error(derr),
 .data_line_copy_ready(copy_ready),.data_cache_command_error(maint_error),
 .data_request_valid(dreq),.data_write(dwrite),.data_address(daddr),.data_write_data(dwdata),.data_response_ready(drespready),
 .data_line_copy_valid(copy_start),.data_line_copy_source(copy_source),.data_line_copy_destination_page(copy_page),
 .data_line_clean_valid(clean_start),.data_line_clean_address(clean_address),
 .device_index(device_index),.device_channel(device_channel),.device_read_enable(device_read_enable),
 .device_write_enable(device_write_enable),.device_write_data(device_write_data),.device_read_data(device_read_data),
 .halted(halted),.halt_signal(halt_signal),.fault(fault),.fault_code(fault_code),.fault_pc(fault_pc),
 .pc(pc),.code_segment(code_segment),.data_segment(data_segment),.retired_words(retired_words));

__SYSCTL__ sysctl(
 .clk(clk),.reset(reset),.device_index(device_index),.device_channel(device_channel),
 .device_read_enable(device_read_enable),.device_write_enable(device_write_enable),.device_write_data(device_write_data),
 .device_read_data(device_read_data),.dcache_maintenance_ready(maint_ready),
 .dcache_maintenance_done(maint_done),.dcache_maintenance_error(maint_error),
 .watch_read_data(16'd0),.cpu_hold(held),.dcache_clean(dc_clean),.dcache_invalidate(dc_invalidate));

__CACHE__ cache(
 .clk(clk),.reset(reset),.clean_all(dc_clean),.invalidate_all(dc_invalidate),
 .line_copy_start(copy_start),.line_copy_source(copy_source),.line_copy_destination_page(copy_page),
 .line_clean_start(clean_start),.line_clean_address(clean_address),.line_copy_ready(copy_ready),
 .cpu_request_valid(dreq),.cpu_write(dwrite),.cpu_address(daddr),.cpu_write_data(dwdata),.cpu_response_ready(drespready),
 .cpu_request_ready(dready),.cpu_response_valid(dresp),.cpu_read_data(drdata),.cpu_error(derr),
 .memory_request_ready(mready),.memory_write_data_ready(mwready),.memory_response_valid(mresp),
 .memory_read_data(mrdata),.memory_error(merror),.memory_request_valid(mreq),.memory_write(mwrite),
 .memory_line(mline),.memory_address(maddr),.memory_write_data(mwdata),.memory_response_ready(mrespready),
 .maintenance_ready(maint_ready),.maintenance_busy(maint_busy),.maintenance_done(maint_done),
 .maintenance_error(maint_error),.valid_sweep(sweep));

// Idle -> latency -> read beats OR write beats -> latency -> write response.
// Write data becomes externally observable at the successful response edge.
integer mstate=0, delay_left=0, beat=0, base=0, lane;
reg transfer_write=0;
reg [63:0] write_buffer[0:3];
assign mready=mstate==0 && cycles%4!=0;
assign mwready=mstate==3 && cycles%3!=0;
assign mresp=(mstate==2 && cycles%3!=0) || mstate==5;
assign merror=((CASE==5 || CASE==7) && mstate==2 && beat==0) || (CASE==4 && mstate==5);
assign mrdata={memory[base+4*beat+3],memory[base+4*beat+2],memory[base+4*beat+1],memory[base+4*beat]};
always @(posedge clk) begin
 cycles<=cycles+1;
 if(cycles>5000) $fatal(1,"bounded protocol case=%0d pc=%h cache=%0d hold=%b",CASE,pc,cache.state,held);
 if(reset) begin mstate<=0;delay_left<=0;beat<=0;end
 else case(mstate)
 0: if(mreq && mready) begin
     if(!mline || |maddr[3:0]) $fatal(1,"unaligned non-line cache transfer");
     base<=maddr;transfer_write<=mwrite;delay_left<=9;beat<=0;mstate<=1;
 end
 1: if(delay_left!=0) delay_left<=delay_left-1;else mstate<=transfer_write?3:2;
 2: if(mresp && mrespready) begin
     if(merror || beat==3) begin beat<=0;mstate<=0;end else beat<=beat+1;
 end
 3: if(mwready) begin
     write_buffer[beat]<=mwdata;
     if(beat==3) begin beat<=0;delay_left<=6;mstate<=4;end else beat<=beat+1;
 end
 4: if(delay_left!=0) delay_left<=delay_left-1;else mstate<=5;
 5: if(mrespready) begin
     if(!merror) for(lane=0;lane<16;lane=lane+1)
       memory[base+lane]<=write_buffer[lane/4][(lane%4)*16+:16];
     mstate<=0;
 end
 endcase
end

integer commands=0,accepted=0,completed=0,barrier_cycles=0,held_responses=0,fst_tail_cycles=0,wait_ready=0;
integer check_word;
integer fpu_busy_cycles=0;
reg [15:0] expected;
wire command_instruction=core.state==2 && core.instruction[15:8]==8'h78 &&
 (core.instruction[7:4]==1 || core.instruction[7:4]==4);
always @(posedge clk) begin
 if(reset) begin
   commands<=0;accepted<=0;completed<=0;barrier_cycles<=0;
   held_responses<=0;fst_tail_cycles<=0;pause_count<=0;wait_ready<=0;
   fpu_busy_cycles<=0;
 end else begin
   if(core.fpu2_busy) fpu_busy_cycles<=fpu_busy_cycles+1;
   if((dc_clean || dc_invalidate) && !maint_ready) wait_ready<=wait_ready+1;
   if(finite_pause) pause_count<=pause_count+1;
   if(command_instruction && (core.async_store_valid || core.fpu2_pending!=0)) barrier_cycles<=barrier_cycles+1;
   if(command_instruction && core.fpu2_pending!=0) fst_tail_cycles<=fst_tail_cycles+1;
   if(command_instruction && dresp && !drespready) held_responses<=held_responses+1;
   if(device_write_enable && device_index==0 && (device_channel==1 || device_channel==4)) begin
     if(core.async_store_valid || core.fpu2_pending!=0 || core.fpu2_busy)
       $fatal(1,"maintenance device write overtook old CPU/FPU stores");
     commands<=commands+1;
   end
   if((dc_clean || dc_invalidate) && maint_ready) begin
     if(commands!=accepted+1) $fatal(1,"lost or duplicated maintenance request");
     accepted<=accepted+1;
   end
   if(held && (core.async_store_valid || core.fpu2_pending!=0 || dreq))
     $fatal(1,"whole-core hold applied before old work drained");
   if(maint_done && held && !dc_clean && !dc_invalidate) begin
     if(accepted!=completed+1) $fatal(1,"completion not owned by accepted command");
     if(!maint_error) begin
       for(check_word=0;check_word<((CASE==2 || CASE==7)?8:1);check_word=check_word+1) begin
         expected=(CASE==2 || CASE==7)?((check_word%2)?16'h2000:16'h1000)+check_word/2:16'h1234;
         if(memory[16'h4000+check_word]!==expected)
           $fatal(1,"early completion word=%0d got=%h expected=%h",check_word,memory[16'h4000+check_word],expected);
       end
     end
     completed<=completed+1;
   end
 end
end

// Reset an accepted writeback, then restart exactly the same software.
reg reset_once=0;
integer reset_left=0;
always @(negedge clk) begin
 if(reset_left!=0) begin
   reset_left=reset_left-1;if(reset_left==0) reset=0;
 end else if(CASE==6 && !reset && !reset_once && held && mstate==3) begin
   reset_once=1;reset_left=3;reset=1;
 end
end
integer index;
initial begin
 for(index=0;index<256;index=index+1) rom[index]=16'h6c00;
 for(index=0;index<65536;index=index+1) memory[index]=16'h55aa;
 __PROGRAM__
 repeat(3) @(posedge clk);@(negedge clk);reset=0;
 wait(halted || fault);#1;
 if(CASE==5 || CASE==7) begin
   if(!fault || fault_code!=4 || commands!=0 || accepted!=0 || completed!=0)
     $fatal(1,"older fault must stop handoff, fault=%b code=%0d commands=%0d",fault,fault_code,commands);
 end else begin
   if(fault || held || halt_signal!==EXPECTED_RESULT)
     $fatal(1,"completion status/resume failed: fault=%b hold=%b status=%h",fault,held,halt_signal);
   if(commands!=(CASE==3?3:1) || accepted!=commands || completed!=commands)
     $fatal(1,"command/accept/done counts %0d/%0d/%0d",commands,accepted,completed);
   if(CASE<8 && barrier_cycles==0) $fatal(1,"missing overlapping drain coverage");
   if(CASE>=9 && fpu_busy_cycles==0) $fatal(1,"missing real FPU calculation/writeback coverage");
   if(CASE==1 && held_responses<5) $fatal(1,"missing held old response coverage");
   if(CASE==2 && fst_tail_cycles==0) $fatal(1,"missing FST tail overlap coverage");
   if(CASE==8 && wait_ready==0) $fatal(1,"missing real delayed cache acceptance");
   if(CASE==6 && !reset_once) $fatal(1,"missing reset coverage");
 end
 $display("PROTOCOL_PASS case=%0d cycles=%0d cmd/accept/done=%0d/%0d/%0d barrier=%0d held_response=%0d fst_tail=%0d",CASE,cycles,commands,accepted,completed,barrier_cycles,held_responses,fst_tail_cycles);
 $finish;
end
endmodule
