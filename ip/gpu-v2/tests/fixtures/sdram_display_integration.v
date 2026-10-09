`timescale 1ns/1ps
module tb;
__PARAMS__
localparam STARVE=__STARVE__, PHASE=__PHASE__;
reg controller_clk=0,clk=0,phy_clk=0,pixel_clock=0;
wire logic_clk=clk, sdram_clk=phy_clk, serial_clock=1'b0;
reg reset=1,video_locked=0;
always #5.051 controller_clk=~controller_clk;
always @(posedge controller_clk) clk=~clk;
initial begin #13.259; forever begin phy_clk=~phy_clk; #5.051; end end
initial begin #(PHASE==0 ? 0 : 7.117); forever #(__PIXEL_HALF__) pixel_clock=~pixel_clock; end
wire memory_request_valid,memory_urgent,underflow;
wire [21:0] memory_address;
integer published_groups=0;
__PORTS__
wire O_sdram_clk,O_sdram_cke,O_sdram_cs_n,O_sdram_cas_n,O_sdram_ras_n,O_sdram_wen_n;
wire [3:0] O_sdram_dqm; wire [10:0] O_sdram_addr; wire [1:0] O_sdram_ba; wire [31:0] IO_sdram_dq;
GowinSdramCombination mc(.*);
integer refreshes;
LabPinModel #(.PERIOD_NS(10.102),.RETURN_MIN(1),.RETURN_MAX(4)) pin(
 .sclk(O_sdram_clk),.reset(reset),.cke(O_sdram_cke),.cs(O_sdram_cs_n),
 .ras(O_sdram_ras_n),.cas(O_sdram_cas_n),.we(O_sdram_wen_n),.dqm(O_sdram_dqm),
 .a(O_sdram_addr),.ba(O_sdram_ba),.dq(IO_sdram_dq),.cycle(),.refreshes(refreshes));
FramebufferHdmi display(.clk(clk),.reset(reset),.pixel_clock(pixel_clock),.serial_clock(serial_clock),.video_locked(video_locked),
 .memory_request_ready(display_request_ready),.memory_data_valid(display_response_valid),
 .memory_read_data(display_read_data),.memory_last(display_response_last),.memory_error(display_error),
 .memory_request_valid(memory_request_valid),.memory_urgent(memory_urgent),.memory_address(memory_address),.underflow(underflow),
 .device_index(3'd3),.device_channel(4'd0),.device_read_enable(1'b0),.device_write_enable(1'b0),.device_write_data(16'd0),
 .device_read_data(),.tmds_clk_p(),.tmds_clk_n(),.tmds_data_p(),.tmds_data_n());

function [15:0] pattern;
 input integer x,y; begin pattern=x*73+y*977; end
endfunction
function [23:0] color;
 input [15:0] p; begin color={p[15:11],p[15:13],p[10:5],p[10:9],p[4:0],p[4:2]}; end
endfunction
function [20:0] native_word;
 input integer bytes; integer word_address;
 begin word_address=bytes/4; native_word=((word_address>>5)&3)*524288+(word_address&31)+((word_address>>7)<<5); end
endfunction
integer seed,x,y,address;
initial begin
 // Independent coordinate goldens are injected into SDRAM, never into a
 // controller response or line buffer. CPU/GPU use disjoint scratch regions.
 for(seed=0;seed<16384;seed=seed+4) begin
  pin.seed_word(native_word(32'h20000+seed),32'ha5a5a5a5);
  pin.seed_word(native_word(32'h30000+seed),32'ha5a5a5a5);
  pin.seed_word(native_word(32'h100000+seed),32'ha5a5a5a5);
  pin.seed_word(native_word(32'h80000+seed),32'ha5a5a5a5);
  pin.seed_word(native_word(32'hc0000+seed),32'ha5a5a5a5);
 end
 for(y=0;y<FB_HEIGHT;y=y+1) for(x=0;x<FB_WIDTH;x=x+2) begin
  address=2*(22'h200000+(y/16)*TILE_ROW_STRIDE+(x/16)*256+(y%16)*16+x%16);
  pin.seed_word(native_word(address),{pattern(x+1,y),pattern(x,y)});
 end
 repeat(32) @(negedge clk);
 reset=0;
 // Independent video PLL lock delay moves active demand relative to the
 // native refresh epoch and fixed CPU arrivals, rather than shifting all.
 repeat(PHASE) @(negedge clk);
 video_locked=1;
end

integer cycles=0,request_count=0,complete_count=0,group_beats=0;
integer pixels=0,lines=0,column=0,source_y=0,pixel_cycles=0;
integer initial100=0,first_visible=0,first_publish=0,max_fill=0,max_return_publish=0;
integer release_stamp[0:1],last_return[0:1],group_start[0:1];
integer max_release_fill=0,max_publish_cdc=0,cpu_i_jobs=0,cpu_d_jobs=0,fb_reads=0,fb_writes=0;
integer i_number=0,d_number=0,fr_number=0,fw_number=0,i_beats=0,d_beats=0;
reg i_active=0,d_active=0,fr_active=0,fw_active=0;
reg [1:0] old_published=0,old_released=0;
integer request_y,request_x,delta,k;
initial begin for(k=0;k<2;k=k+1) begin release_stamp[k]=-1;last_return[k]=0;group_start[k]=0;end end
always @(posedge clk) begin
 cycles=cycles+1;
 if(cycles>1150000) $fatal(1,"integration cycle watchdog started=%d visible=%0d underflow=%d",display.started,lines,underflow);
 if(!reset) begin
  if(display_request_valid && display_request_ready) begin
   request_y=(request_count/50)*2+(request_count%2);
   request_x=((request_count%50)/2)*16;
   if(memory_address !== 22'h200000+(request_y/16)*TILE_ROW_STRIDE+(request_y%16)*16+(request_x/16)*256)
    $fatal(1,"wrong/duplicate/missing tiled request %0d address=%h",request_count,memory_address);
   if(request_count%50==0) group_start[(request_count/50)%2]=cycles;
   request_count=request_count+1;
  end
  if(display_response_valid) begin
   if(display_error || display_response_last !== (group_beats==3)) $fatal(1,"display segment protocol failure");
   if(display_response_last) begin
    group_beats=0; complete_count=complete_count+1;
    if(complete_count%50==0) last_return[(complete_count/50-1)%2]=cycles;
    if(complete_count==100)begin initial100=cycles;$display("NODE initial100 complete cycle=%0d",cycles);end
   end else group_beats=group_beats+1;
  end
  for(k=0;k<2;k=k+1) begin
   if(display.released[k]!=old_released[k]) release_stamp[k]=cycles;
   if(display.published[k]!=old_published[k]) begin
    if(complete_count!=(published_groups+1)*50 || group_beats!=0) $fatal(1,"partial/duplicate group publication");
    published_groups=published_groups+1;
    if(first_publish==0) first_publish=cycles;
    delta=cycles-group_start[k];if(delta>max_fill)max_fill=delta;
    delta=cycles-last_return[k];if(delta>max_return_publish)max_return_publish=delta;
    if(release_stamp[k]>=0) begin delta=cycles-release_stamp[k];if(delta>max_release_fill)max_release_fill=delta;end
   end
  end
  old_published=display.published;old_released=display.released;
  // Persistent one-job-per-client source: CPU at configured average rates,
  // framebuffer read/write saturated with independent data and row changes.
  if(cycles%1728==17 && !i_active && !instruction_request_valid) begin
   instruction_request_valid<=1;instruction_address<=22'h80000+(i_number%4)*64;i_number=i_number+1;
  end
  if(instruction_request_valid && instruction_request_ready)begin instruction_request_valid<=0;i_active<=1;end
  if(instruction_response_valid)begin
   if(instruction_error || instruction_read_data!==64'ha5a5a5a5a5a5a5a5)$fatal(1,"CPU instruction data");
   if(i_beats==3)begin i_active<=0;cpu_i_jobs=cpu_i_jobs+1;i_beats=0;end
   else i_beats=i_beats+1;
  end
  if(cycles%288==43 && !d_active && !data_request_valid)begin
   data_request_valid<=1;data_address<=22'h10000+(d_number%2)*22'h8000+(d_number%4)*64;
   data_write<=d_number%3==2;data_write_data<=64'ha5a5a5a5a5a5a5a5;d_number=d_number+1;
  end
  if(data_request_valid && data_request_ready)begin data_request_valid<=0;d_active<=1;end
  if(data_response_valid)begin
   if(data_error || (!data_write && data_read_data!==64'ha5a5a5a5a5a5a5a5))$fatal(1,"CPU data result");
   if(data_write || d_beats==3)begin d_active<=0;cpu_d_jobs=cpu_d_jobs+1;d_beats=0;end
   else d_beats=d_beats+1;
  end
  if(!fr_active && !gpu_fb_r_request_valid)begin
   gpu_fb_r_request_valid<=1;gpu_fb_r_address<=22'h60000+(fr_number%32)*256;
   gpu_fb_r_line_count_minus_1<=__FB_COUNT__;fr_number=fr_number+1;
  end
  if(gpu_fb_r_request_valid && gpu_fb_r_request_ready)begin gpu_fb_r_request_valid<=0;fr_active<=1;end
  if(gpu_fb_r_response_valid)begin
   if(gpu_fb_r_error || gpu_fb_r_read_data!==64'ha5a5a5a5a5a5a5a5)$fatal(1,"FB read data");
   if(gpu_fb_r_response_last)begin fr_active<=0;fb_reads=fb_reads+1;end
  end
  if(!fw_active && !gpu_fb_w_request_valid)begin
   gpu_fb_w_request_valid<=1;gpu_fb_w_address<=22'h40000+(fw_number%32)*256;
   gpu_fb_w_write<=1;gpu_fb_w_write_data<=64'h0123456789abcdef;
   gpu_fb_w_line_count_minus_1<=__FB_COUNT__;fw_number=fw_number+1;
  end
  if(gpu_fb_w_request_valid && gpu_fb_w_request_ready)begin gpu_fb_w_request_valid<=0;fw_active<=1;end
  if(gpu_fb_w_response_valid)begin
   if(gpu_fb_w_error || !gpu_fb_w_response_last)$fatal(1,"FB write completion");
   fw_active<=0;fb_writes=fb_writes+1;
  end
 end
end

reg old_visible=0;reg [1:0] old_publish_sync=0;
integer pixel_slot,pixel_delta,publish_visible[0:1],min_ready_lead=32'h7fffffff;
initial begin publish_visible[0]=0;publish_visible[1]=0;end
wire visible=display.visible_pipe3 && display.framebuffer_pipe3;
always @(posedge pixel_clock) begin
 pixel_cycles=pixel_cycles+1;
 if(!display.pixel_reset) begin
  for(pixel_slot=0;pixel_slot<2;pixel_slot=pixel_slot+1) if(display.publish_sync[pixel_slot]!=old_publish_sync[pixel_slot])begin
   pixel_delta=cycles-last_return[pixel_slot];if(pixel_delta>max_publish_cdc)max_publish_cdc=pixel_delta;
   publish_visible[pixel_slot]=cycles;
  end
  old_publish_sync=display.publish_sync;
  if(underflow) begin
   if(STARVE && lines>=8 && published_groups==6) begin
    $display("PASS expected production underflow lines=%0d published=%0d",lines,published_groups);$finish;
   end
   $fatal(1,"unexpected real display underflow lines=%0d y=%0d",lines,source_y);
  end
  if(display.started && display.h_count==0 && display.v_count>=V_ACTIVE_START && display.v_count<V_ACTIVE_END &&
     display.vertical_repeat==0 && !display.display_second && lines>=8 && display.line_ready)begin
   pixel_delta=cycles-publish_visible[display.display_slot];if(pixel_delta<min_ready_lead)min_ready_lead=pixel_delta;
  end
  if(visible && !old_visible)begin
   column=0;source_y=lines/SCALE;
   if(first_visible==0)begin first_visible=cycles;$display("NODE scanout started cycle=%0d",cycles);end
  end
  if(visible) begin
   if(STARVE && published_groups==6 && source_y>=12 && !display.scan_line_ready)begin
    if(display.rgb_pipe!==24'd0)$fatal(1,"unpublished row exposed stale data");
   end else if(display.rgb_pipe!==color(pattern(column/SCALE,source_y)))
    $fatal(1,"wrong/stale pixel x=%0d y=%0d got=%h expected=%h",column,source_y,display.rgb_pipe,color(pattern(column/SCALE,source_y)));
   column=column+1;pixels=pixels+1;
  end
  if(!visible && old_visible)begin
   if(column!=FB_WIDTH*SCALE)$fatal(1,"incomplete/duplicate output row %0d",column);
   lines=lines+1;
   if(lines==48 && !STARVE) begin
    if(initial100==0 || first_visible<=initial100 || published_groups<13 || refreshes<100 ||
       cpu_i_jobs<10 || cpu_d_jobs<100 || fb_reads<100 || fb_writes<100)
     $fatal(1,"required coverage initial100=%0d first_visible=%0d groups=%0d refreshes=%0d CPU=%0d/%0d FB=%0d/%0d",initial100,first_visible,published_groups,refreshes,cpu_i_jobs,cpu_d_jobs,fb_reads,fb_writes);
    $display("PASS production display integration cycles=%0d pixels=%0d lines=%0d first_publish=%0d initial100=%0d first_visible=%0d max_fill=%0d max_release_fill=%0d return_publish=%0d return_cdc=%0d min_ready_lead=%0d groups=%0d refreshes=%0d cpu_i=%0d cpu_d=%0d fb_r=%0d fb_w=%0d",cycles,pixels,lines,first_publish,initial100,first_visible,max_fill,max_release_fill,max_return_publish,max_publish_cdc,min_ready_lead,published_groups,refreshes,cpu_i_jobs,cpu_d_jobs,fb_reads,fb_writes);
    $finish;
   end
  end
  old_visible=visible;
 end
end
endmodule
