`timescale 1ns/1ps
module tb;
`ifndef PORTABLE
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0;always #5 clk=~clk;
reg reset=1,ce=0,start=0,geo_we=0,bbox_done=0,bbox_empty=0;
reg[3:0]geo_wa=0;reg[29:0]geo_wd=0;
reg[4:0]bbox_xlo=0,bbox_xhi=24;reg[3:0]bbox_ylo=0,bbox_yhi=14;
reg[1:0]phase=0;reg quad_ready=0;
wire quad_valid,quad_take,sample_valid,quad_end_valid,busy,done;
wire[7:0]quad_x;wire[6:0]quad_y;wire[1:0]sample_lane;wire[29:0]sample_h;
wire sample_cover;wire[3:0]quad_mask,error;
rc_raster dut(.clk(clk),.reset(reset),.ce(ce),.phase(phase),.start(start),
 .geo_we(geo_we),.geo_wa(geo_wa),.geo_wd(geo_wd),.bbox_done(bbox_done),.bbox_empty(bbox_empty),
 .bbox_xlo(bbox_xlo),.bbox_xhi(bbox_xhi),.bbox_ylo(bbox_ylo),.bbox_yhi(bbox_yhi),
 .run_enable(1'b1),.quad_ready(quad_ready),.quad_valid(quad_valid),.quad_take(quad_take),
 .quad_x(quad_x),.quad_y(quad_y),.sample_valid(sample_valid),.sample_lane(sample_lane),.sample_h(sample_h),
 .sample_cover(sample_cover),.quad_end_valid(quad_end_valid),.quad_mask(quad_mask),.error(error),.busy(busy),.done(done));
reg signed[31:0]coefficients[0:@N@*9-1];reg[3:0]masks[0:@N@*24000-1];
reg[18:0]bounds[0:@N@-1];reg[19:0]counts[0:@N@-1];reg seen[0:23999];
reg[17:0]origins[0:@N@-1];integer origin_returns=0,origin_xy=0,origin_edge_index;
integer wall=0,en=0,readywait=0,ce0=0,p=0,j=0,x=0,y=0,q=0,live=0,totalq=0,empty=0,samples=0;
integer prefetched=0,fallback=0,prefetch_full=0,prefetch_partial=0;
integer predicted=0,known_skip=0;
integer rejected_tail_bypasses=0;
reg pixel_live=0;reg[1:0]lane;
reg signed[63:0]want_h;
always @(negedge clk)begin
 ce=!reset&&(wall%13!=3&&wall%13!=4);
 quad_ready=wall%23<17;
end
always @(posedge clk)begin
 wall=wall+1;if(wall>5000000)$fatal(1,"bounded raster scan watchdog");
 if(reset)phase<=0;
 else if(ce)begin
  en=en+1;phase<=phase+1'b1;
  if(error)$fatal(1,"raster error %h packet %0d",error,p);
`ifdef RASTER_LOCALITY_LIMIT
  if(dut.start_tile&&dut.head_meta[22]&&dut.head_meta[21:18]>4)$fatal(1,"full tile exceeds locality unit");
`endif
`ifdef RASTER_REJECT_TAIL
  if(dut.reject_tail)begin
   if(phase!=3||dut.scan_full||sample_valid||quad_take||dut.scan_fetch0)$fatal(1,"invalid rejected-tail bypass");
   if(dut.scan_classify_pending||(dut.check_finish&&!dut.scan_end))rejected_tail_bypasses=rejected_tail_bypasses+1;
  end
`endif
`ifdef RASTER_ORIGIN_DSP
  if(dut.origin_retire)begin
   origin_edge_index=dut.origin_edge1;
   want_h=$signed(coefficients[p*9+origin_edge_index*3+2]);
   want_h=want_h+$signed(coefficients[p*9+origin_edge_index*3])*$signed({1'b0,origins[p][8:0]});
   want_h=want_h+$signed(coefficients[p*9+origin_edge_index*3+1])*$signed({1'b0,origins[p][17:9]});
   if(dut.origin_result[29:0]!==want_h[29:0])$fatal(1,"DSP root h packet%0d edge%0d",p,origin_edge_index);
   origin_returns=origin_returns+1;
   if(origins[p][8:0]!=0&&origins[p][17:9]!=0)origin_xy=origin_xy+1;
  end
`endif
`ifdef RASTER_NEXT_PREDICATE
  if(dut.check_start)begin
   if(dut.check_accept)$fatal(1,"CHECK must only advance a known rejected quad");
   known_skip=known_skip+1;
  end
  if(quad_take&&!dut.scan_full)begin
   if(!dut.check_accept)$fatal(1,"partial quad admitted without a stored predicate");
   predicted=predicted+1;
  end
`endif
`ifdef RASTER_PREFETCH_COVERAGE
  if(dut.prefetch_issue&&(dut.tile_empty||!dut.scan_end))$fatal(1,"prefetch ownership");
  if(dut.s_prefetch_retire&&phase!=3)$fatal(1,"prefetch write calendar");
  if(dut.prefetch_handoff)begin
   prefetched=prefetched+1;
   if(dut.head_meta[22])prefetch_full=prefetch_full+1;else prefetch_partial=prefetch_partial+1;
  end
  if(dut.start_tile&&!dut.prefetch_handoff)fallback=fallback+1;
`endif
  if(quad_valid&&!quad_ready)readywait=readywait+1;
  if(quad_take)begin x=quad_x*2;y=quad_y*2;q=quad_y*200+quad_x;totalq=totalq+1;end
  if(sample_valid)begin pixel_live=1;lane=sample_lane;end
  if(pixel_live&&phase<3)begin
   want_h=coefficients[p*9+phase*3+2];
   want_h=want_h+$signed(coefficients[p*9+phase*3])*(x+(lane&1));
   want_h=want_h+$signed(coefficients[p*9+phase*3+1])*(y+(lane>>1));
   if(sample_h!==want_h[29:0])$fatal(1,"pixel h packet%0d xy%0d,%0d lane%0d edge%0d got%h want%h",p,x,y,lane,phase,sample_h,want_h[29:0]);
   if(phase==2)begin pixel_live=0;samples=samples+1;end
  end
  if(quad_end_valid)begin
   if(quad_mask!==masks[p*24000+q])$fatal(1,"quad mask packet%0d q%0d got%h want%h",p,q,quad_mask,masks[p*24000+q]);
   if(seen[q])$fatal(1,"quad visited twice");seen[q]=1;
   if(quad_mask!=0)live=live+1;else empty=empty+1;
  end
 end else if(!reset)ce0=ce0+1;
end
task tick_clock;begin @(posedge clk);#1;end endtask
task enabled;begin tick_clock();while(!ce)tick_clock();end endtask
initial begin
 $readmemh("coefficients.hex",coefficients);$readmemh("masks.hex",masks);$readmemh("bounds.hex",bounds);$readmemh("counts.hex",counts);
 $readmemh("origins.hex",origins);
 repeat(24)tick_clock();@(negedge clk);reset=0;
 for(p=0;p<@N@;p=p+1)begin
  live=0;for(j=0;j<24000;j=j+1)seen[j]=0;
  @(negedge clk);start=1;enabled();@(negedge clk);start=0;
  for(j=0;j<9;j=j+1)begin
   @(negedge clk);geo_we=1;geo_wa=(j%3)*4+j/3;geo_wd=coefficients[p*9+j];
   enabled();@(negedge clk);geo_we=0;
  end
  @(negedge clk);bbox_xlo=bounds[p][4:0];bbox_xhi=bounds[p][9:5];bbox_ylo=bounds[p][13:10];bbox_yhi=bounds[p][17:14];bbox_empty=bounds[p][18];bbox_done=1;
  enabled();@(negedge clk);bbox_done=0;
  while(busy)tick_clock();
  if(live!=counts[p])$fatal(1,"missing covered quads packet%0d got%0d want%0d",p,live,counts[p]);
 end
 if(samples!=4*totalq||readywait==0||ce0==0)$fatal(1,"missing scan/stall coverage");
`ifdef RASTER_REJECT_TAIL
 if(rejected_tail_bypasses==0)$fatal(1,"missing rejected-tail bypass coverage");
 $display("TAIL bypasses=%0d",rejected_tail_bypasses);
`endif
`ifdef RASTER_ORIGIN_TRANSLATED
 if(origin_returns!=3*@N@||origin_xy!=origin_returns)$fatal(1,"missing two-axis DSP origin coverage");
 $display("ORIGIN returns=%0d both_axes=%0d",origin_returns,origin_xy);
`endif
`ifdef RASTER_NEXT_PREDICATE
 if(predicted==0||known_skip==0)$fatal(1,"missing predicted/known-rejected quad coverage");
 $display("PREDICATE predicted=%0d known_skip=%0d",predicted,known_skip);
`endif
`ifdef RASTER_PREFETCH_COVERAGE
 if(prefetched==0||fallback==0)$fatal(1,"missing tile handoff coverage");
 $display("HANDOFF prefetched=%0d fallback=%0d full=%0d partial=%0d",prefetched,fallback,prefetch_full,prefetch_partial);
`endif
 $display("PASS SCAN packets=%0d cycles=%0d enabled=%0d quads=%0d empty=%0d samples=%0d backpressure=%0d ce0=%0d",p,wall,en,totalq,empty,samples,readywait,ce0);$finish;
end
endmodule
