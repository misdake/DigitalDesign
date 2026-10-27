`timescale 1ns/1ps
// Real CPU/cache/GPU/Display + production arbiter, controlled 64-bit memory service.
// Logic54MHz / pixel33.3MHz; controlled memory peer and direct instruction ROM.
module tb;
reg[15:0]depth1[0:95999],depth2[0:95999],matrix1[0:41],matrix2[0:41];reg[63:0]atlas_initial[0:8191];integer uis=0,ui_terminals=0,reloaded_after_ui=0;
localparam FAULT=0;
reg clk=0,reset=1,pixel_clock=0;real pixel_half_period=0.05;always #9.259259 clk=~clk;always #(pixel_half_period) pixel_clock=~pixel_clock;
GSR GSR(.GSRI(1'b1));
integer cycle=0;reg[15:0]rom[0:65535];reg[15:0]gold1[0:95999],gold2[0:95999];reg[63:0]asset_initial[0:16383];reg[63:0]mem[0:1048575];
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
always @(posedge display_started) pixel_half_period=15.015015;
integer held_response=0;wire pause=rv&&held_response<5;
__CORE__ cpu(.clk(clk),.reset(reset),.hold(hold||sweep||pause||!display_started),
 .instruction_request_ready(1'b1),.instruction_response_valid(ireq),.instruction_data(rom[ia[15:0]]),.instruction_error(1'b0),
 .instruction_request_valid(ireq),.instruction_address(ia),.instruction_response_ready(iready),
 .data_request_ready(dr),.data_response_valid(rv),.data_read_data(rd),.data_error(de),
 .data_line_copy_ready(lc_ready),.data_cache_command_error(error),.data_request_valid(dq),.data_write(dw),.data_address(da),.data_write_data(wd),.data_response_ready(rr),
 .data_line_copy_valid(lc),.data_line_copy_source(ls),.data_line_copy_destination_page(lp),.data_line_clean_valid(cl),.data_line_clean_address(ca),
 .device_index(di),.device_channel(ch),.device_read_enable(ren),.device_write_enable(wen),.device_write_data(wdata),.device_read_data(rdata),
 .halted(halted),.halt_signal(signal),.fault(fault),.fault_code(fault_code),.pc(pc));
assign rdata=di==0?(ch==8?16'd0:sdata):di==4?gdata:di==3?display_data:16'd0;
__SYSCTL__ sysctl(.clk(clk),.reset(reset),.device_index(di),.device_channel(ch),.device_read_enable(ren),.device_write_enable(wen),.device_write_data(wdata),.device_read_data(sdata),
 .dcache_maintenance_ready(ready),.dcache_maintenance_done(done),.dcache_maintenance_error(error),.watch_read_data(16'd0),
 .cpu_hold(hold),.dcache_clean(clean),.dcache_invalidate(invalidate));
__CACHE__ cache(.clk(clk),.reset(reset),.clean_all(clean),.invalidate_all(invalidate),.line_copy_start(lc),.line_copy_source(ls),.line_copy_destination_page(lp),.line_clean_start(cl),.line_clean_address(ca),.line_copy_ready(lc_ready),
 .cpu_request_valid(dq),.cpu_write(dw),.cpu_address(da),.cpu_write_data(wd),.cpu_response_ready(rr),.cpu_request_ready(dr),.cpu_response_valid(rv),.cpu_read_data(rd),.cpu_error(de),
 .memory_request_ready(mready),.memory_write_data_ready(mwready),.memory_response_valid(mrv),.memory_read_data(mrd),.memory_error(me),.memory_request_valid(mq),.memory_write(mw),.memory_line(ml),.memory_address(ma),.memory_write_data(mwd),.memory_response_ready(mrr),
 .maintenance_ready(ready),.maintenance_busy(busy),.maintenance_done(done),.maintenance_error(error),.valid_sweep(sweep));

wire frr,frv,frl;wire[63:0]frd;
wire roq,ror,rov,rol,frq,fwq,fwr,fwready,fwv;
wire[21:0]roa,fra,fwa;wire[1:0]roc,frc,fwc;wire[63:0]rod,fwd;
wire ce=1'b1;
gpu_v2_vliw_command gpu(.clk(clk),.reset(reset),.ce(ce),.abort(1'b0),.output_ready(1'b1),
 .device_index(di),.device_channel(ch),.device_read_enable(ren),.device_write_enable(wen),.device_write_data(wdata),.device_read_data(gdata),
 .ro_request_valid(roq),.ro_request_ready(ror),.ro_address(roa),.ro_count(roc),.ro_response_valid(rov),.ro_response_last(rol),.ro_response_error(1'b0),.ro_data(rod),
 .fb_r_request_valid(frq),.fb_r_request_ready(frr),.fb_r_address(fra),.fb_r_count(frc),.fb_r_response_valid(frv),.fb_r_response_last(frl),.fb_r_response_error(1'b0),.fb_r_data(frd),
 .fb_w_request_valid(fwq),.fb_w_request_ready(fwr),.fb_w_address(fwa),.fb_w_count(fwc),.fb_w_data(fwd),.fb_w_data_ready(fwready),.fb_w_response_valid(fwv),.fb_w_response_last(1'b1),.fb_w_response_error(1'b0));

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
 .gpu_ro_request_valid(roq),.gpu_ro_write(1'b0),.gpu_ro_address(roa),.gpu_ro_line_count_minus_1(roc),.gpu_ro_write_data(64'd0),
 .gpu_ro_request_ready(ror),.gpu_ro_response_valid(rov),.gpu_ro_read_data(rod),.gpu_ro_response_last(rol),
 .gpu_fb_r_request_valid(frq),.gpu_fb_r_write(1'b0),.gpu_fb_r_address(fra),.gpu_fb_r_line_count_minus_1(frc),.gpu_fb_r_write_data(64'd0),
 .gpu_fb_r_request_ready(frr),.gpu_fb_r_response_valid(frv),.gpu_fb_r_read_data(frd),.gpu_fb_r_response_last(frl),
 .gpu_fb_w_request_valid(fwq),.gpu_fb_w_write(1'b1),.gpu_fb_w_address(fwa),.gpu_fb_w_line_count_minus_1(fwc),.gpu_fb_w_write_data(fwd),
 .gpu_fb_w_request_ready(fwr),.gpu_fb_w_write_data_ready(fwready),.gpu_fb_w_response_valid(fwv),
 .memory_request_ready(ar),.memory_write_data_ready(awr),.memory_response_valid(av),.memory_read_data(ard),.memory_response_last(alast),.memory_error(ae),
 .memory_request_valid(aq),.memory_write(aw),.memory_line(al),.memory_address(aa),.memory_line_count_minus_1(ac),.memory_write_data(awd),.memory_response_ready(arr));
integer ms=0,mb=0,mi=0,mn=0,md=0;reg mwrite=0;reg[63:0]write_buffer[0:15];
integer writes=0,terms=0,cpu_high_writes=0,cpu_color_writes=0,blocked=0,ce0=0,fb_reads=0,draws=0,slot0=0,slot1=0,dma_overlap=0;
integer request_owner=0,write_owner=0,display_reads=0,display_terms=0,display_outstanding=0,display_base=0,contention=0;
integer swaps=0,presents=0,present_done=0;reg[21:0]last_active='h200000;
// Stop accepting new peer transactions only after software has halted; finish all accepted work.
assign ar=ms==0&&cycle%7!=3&&!halted&&!fault;assign awr=ms==3&&cycle%5!=1;
assign av=ms==2||ms==5;assign ard=mem[mb+mi];assign alast=ms==5||mi==mn-1;assign ae=1'b0;
integer k;
function protected_color;input[21:0]address;begin
 protected_color=(address>=display.active_base&&address<display.active_base+96000)||
 (display_outstanding&&address>=display_base&&address<display_base+96000);
end endfunction
always @(posedge clk)begin
 if(reset)begin ms<=0;mi<=0;end else begin
  if(display_underflow||display_start_error)$fatal(1,"Display health failed cycle=%0d",cycle);
  if(!ce)ce0<=ce0+1;
  if(fwq&&!fwr)blocked<=blocked+1;
  if(display_request&&(mq||roq||frq||fwq))contention<=contention+1;
  if(display_request&&display_ready)begin
   if(display_outstanding)$fatal(1,"Two Display transactions");
   display_outstanding<=1;display_base<=display.active_base;display_reads<=display_reads+1;
   if(display_address<display.active_base||display_address>=display.active_base+96000)$fatal(1,"Display bounds");
  end
  if(display_valid&&display_last)begin display_outstanding<=0;display_terms<=display_terms+1;end
  if(fwq&&fwr)begin
   if(protected_color(fwa))$fatal(1,"GPU accepted write to Front or old outstanding read");
   if(!((fwa>='h200000&&fwa<('h400000+192000)/2)||(fwa>='h218000&&fwa<('h430000+192000)/2)||(fwa>='h230000&&fwa<('h460000+192000)/2)))$fatal(1,"GPU output bounds");
   writes<=writes+1;
  end
  if(gpu.ui_start)uis<=uis+1;
  if(ce&&gpu.ui_done&&gpu.processor.state==18)ui_terminals<=ui_terminals+1;
  if(ce&&gpu.start_fire&&uis%2==1)reloaded_after_ui<=reloaded_after_ui+1;
  if(frq&&frr)fb_reads<=fb_reads+1;
  if(ce&&gpu.start_fire)begin $display("D3_DRAW cycle=%0d slot=%0d",cycle,gpu.slot);draws<=draws+1;if(gpu.slot)slot1<=slot1+1;else slot0<=slot0+1;end
  if(gpu.input_active&&gpu.mesh_loading)dma_overlap<=dma_overlap+1;
  if(aq&&ar)begin
   if(!al||aa[3:0]!=0||ac==2)$fatal(1,"Shared memory framing");
   mb<=aa>>2;mi<=0;mn<=(ac+1)*4;mwrite<=aw;ms<=1;md<=1;
   write_owner<=fwq&&fwr?4:mq&&mready?1:0;
  end
  case(ms)
   1:if(md!=0)md<=md-1;else ms<=mwrite?3:2;
   2:if(arr)begin if(mi==mn-1)begin mi<=0;ms<=0;end else mi<=mi+1;end
   3:if(awr)begin write_buffer[mi]<=awd;if(mi==mn-1)begin mi<=0;md<=5;ms<=4;end else mi<=mi+1;end
   4:if(md!=0)md<=md-1;else ms<=5;
   5:if(arr)begin
    for(k=0;k<16;k=k+1)if(k<mn)mem[mb+k]<=write_buffer[k];
    if(write_owner==4)begin
     if(protected_color(mb<<2))$fatal(1,"GPU committed protected old Front");terms<=terms+1;
    end
    if(write_owner==1)begin
     if(mb>=('h490000/8)&&mb<('h4b0000/8))cpu_high_writes<=cpu_high_writes+1;
     if((mb>='h400000/8&&mb<('h400000+192000)/8)||(mb>='h430000/8&&mb<('h430000+192000)/8))begin
      if(protected_color(mb<<2))$fatal(1,"CPU dirty alias wrote protected Front");cpu_color_writes<=cpu_color_writes+1;
     end
    end
    ms<=0;
   end
  endcase
  if(wen&&di==3&&ch==3)begin if(display.next_pending)$fatal(1,"Pending overwritten");presents<=presents+1;end
  if(wen&&di==7&&ch==2)begin
   if(display.next_pending||display.active_base!=(wdata==1?'h218000:'h200000)||display_outstanding&&display_base!=display.active_base)$fatal(1,"Software released old Front early");
   present_done<=present_done+1;
  end
 end
end
always @(negedge clk)if(!reset&&display.active_base!=last_active)begin
 if(display_outstanding&&display_base==last_active)$fatal(1,"Display changed owner before old read drain");
 last_active<=display.active_base;swaps<=swaps+1;
 $display("D3_SWAP cycle=%0d active=%h reads=%0d terminals=%0d outstanding=%0d",cycle,display.active_base,display_reads,display_terms,display_outstanding);
end

integer command_bytes=0,idx,starts=0,completions=0;
integer stage=0,stage_cycle=0,stage_words=0;
integer cp_open=0,cp_pc=-1,cp_opcode=0,cp_cycle=0;
integer batch_start=0,batch_steps=0,batch_enabled=0,batch_id=0;
integer submit_cycle=0,complete_cycle=0,present_cycle=0;
integer light_count=0;reg [63:0]g_sum=0,h_sum=0;
always @(posedge clk) if(!reset)begin
 if(stage==0&&display_started&&!hold&&!sweep)begin
  stage=100;stage_cycle=cycle;stage_words=cpu.retired_words;
 end
 if(wen&&di==7&&ch==1)begin
  if(stage!=0)$display("CPU_STAGE id=%0d start=%0d end=%0d cycles=%0d retired_words=%0d",stage,stage_cycle,cycle,cycle-stage_cycle,cpu.retired_words-stage_words);
  stage=wdata;stage_cycle=cycle;stage_words=cpu.retired_words;
 end
 if(wen&&di==4&&ch==1)begin starts=starts+1;submit_cycle=cycle;$display("FRAME_SUBMIT cycle=%0d bytes=%0d",cycle,command_bytes);end
 if(wen&&di==7&&ch==0)begin
  completions=completions+1;complete_cycle=cycle;$display("GPU_COMPLETE cycle=%0d gpu_wall_cycles=%0d writes=%0d terminals=%0d",cycle,cycle-submit_cycle,writes,terms);
  if(terms!=writes||draws!=24)$fatal(1,"incomplete full scene");
 end
 if(wen&&di==7&&ch==2)begin present_cycle=cycle;$display("PRESENT_COMPLETE cycle=%0d wait_cycles=%0d",cycle,cycle-complete_cycle);end
 if(ce&&gpu.processor.state==2&&cp_pc!=gpu.processor.diagnostic_pc)begin
  if(cp_open)$display("GPU_COMMAND pc=%0d opcode=%0d start=%0d cycles=%0d",cp_pc,cp_opcode,cp_cycle,cycle-cp_cycle);
  cp_open=1;cp_pc=gpu.processor.diagnostic_pc;cp_opcode=gpu.processor.opcode;cp_cycle=cycle;
 end
 if(cp_open&&!gpu.busy)begin
  $display("GPU_COMMAND pc=%0d opcode=%0d start=%0d cycles=%0d",cp_pc,cp_opcode,cp_cycle,cycle-cp_cycle);cp_open=0;
 end
 if(ce&&gpu.start_fire)begin
  batch_id=batch_id+1;batch_start=cycle;batch_steps=0;batch_enabled=0;
  $display("GPU_BATCH_START id=%0d cycle=%0d slot=%0d light=%0d,%0d,%0d ambient=%0d directional=%0d mode=%0d code=%0d specular=%h unlit=%0d",batch_id,cycle,gpu.slot,$signed(gpu.light_x),$signed(gpu.light_y),$signed(gpu.light_z),gpu.ambient,gpu.directional,gpu.context_mode,gpu.context_code,gpu.specular,gpu.draw_unlit);
 end
 if(ce&&gpu.core.front.meshlet.core.busy)begin
  batch_enabled=batch_enabled+1;
  if(gpu.core.front.meshlet.core.step)batch_steps=batch_steps+1;
 end
 if(ce&&gpu.meshlet_done)$display("GPU_BATCH_END id=%0d start=%0d end=%0d cycles=%0d vt_steps=%0d vt_enabled=%0d",batch_id,batch_start,cycle,cycle-batch_start,batch_steps,batch_enabled);
end
always @(posedge clk)begin
 cycle<=cycle+1;
 if(cycle%100000==0)begin $display("PROFILE_PROGRESS cycle=%0d pc=%h draws=%0d gpu=%0d",cycle,pc,draws,gpu.processor.state);$fflush();end
 if(cycle>12000000)$fatal(1,"one-frame profile watchdog");
 if(!reset)begin
  if(pause)held_response<=held_response+1;
  if(wen&&di==4&&ch==0)command_bytes<=(wdata+1)*4;
  if(fault)$fatal(1,"CPU fault %h",fault_code);
 end
end
initial begin
 for(idx=0;idx<65536;idx=idx+1)rom[idx]=16'h6c00;
 for(idx=0;idx<1048576;idx=idx+1)mem[idx]=0;
 $readmemh("__ASSETS__",mem);$readmemh("__PROGRAM__",rom);
 repeat(3)@(posedge clk);@(negedge clk);reset=0;
 wait(halted||fault);#1;
 if(stage!=0)$display("CPU_STAGE id=%0d start=%0d end=%0d cycles=%0d retired_words=%0d",stage,stage_cycle,cycle,cycle-stage_cycle,cpu.retired_words-stage_words);
 if(fault||signal!==16'h600d||starts!=1||completions!=1||draws!=24||uis!=1||swaps!=1||terms!=writes||display.active_linear!==1'b1)$fatal(1,"profile completion coverage");
 $display("CLASSIC_PROFILE_PASS cycles=%0d retired_words=%0d draws=%0d ui=%0d writes=%0d submit=%0d complete=%0d present=%0d",cycle,cpu.retired_words,draws,uis,writes,submit_cycle,complete_cycle,present_cycle);
 $finish;
end
endmodule
