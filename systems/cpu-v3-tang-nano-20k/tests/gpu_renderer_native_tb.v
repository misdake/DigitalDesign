`timescale 1ns/1ps
// Real CPU/cache/GPU/Display + production arbiter, controlled 64-bit memory service.
// Accelerated pixel clock is a protocol stress profile, not board clock qualification.
module tb;
localparam FAULT=__FAULT__;
reg clk=0,reset=1,pixel_clock=0;real pixel_half_period=0.05;always #5 clk=~clk;always #(pixel_half_period) pixel_clock=~pixel_clock;
GSR GSR(.GSRI(1'b1));
integer cycle=0;reg[15:0]rom[0:65535];reg[15:0]shadow[0:49151];reg[63:0]mem[0:1048575];
wire ireq,iready;wire[31:0]ia;
wire dq,dw,dr,rv,rr,de;wire[31:0]da;wire[15:0]wd,rd;
wire lc,lc_ready,cl;wire[21:0]ls,ca;wire[7:0]lp;
wire[2:0]di;wire[3:0]ch;wire ren,wen;wire[15:0]wdata,rdata,sdata,gdata;
wire hold,clean,invalidate,ready,busy,done,error,sweep;
wire halted,fault;wire[15:0]signal,pc;wire[7:0]fault_code;
wire mq,mw,ml,mready,mwready,mrv,mrr,me;wire[21:0]ma;wire[63:0]mwd,mrd;
wire display_request,display_ready,display_valid,display_last,display_error,display_underflow,display_started,display_start_error;
wire[21:0]display_address;wire[63:0]display_payload;wire[15:0]display_data;
// Compress only the cold blank startup; scanout itself remains the 2x stress profile.
always @(posedge display_started) pixel_half_period=2.5;
integer held_response=0;wire pause=rv&&held_response<5;
__CORE__ cpu(.clk(clk),.reset(reset),.hold(hold||sweep||pause||!display_started),
 .instruction_request_ready(1'b1),.instruction_response_valid(ireq),.instruction_data(rom[ia[15:0]]),.instruction_error(1'b0),
 .instruction_request_valid(ireq),.instruction_address(ia),.instruction_response_ready(iready),
 .data_request_ready(dr),.data_response_valid(rv),.data_read_data(rd),.data_error(de),
 .data_line_copy_ready(lc_ready),.data_cache_command_error(error),.data_request_valid(dq),.data_write(dw),.data_address(da),.data_write_data(wd),.data_response_ready(rr),
 .data_line_copy_valid(lc),.data_line_copy_source(ls),.data_line_clean_valid(cl),.data_line_clean_address(ca),
 .device_index(di),.device_channel(ch),.device_read_enable(ren),.device_write_enable(wen),.device_write_data(wdata),.device_read_data(rdata),
 .halted(halted),.halt_signal(signal),.fault(fault),.fault_code(fault_code),.pc(pc));
assign rdata=di==0?sdata:di==4?gdata:di==3?display_data:16'd0;
__SYSCTL__ sysctl(.clk(clk),.reset(reset),.device_index(di),.device_channel(ch),.device_read_enable(ren),.device_write_enable(wen),.device_write_data(wdata),.device_read_data(sdata),
 .dcache_maintenance_ready(ready),.dcache_maintenance_done(done),.dcache_maintenance_error(error),.watch_read_data(16'd0),
 .cpu_hold(hold),.dcache_clean(clean),.dcache_invalidate(invalidate));
__CACHE__ cache(.clk(clk),.reset(reset),.clean_all(clean),.invalidate_all(invalidate),.line_copy_start(lc),.line_copy_source(ls),.line_clean_start(cl),.line_clean_address(ca),.line_copy_ready(lc_ready),
 .cpu_request_valid(dq),.cpu_write(dw),.cpu_address(da),.cpu_write_data(wd),.cpu_response_ready(rr),.cpu_request_ready(dr),.cpu_response_valid(rv),.cpu_read_data(rd),.cpu_error(de),
 .memory_request_ready(mready),.memory_write_data_ready(mwready),.memory_response_valid(mrv),.memory_read_data(mrd),.memory_error(me),.memory_request_valid(mq),.memory_write(mw),.memory_line(ml),.memory_address(ma),.memory_write_data(mwd),.memory_response_ready(mrr),
 .maintenance_ready(ready),.maintenance_busy(busy),.maintenance_done(done),.maintenance_error(error),.valid_sweep(sweep));

wire frr,frv,frl;wire[63:0]frd;
wire roq,ror,rov,rol,frq,fwq,fwr,fwready,fwv;
wire[21:0]roa,fra,fwa;wire[1:0]roc,frc,fwc;wire[63:0]rod,fwd;
wire ce=cycle%7!=2;
wire roe,fre,fwe,fwl;
__GPU__ gpu(.clk(clk),.reset(reset),
 .device_index(di),.device_channel(ch),.device_read_enable(ren),.device_write_enable(wen),.device_write_data(wdata),.device_read_data(gdata),
 .gpu_ro_request_valid(roq),.gpu_ro_request_ready(ror),.gpu_ro_write(),.gpu_ro_address(roa),.gpu_ro_line_count_minus_1(roc),.gpu_ro_write_data(),.gpu_ro_write_data_ready(1'b0),
 .gpu_ro_response_valid(rov),.gpu_ro_response_last(rol),.gpu_ro_error(roe),.gpu_ro_read_data(rod),
 .gpu_fb_r_request_valid(frq),.gpu_fb_r_request_ready(frr),.gpu_fb_r_write(),.gpu_fb_r_address(fra),.gpu_fb_r_line_count_minus_1(frc),.gpu_fb_r_write_data(),.gpu_fb_r_write_data_ready(1'b0),
 .gpu_fb_r_response_valid(frv),.gpu_fb_r_response_last(frl),.gpu_fb_r_error(fre),.gpu_fb_r_read_data(frd),
 .gpu_fb_w_request_valid(fwq),.gpu_fb_w_request_ready(fwr),.gpu_fb_w_write(),.gpu_fb_w_address(fwa),.gpu_fb_w_line_count_minus_1(fwc),.gpu_fb_w_write_data(fwd),.gpu_fb_w_write_data_ready(fwready),
 .gpu_fb_w_response_valid(fwv),.gpu_fb_w_response_last(fwl),.gpu_fb_w_error(fwe));
wire[2:0]slot_valid,slot_ready,slot_write;wire[21:0]slot_address[0:2];wire[1:0]slot_count[0:2];
CpuV3GpuMemoryRequestSlot slot_ro(.clk(clk),.reset(reset),.in_valid(roq),.in_ready(ror),.in_write(1'b0),.in_address(roa),.in_count(roc),.out_valid(slot_valid[0]),.out_ready(slot_ready[0]),.out_write(slot_write[0]),.out_address(slot_address[0]),.out_count(slot_count[0]));
CpuV3GpuMemoryRequestSlot slot_fr(.clk(clk),.reset(reset),.in_valid(frq),.in_ready(frr),.in_write(1'b0),.in_address(fra),.in_count(frc),.out_valid(slot_valid[1]),.out_ready(slot_ready[1]),.out_write(slot_write[1]),.out_address(slot_address[1]),.out_count(slot_count[1]));
CpuV3GpuMemoryRequestSlot slot_fw(.clk(clk),.reset(reset),.in_valid(fwq),.in_ready(fwr),.in_write(1'b1),.in_address(fwa),.in_count(fwc),.out_valid(slot_valid[2]),.out_ready(slot_ready[2]),.out_write(slot_write[2]),.out_address(slot_address[2]),.out_count(slot_count[2]));
// Test-only clock-enable stress at the native core; production wrapper stays exact.
initial force gpu.core.ce=ce;

__DISPLAY__ display(.clk(clk),.reset(reset),.pixel_clock(pixel_clock),.serial_clock(1'b0),.video_locked(1'b1),.boot_enable(!reset),.boot_failed(1'b0),
 .memory_request_ready(display_ready),.memory_data_valid(display_valid),.memory_read_data(display_payload),.memory_last(display_last),.memory_error(display_error),
 .device_index(di),.device_channel(ch),.device_read_enable(ren),.device_write_enable(wen),.device_write_data(wdata),.device_read_data(display_data),
 .memory_request_valid(display_request),.memory_address(display_address),.underflow(display_underflow),.started_ack(display_started),.startup_error(display_start_error));
wire aq,aw,al,ar,awr,av,arr,ae,alast;wire[21:0]aa;wire[1:0]ac;wire[63:0]awd,ard;
__ARBITER__ arbiter(.clk(clk),.reset(reset),.lookahead_enable(1'b0),
 .instruction_request_valid(1'b0),.instruction_address(22'd0),.instruction_response_ready(1'b1),
 .dma_request_valid(1'b0),.dma_write(1'b0),.dma_address(22'd0),.dma_write_data(16'd0),.dma_response_ready(1'b1),
 .data_request_valid(mq),.data_write(mw),.data_line(ml),.data_address(ma),.data_write_data(mwd),.data_response_ready(mrr),
 .data_request_ready(mready),.data_write_data_ready(mwready),.data_response_valid(mrv),.data_read_data(mrd),.data_error(me),
 .display_request_valid(display_request),.display_address(display_address),.display_response_ready(1'b1),
 .display_request_ready(display_ready),.display_response_valid(display_valid),.display_read_data(display_payload),.display_response_last(display_last),.display_error(display_error),
 .gpu_ro_request_valid(slot_valid[0]),.gpu_ro_write(slot_write[0]),.gpu_ro_address(slot_address[0]),.gpu_ro_line_count_minus_1(slot_count[0]),.gpu_ro_write_data(64'd0),
 .gpu_ro_request_ready(slot_ready[0]),.gpu_ro_response_valid(rov),.gpu_ro_read_data(rod),.gpu_ro_response_last(rol),.gpu_ro_error(roe),
 .gpu_fb_r_request_valid(slot_valid[1]),.gpu_fb_r_write(slot_write[1]),.gpu_fb_r_address(slot_address[1]),.gpu_fb_r_line_count_minus_1(slot_count[1]),.gpu_fb_r_write_data(64'd0),
 .gpu_fb_r_request_ready(slot_ready[1]),.gpu_fb_r_response_valid(frv),.gpu_fb_r_read_data(frd),.gpu_fb_r_response_last(frl),.gpu_fb_r_error(fre),
 .gpu_fb_w_request_valid(slot_valid[2]),.gpu_fb_w_write(slot_write[2]),.gpu_fb_w_address(slot_address[2]),.gpu_fb_w_line_count_minus_1(slot_count[2]),.gpu_fb_w_write_data(fwd),
 .gpu_fb_w_request_ready(slot_ready[2]),.gpu_fb_w_write_data_ready(fwready),.gpu_fb_w_response_valid(fwv),.gpu_fb_w_response_last(fwl),.gpu_fb_w_error(fwe),
 .memory_request_ready(ar),.memory_write_data_ready(awr),.memory_response_valid(av),.memory_read_data(ard),.memory_response_last(alast),.memory_error(ae),
 .memory_request_valid(aq),.memory_write(aw),.memory_line(al),.memory_address(aa),.memory_line_count_minus_1(ac),.memory_write_data(awd),.memory_response_ready(arr));
// Ordered external memory. A write commits only at a successful terminal.
integer ms=0,mb=0,mi=0,mn=0,md=0;reg mwrite=0;reg[63:0]write_buffer[0:15];
assign ar=ms==0&&cycle%7!=3&&!halted&&!fault;assign awr=ms==3&&cycle%5!=1;
assign av=ms==2||ms==5;assign ard=mem[mb+mi];assign alast=ms==5||mi==mn-1;assign ae=1'b0;
integer k,idx,starts=0,terminals=0,swaps=0,presents=0,prepare=0,overlap=0,held=0,ce0=0,blocked=0,fst=0,maintenance=0;
reg was_busy=0;reg[21:0]last_active='h200000;integer display_outstanding=0,display_base=0;
integer ui_starts=0,ui_done=0,ui_release_pending=0,display_reads=0,display_terminals=0,pending_cycles=0;
integer ro_live=0,fr_live=0,fw_live=0;
integer ncmd=0,nobj=0,last_count=0,active_cb=0,active_obj=0,active_page=0;
reg[15:0]snap_cmd[0:32767],snap_obj[0:32767],snap_ui[0:399];
localparam SLOTA=__SLOTA__,SLOTB=__SLOTB__;
localparam PAGES=__PAGES__,PIPE=__PIPE__,TARGETS=__TARGETS__,RENDERER=__RENDERER__,OLD=__OLD__,DIRTY=__DIRTY__;
function[15:0]word_at;input integer word;begin word_at=mem[word>>2]>>((word%4)*16);end endfunction
function protected_color;input[21:0]address;begin
 protected_color=(address>=display.active_base&&address<display.active_base+102400)||
 (display_outstanding&&address>=display_base&&address<display_base+102400);
end endfunction
function[15:0]pixel_at;input integer base,x,y;integer off;begin
 off=((x/16)*16+y/16)*512+(y%16)*32+(x%16)*2;
 pixel_at=word_at((base+off)/2);
end endfunction
always @(posedge clk)begin
 cycle<=cycle+1;
 if(cycle%20000==0)begin $display("NATIVE_PROGRESS cycle=%0d pc=%h gpu=%0d starts=%0d done=%0d swaps=%0d",cycle,pc,gpu.core.busy,starts,terminals,swaps);$fflush();end
 if(cycle>4000000)$fatal(1,"native watchdog cpu=%0d pc=%h gpu=%0d",cpu.state,pc,gpu.core.processor.state);
 if(!reset)begin
  if(display_underflow||display_start_error)$fatal(1,"Display health failed cycle=%0d",cycle);
  if(!ce)ce0<=ce0+1;if(fwq&&!fwr)blocked<=blocked+1;
  case({roq&&ror,rov&&(rol||roe)})2'b10:ro_live<=ro_live+1;2'b01:ro_live<=ro_live-1;endcase
  case({frq&&frr,frv&&(frl||fre)})2'b10:fr_live<=fr_live+1;2'b01:fr_live<=fr_live-1;endcase
  case({fwq&&fwr,fwv&&(fwl||fwe)})2'b10:fw_live<=fw_live+1;2'b01:fw_live<=fw_live-1;endcase
  if(cpu.fpu2_pending!=0)fst<=fst+1;
  if(prepare&&gpu.core.busy)overlap<=overlap+1;
  if(pause)held_response<=held_response+1;
  if(dq&&dr&&dw&&da<49152)begin
   shadow[da]<=wd;
   if((da==PAGES||da==PAGES+4)&&wd==0&&shadow[da]>=2)begin
    if(gpu.core.busy||terminals==0)$fatal(1,"UI freed before real GPU terminal");
    if(display.next_pending)ui_release_pending<=ui_release_pending+1;
    $display("NATIVE_UI_FREE cycle=%0d slot=%0d pending=%0d",cycle,(da-PAGES)/4,display.next_pending);
   end
  end
  if(display.next_pending)pending_cycles<=pending_cycles+1;
  if(gpu.core.ui_start)ui_starts<=ui_starts+1;
  if(ce&&gpu.core.ui_done&&gpu.core.processor.state==18)ui_done<=ui_done+1;
  if(dq&&dr&&dw&&da>=22'h200000)begin
   // References live in CPU memory; observe accepted scalar/FST stores, not stale DRAM metadata.
   for(k=0;k<2;k=k+1)begin
    if((da>=(k==0?'h4c0000:'h7e0000)/2&&da<((k==0?'h4c0000:'h7e0000)+65536)/2)||
       (da>=(k==0?'h4d0000:'h7f0000)/2&&da<((k==0?'h4d0000:'h7f0000)+65536)/2))begin
     if(shadow[PAGES+4*k]>=2)begin
      if(shadow[PAGES+4*k]!=2||shadow[PIPE+2]!=k+1||gpu.core.busy||display.next_pending)$fatal(1,"held input store");
      if(da<(k==0?'h4d0000:'h7f0000)/2+shadow[TARGETS+k]-1||da>=(k==0?'h4d0000:'h7f0000)/2+shadow[TARGETS+k]-1+16)$fatal(1,"non-Target Recorded store");
     end
    end
    if(da>='h7df800/2+k*512&&da<'h7df800/2+(k+1)*512&&shadow[PAGES+4*k]!=1)$fatal(1,"UI modified while referenced/free");
   end
  end
  if(display_request&&display_ready)begin
   if(display_outstanding)$fatal(1,"two Display reads");display_outstanding<=1;display_base<=display.active_base;display_reads<=display_reads+1;
  end
  if(display_valid&&(display_last||display_error))begin display_outstanding<=0;display_terminals<=display_terminals+1;end
  if(slot_valid[2]&&slot_ready[2])begin
   if(protected_color(slot_address[2]))$fatal(1,"GPU write to Front or old read");
   if(!((slot_address[2]>='h200000&&slot_address[2]<'h200000+102400)||(slot_address[2]>='h220000&&slot_address[2]<'h220000+102400)||(slot_address[2]>='h240000&&slot_address[2]<'h240000+102400)))$fatal(1,"GPU output outside planes");
  end
  if(wen&&di==7&&ch==8)begin prepare<=1;$display("NATIVE_RECORD tick=%0d cycle=%0d gpu=%0d pending=%0d",wdata,cycle,gpu.core.busy,display.next_pending);end
  if(wen&&di==7&&ch==9)prepare<=0;
  if(wen&&di==0&&(ch==1||ch==4))begin
   if(cpu.async_store_valid||cpu.fpu2_pending!=0||cpu.fpu2_busy)$fatal(1,"fence overtook CPU stores");maintenance<=maintenance+1;
  end
  if(wen&&di==4&&ch==0)last_count<=wdata;
  if(wen&&di==4&&ch==1)begin
   if(gpu.core.busy||display.next_pending)$fatal(1,"submitted before GPU/Display retirement");
   if(wdata!=(starts%2==0?'h9800:'hfc00))$fatal(1,"not A/B/A inputs");
   for(idx=0;idx<4;idx=idx+1)begin
    if(word_at(OLD+idx*2)!==0||word_at(OLD+idx*2+1)!==starts+idx+1)$fatal(1,"old FST not published");
   end
   if(word_at(DIRTY)!==16'h5101+starts||word_at(DIRTY+1)!==16'h5201+starts)$fatal(1,"dirty scalar not published");
   active_cb=wdata*64;active_obj=(starts%2==0?'h4d0000:'h7f0000)/2;active_page='h7df800/2+(starts%2)*512;
   ncmd=shadow[(starts%2==0?SLOTA:SLOTB)+7];nobj=shadow[(starts%2==0?SLOTA:SLOTB)+8];
   if(ncmd!=(last_count+1)*2||ncmd>32768||nobj>32768||ncmd<2)$fatal(1,"native count/capacity mismatch");
   if(word_at(active_cb+ncmd-2)!==0||word_at(active_cb+ncmd-1)!==16'hf000)$fatal(1,"END not last valid command");
   for(idx=ncmd;idx<((ncmd+63)&~63);idx=idx+1)if(word_at(active_cb+idx)!==0)$fatal(1,"nonzero fetch padding");
   for(idx=0;idx<((ncmd+63)&~63);idx=idx+1)snap_cmd[idx]=word_at(active_cb+idx);
   for(idx=0;idx<nobj;idx=idx+1)snap_obj[idx]=word_at(active_obj+idx);
   for(idx=0;idx<400;idx=idx+1)snap_ui[idx]=word_at(active_page+idx);
   if(snap_ui[0]!==16'hff31+starts)$fatal(1,"stale/wrong UI page");
   for(idx=375;idx<384;idx=idx+1)if(snap_ui[idx]!==16'hffff)$fatal(1,"UI fetch padding");
   starts<=starts+1;$display("NATIVE_SEND seq=%0d cycle=%0d base=%h",starts,cycle,wdata);
  end
  if(was_busy&&!gpu.core.busy)begin
   if(ro_live!=0||fr_live!=0||fw_live!=0||slot_valid!=0)$fatal(1,"GPU terminal preceded actual transport retirement");
   for(idx=0;idx<((ncmd+63)&~63);idx=idx+1)if(word_at(active_cb+idx)!==snap_cmd[idx])$fatal(1,"inflight command mutated");
   for(idx=0;idx<nobj;idx=idx+1)if(word_at(active_obj+idx)!==snap_obj[idx])$fatal(1,"inflight object mutated");
   for(idx=0;idx<400;idx=idx+1)if(word_at(active_page+idx)!==snap_ui[idx])$fatal(1,"inflight UI mutated");
   terminals<=terminals+1;
   if(pixel_at(terminals%2==0?'h440000:'h400000,3,2)!==16'hffff)$fatal(1,"real UI missing");
   if(pixel_at(terminals%2==0?'h440000:'h400000,2,2)!==(terminals==0?16'h1112:16'hffff))$fatal(1,"wrong UI glyph generation");
   if(pixel_at('h480000,200,120)==16'hffff||pixel_at('h480000,399,239)!==16'hffff)$fatal(1,"real depth coverage missing");
   if(pixel_at(terminals%2==0?'h440000:'h400000,200,120)!==16'hf800)$fatal(1,"real triangle missing");
   if(pixel_at(terminals%2==0?'h440000:'h400000,399,239)!==16'h1112+terminals)$fatal(1,"real clear missing");
   $display("NATIVE_GPU_DONE seq=%0d cycle=%0d",terminals,cycle);
  end
  was_busy<=gpu.core.busy;
  if(wen&&di==3&&ch==3)begin if(display.next_pending)$fatal(1,"overwrote Pending");presents<=presents+1;end
  if(aq&&ar)begin mb<=aa>>2;mi<=0;mn<=(ac+1)*4;mwrite<=aw;ms<=1;md<=1;end
  case(ms)
   1:if(md!=0)md<=md-1;else ms<=mwrite?3:2;
   2:if(arr)begin if(mi==mn-1)begin mi<=0;ms<=0;end else mi<=mi+1;end
   3:if(awr)begin write_buffer[mi]<=awd;if(mi==mn-1)begin mi<=0;md<=5;ms<=4;end else mi<=mi+1;end
   4:if(md!=0)md<=md-1;else ms<=5;
   5:if(arr)begin for(k=0;k<16;k=k+1)if(k<mn)mem[mb+k]<=write_buffer[k];ms<=0;end
  endcase
 end
end
always @(negedge clk)if(!reset&&display.active_base!=last_active)begin
 if(display_outstanding&&display_base==last_active)$fatal(1,"Display switched before old read terminal");
 last_active<=display.active_base;swaps<=swaps+1;$display("NATIVE_SWAP cycle=%0d active=%h",cycle,display.active_base);
end
integer pixel_x=0,pixel_y=0,scan_red=0,scan_white=0;reg visible_before=0;
always @(posedge pixel_clock)begin
 if(display.h_count==216)pixel_y=(display.v_count-32)/2;
 if(display.framebuffer_pipe3&&display.visible_pipe3&&!visible_before)pixel_x=0;
 visible_before<=display.framebuffer_pipe3&&display.visible_pipe3;
 if(display.framebuffer_pipe3&&display.visible_pipe3&&swaps>0)begin
  if(pixel_y==120&&pixel_x==400)begin if(display.rgb_pipe!==24'hff0000)$fatal(1,"real scan red mismatch");scan_red<=scan_red+1;end
  if(pixel_y==2&&pixel_x==6)begin if(display.rgb_pipe!==24'hffffff)$fatal(1,"real scan UI mismatch");scan_white<=scan_white+1;end
  pixel_x=pixel_x+1;
 end
end
initial begin
 for(idx=0;idx<65536;idx=idx+1)rom[idx]=16'h6c00;
 for(idx=0;idx<1048576;idx=idx+1)mem[idx]=64'h55aa55aa55aa55aa;
 for(idx=0;idx<49152;idx=idx+1)begin shadow[idx]=0;mem[idx>>2]=0;end
 $readmemh("__ASSETS__",mem);$readmemh("__PROGRAM__",rom);
 repeat(3)@(posedge clk);@(negedge clk);reset=0;
 wait(halted||fault);#1;
 $display("NATIVE_HALT cycle=%0d signal=%h starts=%0d done=%0d swaps=%0d overlap=%0d",cycle,signal,starts,terminals,swaps,overlap);
 if(fault||signal!==16'h600d)$fatal(1,"CPU failure pc=%h code=%0d signal=%h",pc,fault_code,signal);
 if(starts!=3||terminals!=3||swaps!=3||presents!=3||overlap==0||ce0==0||blocked==0||fst==0||maintenance<3)$fatal(1,"native coverage missing");
 if(ui_starts!=3||ui_done!=3||ui_release_pending==0||pending_cycles==0||scan_red==0||scan_white==0||display_reads==0)$fatal(1,"UI/Display lifetime coverage missing starts=%0d done=%0d freePending=%0d scan=%0d/%0d reads=%0d",ui_starts,ui_done,ui_release_pending,scan_red,scan_white,display_reads);
 if(shadow[PAGES]!=0||shadow[PAGES+4]!=0||shadow[PIPE+2]!=0||shadow[PIPE+3]!=0)$fatal(1,"drain left input refs");
 $display("NATIVE_LIFETIME UI=%0d/%0d freePending=%0d pendingCycles=%0d scans=%0d/%0d displayReads=%0d terminals=%0d",ui_starts,ui_done,ui_release_pending,pending_cycles,scan_red,scan_white,display_reads,display_terminals);
 $display("GPU_RENDERER_NATIVE_PASS cycles=%0d overlap=%0d CE0=%0d blocked=%0d FST=%0d",cycle,overlap,ce0,blocked,fst);$finish;
end
endmodule
