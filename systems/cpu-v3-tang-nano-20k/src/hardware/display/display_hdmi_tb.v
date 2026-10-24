`timescale 1ns/1ps
module tb;
// The active mode's localparam block is injected so the checks below follow
// whichever HDMI mode the DUT was compiled with.
__DISPLAY_CONFIG__
reg clk=0,pixel_clock=0,serial_clock=0,reset=1,video_locked=0;
reg boot_enable=0,boot_failed=0;
wire started_ack,startup_error;
always #9.259 clk=~clk;
always #(__PIXEL_HALF_PERIOD__) pixel_clock=~pixel_clock;
always #1 serial_clock=~serial_clock;
reg memory_request_ready=1,memory_data_valid=0,memory_last=0,memory_error=0;
reg [63:0] memory_read_data=0;
reg [2:0] device_index=3; reg [3:0] device_channel=0;
reg device_read_enable=0,device_write_enable=0; reg [15:0] device_write_data=0;
wire memory_request_valid,memory_urgent,underflow,tmds_clk_p,tmds_clk_n;
wire [21:0] memory_address; wire [2:0] tmds_data_p,tmds_data_n;
wire [15:0] device_read_data;
FramebufferHdmi dut(.*);
reg observed_start=0;
always @(negedge pixel_clock) begin
 if(dut.pixel_reset) observed_start=0;
 else if(dut.started && !observed_start) begin
  if(dut.h_count!=0 || dut.v_count!=0 ||
     (dut.publish_sync ^ dut.released)!=2'b11)
   $fatal(1,"startup did not use two complete groups at frame origin");
  observed_start=1;
 end
end
integer beat=0,requests=0,cycles=0,bursts=0;
integer memory_cycles=0;
integer col=0,bad=0,sampled=0,border_sampled=0,de_col=0,de_runs=0;
integer old_frame=0;
integer frame_requests=0,completed_source_frames=0,source_row,source_tile_x;
integer request_row=0,request_x=0,pixel_y=0,linear_samples=0,raw_samples=0;
integer table_i;
real table_linear,table_encoded;
reg [7:0] expected_table [0:63];
reg [15:0] expected_pixel;
reg [23:0] expected_rgb;
reg monitor_linear=0;
initial begin
 for(table_i=0;table_i<64;table_i=table_i+1) begin
  table_linear=table_i/63.0;
  if(table_linear<=0.0031308) table_encoded=12.92*table_linear;
  else table_encoded=1.055*(table_linear**(1.0/2.4))-0.055;
  expected_table[table_i]=$rtoi(255.0*table_encoded+0.5);
 end
end
function [15:0] pattern;
 input integer x,y;
 begin pattern=x*73+y*977; end
endfunction
function [23:0] color;
 input [15:0] p; input linear;
 begin
  if(linear) color={expected_table[{p[15:11],p[15]}],expected_table[p[10:5]],expected_table[{p[4:0],p[4]}]};
  else color={p[15:11],p[15:13],p[10:5],p[10:9],p[4:0],p[4:2]};
 end
endfunction
reg [15:0] wa;
reg [15:0] base=0;
reg vis_d=0,fb_vis_d=0;
reg hsync_d=0,vsync_d=0;
reg saw_second_base=0;
reg previous_frame_complete=0;
wire vis = dut.visible_pipe3;
wire fb_vis = dut.framebuffer_pipe3 && vis;

// Every wait below must complete within the roughly three-frame scenario.
initial begin
 repeat(10*H_TOTAL*V_TOTAL) @(posedge pixel_clock);
 $fatal(1,"display test exceeded six frames");
end

task device_write;
 input [3:0] channel; input [15:0] value;
 begin
  @(negedge clk); device_channel=channel; device_write_data=value; device_write_enable=1;
  @(negedge clk); device_write_enable=0;
 end
endtask

task expect_status;
 input [15:0] mask; input [15:0] expected;
 begin
  @(negedge clk); device_channel=3; device_read_enable=1;
  #1 if ((device_read_data & mask) !== expected) $fatal(1,"status %h mask %h expected %h",device_read_data,mask,expected);
  device_read_enable=0;
 end
endtask
always @(posedge clk) begin
 if(!reset && !dut.front_valid && (memory_request_valid || dut.line_write))
  $fatal(1,"unpresented framebuffer was accessed");
 memory_cycles=memory_cycles+1;
 memory_request_ready<=memory_cycles%19>=4;
 previous_frame_complete<=dut.frame_complete;
 if(dut.frame_complete && !previous_frame_complete) begin
  if(frame_requests!=(dut.front_valid ? 6000 : 0))
   $fatal(1,"source frame used %0d segments instead of 6000",frame_requests);
  if(dut.front_valid)completed_source_frames<=completed_source_frames+1;
  frame_requests<=0;
 end
 memory_data_valid<=0; memory_last<=0;
 // Ignore requests while the DUT is in reset: its fill pointers do not
 // advance there, so accepting would desynchronize the burst count.
 if (!reset && memory_request_valid&&memory_request_ready) begin
  beat<=1; requests<=requests+1; bursts<=bursts+1;
  request_row=(frame_requests/50)*2+(frame_requests%2);
  request_x=((frame_requests%50)/2)*16;
 end
 else if(beat!=0 && memory_cycles%7!=0) begin
  // Tile-linear fill: a 32-byte segment carries 16 consecutive pixels of one
  // source row across four 64-bit beats. Each 64-bit beat holds four 16-bit
  // words (low word first), with a coordinate-dependent pattern for every pixel.
  wa = request_x + (beat-1)*4;
  memory_data_valid<=1;
  memory_read_data<={pattern(wa+3,request_row),pattern(wa+2,request_row),pattern(wa+1,request_row),pattern(wa,request_row)};
  memory_last<=beat==4;
  if(beat==4) beat<=0; else beat<=beat+1;
 end
 if (!reset && memory_request_valid && memory_request_ready) begin
  source_row=(frame_requests/50)*2+(frame_requests%2);
  source_tile_x=(frame_requests%50)/2;
  if(memory_address!==dut.active_base+(source_row/16)*6400+(source_row%16)*16+source_tile_x*256)
   $fatal(1,"tile-linear request %0d address %h is out of sequence",frame_requests,memory_address);
  frame_requests<=frame_requests+1;
  if (memory_address==22'h3e8900) saw_second_base<=1;
  if (memory_address<dut.active_base || {1'b0,memory_address}>={1'b0,dut.active_base}+23'd96000)
   $fatal(1,"request %h outside active framebuffer %h",memory_address,dut.active_base);
 end
end
initial begin
 repeat(4) @(posedge clk); reset=0; video_locked=1;
 repeat(H_TOTAL*V_TOTAL+1000) @(posedge pixel_clock);
 if(requests!=0 || dut.frame_index!=0 || dut.released!=0 || underflow || started_ack)
  $fatal(1,"cold disabled Display made progress");
 @(negedge clk);boot_enable=1;
 // A command with only half an address is ignored and keeps the staged half.
 device_write(1,16'h8000);
 expect_status(16'h0003,16'h0002);
 device_write(3,16'h0001);
 expect_status(16'h0003,16'h0002);

 // Repeated low/high writes overwrite the shadow address, but do not publish it.
 device_write(1,16'h0000);
 device_write(1,16'h8900);
 device_write(2,16'h0020);
 device_write(2,16'h003e);
 expect_status(16'h0003,16'h0000);
 old_frame=dut.frame_index;
 wait(dut.frame_index!=old_frame);
 if(dut.active_base!==22'h200000)
  $fatal(1,"partial or unsubmitted address changed active base");
 if(requests!=0 || dut.front_valid || dut.scan_valid || underflow)
  $fatal(1,"black startup fetched or displayed uninitialized memory");

 // The first explicit same-base present activates A. Until both groups have
 // arrived and crossed a frame origin, visible video must stay black.
 device_write(1,16'h0000);device_write(2,16'h0020);device_write(3,16'h0001);
 wait(dut.front_valid);
 wait(dut.scan_valid);
 old_frame=dut.frame_index;
 wait(dut.frame_index!=old_frame);
 // Restore the staged address used by the following pending-swap tests.
 device_write(1,16'h8900);device_write(2,16'h003e);

 // NEXT_SWAP snapshots the complete shadow address. Later staging cannot mutate
 // this pending swap, even though writes remain accepted while it is pending.
 device_write(4,16'h0001);
 device_write(3,16'h0001);
 device_write(4,16'h0000);
 if(!dut.pending_linear) $fatal(1,"staging mutated pending color format");
 expect_status(16'h0001,16'h0001);
 device_write(1,16'h0000);
 if(dut.pending_base!==22'h3e8900) $fatal(1,"staging mutated maximum pending base: %h",dut.pending_base);

 // Alignment and upper/range failures belong to their staged halves.
 // A valid write to the other half must not erase the invalid predicate.
 device_write(1,16'h8901); device_write(2,16'h003e); device_write(3,16'h0001);
 expect_status(16'h0005,16'h0005);
 if(dut.pending_base!==22'h3e8900) $fatal(1,"misaligned shadow replaced pending");
 device_write(2,16'h0040); device_write(1,16'h8900); device_write(3,16'h0001);
 expect_status(16'h0005,16'h0005);
 if(dut.pending_base!==22'h3e8900) $fatal(1,"invalid high half replaced pending");
 device_write(4,16'h0001);
 device_write(1,16'h8900); device_write(2,16'h003e); device_write(3,16'h0001);
 device_write(4,16'h0000);
 expect_status(16'h0005,16'h0001);
 device_write(1,16'h8910); device_write(2,16'h003e); device_write(3,16'h0001);
 expect_status(16'h0005,16'h0005);
 if(dut.pending_base!==22'h3e8900) $fatal(1,"above-limit aligned shadow replaced pending");
 device_write(1,16'h0000);

 // An out-of-range complete pair is rejected and cannot replace the pending base.
 device_write(2,16'h0040);
 device_write(3,16'h0001);
 expect_status(16'h0005,16'h0005);
 wait(dut.active_base==22'h3e8900);
 if(!dut.active_linear) $fatal(1,"linear format did not follow swap");

 // A later complete submission replaces pending normally. If it lands on the
 // exact clock that applies the old pending base, it remains queued for the
 // following frame rather than being lost.
 device_write(1,16'h0000);
 device_write(2,16'h0020);
 device_write(3,16'h0001);
 if(dut.pending_base!==22'h200000) $fatal(1,"first replacement was not submitted");
 device_write(1,16'h8000);
 device_write(2,16'h0021);
 while (!(dut.frame_sync!=dut.frame_seen && dut.frame_complete)) @(negedge clk);
 device_channel=3; device_write_data=16'h0001; device_write_enable=1;
 @(negedge clk); device_write_enable=0;
 if(dut.active_base!==22'h200000) $fatal(1,"old pending base was not applied");
 if(!dut.next_pending || dut.pending_base!==22'h218000)
  $fatal(1,"simultaneous replacement was lost");
 repeat(1000) @(posedge pixel_clock);
 if(dut.active_base!==22'h200000) $fatal(1,"replacement base was not applied");
 if(requests<4800) $fatal(1,"insufficient line fetches %0d",requests);
 if(completed_source_frames<2) $fatal(1,"too few complete 6000-segment source frames");
 if(!saw_second_base) $fatal(1,"maximum legal framebuffer was never fetched");
 if(dut.frame_index<3) $fatal(1,"frame index did not advance enough: %0d",dut.frame_index);
 if(underflow) $fatal(1,"unexpected underflow");
 if(tmds_clk_n!==~tmds_clk_p || tmds_data_n!==~tmds_data_p) $fatal(1,"bad differential outputs");
 if(bad>0) $fatal(1,"pixel mismatches: %0d of %0d sampled",bad,sampled);
 if(sampled<100000) $fatal(1,"too few visible pixels sampled: %0d",sampled);
 if(linear_samples<100000 || raw_samples<100000) $fatal(1,"both formats were not sampled");
 if(SIDE_BORDER!=0 && border_sampled<100000) $fatal(1,"too few border pixels sampled: %0d",border_sampled);
 if(de_runs<100) $fatal(1,"too few complete active-video lines: %0d",de_runs);
 $display("DIGITAL_DESIGN_PASS"); $finish;
end
// Pixel-accuracy monitor: derive the expected RGB from absolute source x/y,
// vertical/horizontal scale and the row's snapshotted format. No DUT pixel is
// used as a golden anchor, so a shifted row or stale pair cannot pass by merely
// preserving the relative progression. Sync and DE must match the RGB stage.
always @(posedge pixel_clock) begin
 if(!dut.pixel_reset && dut.started) begin
  if(dut.hsync_pipe3!==hsync_d || dut.vsync_pipe3!==vsync_d)
   $fatal(1,"sync/RGB pipeline delay mismatch");
 end
 hsync_d<=dut.hsync; vsync_d<=dut.vsync;
 if(dut.h_count==H_ACTIVE_START+SIDE_BORDER) begin
  pixel_y=(dut.v_count-V_ACTIVE_START)/SCALE;
  monitor_linear=dut.scan_linear;
 end
 vis_d <= vis;
 fb_vis_d <= fb_vis;
 if (vis && !vis_d) de_col=0;
 if (vis) de_col=de_col+1;
 if (!vis && vis_d) begin
  if (de_col!=(H_ACTIVE_END-H_ACTIVE_START))
   $fatal(1,"active-video width was %0d instead of %0d",de_col,H_ACTIVE_END-H_ACTIVE_START);
  de_runs=de_runs+1;
 end
 if (fb_vis && !fb_vis_d) col=0;
 if (fb_vis) begin
  sampled=sampled+1;
  expected_pixel=pattern(col/SCALE,pixel_y);
  expected_rgb=color(expected_pixel,monitor_linear);
  if(monitor_linear) linear_samples=linear_samples+1; else raw_samples=raw_samples+1;
  if (dut.rgb_pipe !== expected_rgb) begin
   if (bad<10) $display("pixel mismatch x=%0d y=%0d linear=%0d: got %h want %h", col, pixel_y,monitor_linear,dut.rgb_pipe,expected_rgb);
   bad=bad+1;
  end
  col=col+1;
 end
 if (vis && !fb_vis) begin
  border_sampled=border_sampled+1;
  if (!dut.scan_valid) begin
   if(dut.rgb_pipe!==0) $fatal(1,"cold visible video was not black");
  end else if (dut.rgb_pipe!==color(16'h1082,0)) $fatal(1,"border pixel mismatch");
 end
end

wire [9:0] reference_blue,reference_green,reference_red;
DisplayTmdsReference ref_b(.clk(pixel_clock),.reset(dut.pixel_reset),.de(dut.visible_pipe3),
 .control({dut.vsync_pipe3,dut.hsync_pipe3}),.data(dut.rgb_pipe[7:0]),.symbol(reference_blue));
DisplayTmdsReference ref_g(.clk(pixel_clock),.reset(dut.pixel_reset),.de(dut.visible_pipe3),
 .control(2'b00),.data(dut.rgb_pipe[15:8]),.symbol(reference_green));
DisplayTmdsReference ref_r(.clk(pixel_clock),.reset(dut.pixel_reset),.de(dut.visible_pipe3),
 .control(2'b00),.data(dut.rgb_pipe[23:16]),.symbol(reference_red));
always @(posedge pixel_clock) begin
 #1;
 if({dut.blue_symbol,dut.green_symbol,dut.red_symbol} !==
    {reference_blue,reference_green,reference_red}) $fatal(1,"TMDS symbol/disparity mismatch");
end
endmodule

module DisplayTmdsReference(input wire clk,input wire reset,input wire de,
 input wire [1:0] control,input wire [7:0] data,output reg [9:0] symbol);
integer i; reg [3:0] data_ones,qm_ones; reg [8:0] qm; reg signed [5:0] disparity=0;
wire signed [5:0] qm_delta=$signed({1'b0,qm_ones,1'b0})-6'sd8;
always @* begin
 data_ones=data[0]+data[1]+data[2]+data[3]+data[4]+data[5]+data[6]+data[7]; qm[0]=data[0];
 if(data_ones>4 || (data_ones==4 && data[0]==0)) begin
  for(i=1;i<8;i=i+1) qm[i]=~(qm[i-1]^data[i]); qm[8]=0;
 end else begin for(i=1;i<8;i=i+1) qm[i]=qm[i-1]^data[i]; qm[8]=1; end
 qm_ones=qm[0]+qm[1]+qm[2]+qm[3]+qm[4]+qm[5]+qm[6]+qm[7];
end
always @(posedge clk) begin
 if(reset) begin disparity<=0; symbol<=10'b1101010100; end
 else if(!de) begin disparity<=0; case(control)
  0:symbol<=10'b1101010100; 1:symbol<=10'b0010101011;
  2:symbol<=10'b0101010100; default:symbol<=10'b1010101011; endcase end
 else if(disparity==0 || qm_ones==4) begin
  symbol[9]<=~qm[8]; symbol[8]<=qm[8]; symbol[7:0]<=qm[8]?qm[7:0]:~qm[7:0];
  if(qm[8]) disparity<=disparity+qm_delta; else disparity<=disparity-qm_delta;
 // The unsized alternatives below are literal zero, so narrowing them to the
 // six-bit signed disparity datapath is exact. Sized zero changes Gowin's
 // signed-expression inference and costs substantial logic on GW2AR-18.
 end else if((disparity>0&&qm_ones>4)||(disparity<0&&qm_ones<4)) begin
  symbol<={1'b1,qm[8],~qm[7:0]}; disparity<=disparity-qm_delta+(qm[8]?6'sd2:0); // gowin-lint: allow EX3791
 end else begin symbol<={1'b0,qm[8],qm[7:0]}; disparity<=disparity+qm_delta-(qm[8]?0:6'sd2); end // gowin-lint: allow EX3791
end endmodule
