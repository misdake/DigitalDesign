`timescale 1ns/1ps
// Real CPU/cache/GPU/Display + production arbiter, controlled 64-bit memory service.
// Accelerated pixel clock is a protocol stress profile, not board clock qualification.
module tb;
reg[15:0]depth1[0:95999],depth2[0:95999],matrix1[0:41],matrix2[0:41];reg[63:0]atlas_initial[0:8191];integer uis=0,ui_terminals=0,reloaded_after_ui=0;
localparam FAULT=0;
reg clk=0,reset=1,pixel_clock=0;real pixel_half_period=0.05;always #5 clk=~clk;always #(pixel_half_period) pixel_clock=~pixel_clock;
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
always @(posedge display_started) pixel_half_period=2.5;
integer held_response=0;wire pause=rv&&held_response<5;
__CORE__ cpu(.clk(clk),.reset(reset),.hold(hold||sweep||pause||!display_started),
 .instruction_request_ready(1'b1),.instruction_response_valid(ireq),.instruction_data(rom[ia[15:0]]),.instruction_error(1'b0),
 .instruction_request_valid(ireq),.instruction_address(ia),.instruction_response_ready(iready),
 .data_request_ready(dr),.data_response_valid(rv),.data_read_data(rd),.data_error(de),
 .data_line_copy_ready(lc_ready),.data_cache_command_error(error),.data_request_valid(dq),.data_write(dw),.data_address(da),.data_write_data(wd),.data_response_ready(rr),
 .data_line_copy_valid(lc),.data_line_copy_source(ls),.data_line_copy_destination_page(lp),.data_line_clean_valid(cl),.data_line_clean_address(ca),
 .device_index(di),.device_channel(ch),.device_read_enable(ren),.device_write_enable(wen),.device_write_data(wdata),.device_read_data(rdata),
 .halted(halted),.halt_signal(signal),.fault(fault),.fault_code(fault_code),.pc(pc));
assign rdata=di==0?sdata:di==4?gdata:di==3?display_data:16'd0;
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
wire ce=cycle%7!=2;
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
integer pixel_x=0,pixel_y=0,scan_version=0,samples_b=0,samples_a=0;reg visible_before=0;reg[15:0]pixel;reg[23:0]rgb;
always @(posedge pixel_clock)begin
 if(display.h_count==216)begin
  pixel_y=(display.v_count-32)/2;
  if(display.v_count==32)scan_version=display.active_base=='h218000 ? 1 : (swaps>=2 ? 2 : 0);
 end
 if(display.framebuffer_pipe3&&display.visible_pipe3&&!visible_before)pixel_x=0;
 visible_before<=display.framebuffer_pipe3&&display.visible_pipe3;
 if(display.framebuffer_pipe3&&display.visible_pipe3)begin
  if(scan_version==1)begin pixel=gold1[pixel_y*400+pixel_x/2];samples_b=samples_b+1;end
  else if(scan_version==2)begin pixel=gold2[pixel_y*400+pixel_x/2];samples_a=samples_a+1;end
  else pixel='h55aa;
  rgb={pixel[15:11],pixel[15:13],pixel[10:5],pixel[10:9],pixel[4:0],pixel[4:2]};
  if(display.rgb_pipe!==rgb)$fatal(1,"Scan RGB mismatch version=%0d x=%0d y=%0d got=%h want=%h",scan_version,pixel_x,pixel_y,display.rgb_pipe,rgb);
  pixel_x=pixel_x+1;
 end
end

integer command_bytes=0;
integer starts=0,completions=0,maintenance=0,fst_address=0,fst_cycles=0,idx;

integer boundaries=0,previous_start=0,last_timer=0,interval_ms=0;
integer uart_index=0,initial_reports=0,normal_reports=0,error_reports=0,retained_state=-1,uart_xor;
reg[7:0]uart_packet[0:10];
always @(posedge clk)if(!reset)begin
 if(wen&&di==7&&ch==7)retained_state<=wdata;
 if(wen&&di==0&&ch==3)begin
  if(wdata[15:8]!==0)$fatal(1,"UART byte has unknown/high bits");
  uart_packet[uart_index]=wdata[7:0];
  if(uart_index==10)begin
   if({uart_packet[0],uart_packet[1],uart_packet[2],uart_packet[3],uart_packet[4]}!==40'h4444474401)$fatal(1,"Demo UART header");
   uart_xor=0;for(integer j=4;j<10;j=j+1)uart_xor=uart_xor^uart_packet[j];
   if(uart_packet[10]!==uart_xor[7:0])$fatal(1,"Demo UART checksum");
   if(uart_packet[5]==0)initial_reports=initial_reports+1;
   else if(uart_packet[5]==1)begin
    if(boundaries!=2||{uart_packet[7],uart_packet[6]}!==16'd1||{uart_packet[9],uart_packet[8]}!==interval_ms[15:0])$fatal(1,"Incorrect previous successful frame telemetry");
    normal_reports=normal_reports+1;
   end else if(uart_packet[5][7])error_reports=error_reports+1;
   else $fatal(1,"Unknown demo telemetry status");
   uart_index=0;
  end else uart_index=uart_index+1;
 end
end
integer ref_ticks=0,ref_ms=0;
always @(posedge clk)begin
 if(reset)begin ref_ticks<=0;ref_ms<=0;end
 else begin
  if(ref_ticks==53999)begin ref_ticks<=0;ref_ms<=(ref_ms+1)&65535;end
  else ref_ticks<=ref_ticks+1;
  if(ren&&di==0&&ch==10)begin
   if(sdata!==ref_ms[15:0])$fatal(1,"Wall-clock mismatch %h/%h",sdata,ref_ms);
   last_timer<=ref_ms;
  end
 end
end
task overlay;input integer frame,elapsed;integer glyph_index,glyph,x,y,at,power;reg[15:0]word;begin
 for(glyph_index=0;glyph_index<14;glyph_index=glyph_index+1)begin
  case(glyph_index)
   0:glyph=70;1:glyph=82;2:glyph=65;3:glyph=77;4:glyph=69;
   5,11:glyph=32;12:glyph=77;13:glyph=83;
   default:begin
    power=glyph_index==6?10000:glyph_index==7?1000:glyph_index==8?100:glyph_index==9?10:1;
    glyph=frame==1?45:48+(elapsed/power)%10;
   end
  endcase
  for(y=0;y<16;y=y+1)for(x=0;x<8;x=x+1)begin
   at=glyph*128+y*8+(x^((y&1)*2));word=atlas_initial[at/4]>>((at%4)*16);
   if(word!==16'h0000&&word!==16'hffff)$fatal(1,"Invalid audited font pixel");
   if(word[0])begin
    at=(16+y)*400+(2+glyph_index)*8+x;
    if(frame==1)gold1[at]=16'hffff;else gold2[at]=16'hffff;
   end
  end
 end
end endtask
task check_surface;input[15:0]frame;integer q,x,y,off,half,file;reg[63:0]expected;reg[15:0]actual,want;begin
 for(q='h400000/8-16;q<'h4d0000/8+16;q=q+1)begin
  expected=64'h55aa55aa55aa55aa;
  if(q>='h400000/8&&q<('h400000+192000)/8&&frame==2)expected=mem[q];
  if(q>='h430000/8&&q<('h430000+192000)/8)expected=mem[q];
  else if(q>='h460000/8&&q<('h460000+192000)/8)expected=mem[q];
  else if((q>='h490000/8&&q<('h490000+((command_bytes+127)&~127))/8)||(q>='h4a0000/8&&q<'h4b0000/8))expected=mem[q];
  if(q>='h4b0000/8&&q<'h4d0000/8)expected=asset_initial[q-'h4b0000/8];
  if(mem[q]!==expected)$fatal(1,"Ownership/padding/immutable mismatch %h",q);
 end
 for(q=command_bytes;q<((command_bytes+127)&~127);q=q+1)
  if((mem[('h490000+q)/8]>>((q%8)*8)&255)!==0)$fatal(1,"Command padding changed");
 file=$fopen(frame==1?"__ACTUAL1__":"__ACTUAL2__","w");
 for(y=0;y<240;y=y+1)for(x=0;x<400;x=x+1)begin
  off=((y/16)*25+x/16)*512+(y%16)*32+(x%16)*2;half=off%8*8;
  actual=mem[((frame==1?'h430000:'h400000)+off)/8]>>half;
  want=frame==1?gold1[y*400+x]:gold2[y*400+x];
  if(actual!==want)$fatal(1,"Demo color frame=%0d x=%0d y=%0d actual=%h want=%h",frame,x,y,actual,want);
  $fdisplay(file,"%04h",actual);
  actual=mem[('h460000+off)/8]>>half;want=frame==1?depth1[y*400+x]:depth2[y*400+x];
  if(actual!==want)$fatal(1,"Demo Depth frame=%0d x=%0d y=%0d actual=%h want=%h",frame,x,y,actual,want);
 end
 $fclose(file);
 $display("DEMO_SURFACE_PASS frame=%0d bytes=%0d previous_ms=%0d",frame,command_bytes,interval_ms);
end endtask

always @(posedge clk)begin
 cycle<=cycle+1;
 if(cycle%20000==0)begin $display("RCC_TRACE cycle=%0d pc=%h cpu=%0d gpu=%0d writes=%0d",cycle,pc,cpu.state,gpu.processor.state,writes);$fflush();end
 if(cycle>6000000)$fatal(1,"RCC GPU watchdog pc=%h cpu=%0d gpu=%0d status=%h writes=%0d",pc,cpu.state,gpu.processor.state,gdata,writes);
 if(!reset)begin
   if(pause)held_response<=held_response+1;
   if(cpu.fpu2_pending!=0)fst_cycles<=fst_cycles+1;
   if(wen&&di==0&&(ch==1||ch==4))begin
     if(cpu.async_store_valid||cpu.fpu2_pending!=0||cpu.fpu2_busy)$fatal(1,"Maintenance overtook old work");
     maintenance<=maintenance+1;
   end
   if(wen&&di==4&&ch==0)command_bytes<=(wdata+1)*4;
   if(wen&&di==4&&ch==1)begin
     if(command_bytes==0||command_bytes>65536)$fatal(1,"Invalid submitted command count");
     if(wdata!='h9200||FAULT||maintenance==0||cpu_high_writes==0)$fatal(1,"Unexpected GPU launch/visibility");
     starts<=starts+1;

     for(idx=0;idx<42;idx=idx+1)
      if(mem[('h4a0100+idx*2)/8][((idx*2)%8)*8+:16] !== (starts==0?matrix1[idx]:matrix2[idx]))
       $fatal(1,"RCC camera snapshot mismatch frame=%0d word=%0d",starts+1,idx);

   end

   if(wen&&di==7&&ch==5)begin
    if(wdata!==last_timer[15:0])$fatal(1,"Boundary timestamp is not actual timer read");
    if(boundaries>=2)$fatal(1,"Unexpected third bounded frame");
    interval_ms=(last_timer-previous_start)&65535;
    if(boundaries==1&&(present_done!=1||completions!=1||interval_ms==0))$fatal(1,"Invalid previous complete-loop time");
    boundaries=boundaries+1;overlay(boundaries,interval_ms);previous_start=last_timer;
   end
   if(wen&&di==7&&ch==6&&wdata!==(boundaries==1?16'd0:16'd128))$fatal(1,"Camera did not rotate");

   if(wen&&di==7&&ch==0)begin
     if(uis!=wdata||ui_terminals!=uis)$fatal(1,"UI retired before its real completion");
     if(wdata==1||wdata==2)check_surface(wdata);
     else $fatal(1,"Bad completion marker");
     if(terms!=writes||draws!=wdata*6)$fatal(1,"Completion precedes actual memory drain");
     completions<=completions+1;
   end
 end
end
initial begin
 for(idx=0;idx<65536;idx=idx+1)rom[idx]=16'h6c00;
 for(idx=0;idx<32768;idx=idx+1)mem[idx]=0;
 for(idx='h400000/8-16;idx<'h4d0000/8+16;idx=idx+1)mem[idx]=64'h55aa55aa55aa55aa;
 for(idx='h4c0000/8-16;idx<'h4d0000/8+16;idx=idx+1)mem[idx]=64'h55aa55aa55aa55aa;
 $readmemh("__ASSETS__",mem);
 for(idx=0;idx<8192;idx=idx+1)atlas_initial[idx]=mem['h4b0000/8+idx];
 $readmemh("__DEPTH1__",depth1);$readmemh("__DEPTH2__",depth2);
 $readmemh("__MATRIX1__",matrix1);$readmemh("__MATRIX2__",matrix2);
 for(idx=0;idx<16384;idx=idx+1)asset_initial[idx]=mem['h4b0000/8+idx];
 $readmemh("__GOLD1__",gold1);$readmemh("__GOLD2__",gold2);
 $readmemh("__PROGRAM__",rom);
 repeat(3)@(posedge clk);@(negedge clk);reset=0;
 wait(halted||fault);#1;
 $display("D3_HALT draws=%0d slot0=%0d slot1=%0d FBreads=%0d swaps=%0d presents=%0d observed=%0d scans_B=%0d scans_A=%0d display_reads=%0d display_terms=%0d outstanding=%0d peer_state=%0d contention=%0d alias_WB=%0d",draws,slot0,slot1,fb_reads,swaps,presents,present_done,samples_b,samples_a,display_reads,display_terms,display_outstanding,ms,contention,cpu_color_writes);
 // Display runs independently of CPU halt. Snapshot after the last already
 // accepted response, rather than requiring an arbitrary halt edge to be idle.
 wait(ms==0&&!display_outstanding);repeat(3)@(posedge clk);#1;
 if(fault||signal!==16'h600d)$fatal(1,"RCC failure pc=%h code=%0d signal=%h",pc,fault_code,signal);

 if(uis!=1||ui_terminals!=1||boundaries!=1||initial_reports!=1||normal_reports!=0||error_reports!=0||uart_index!=0)$fatal(1,"Demo UI/boundary/telemetry coverage");
 if(starts!=1||completions!=1||writes<3000||terms!=writes||blocked==0||ce0==0||held_response<5||draws!=6||slot0!=3||slot1!=3||fb_reads==0||swaps!=1||presents!=1||present_done!=1||samples_b<384000||samples_a!=0||display_terms!=display_reads||contention==0||cpu_color_writes!=0)
  $fatal(1,"Demo coverage incomplete");
 $display("GPU_DEMO_PASS cycles=%0d draws=%0d UI=%0d writes=%0d terminals=%0d swaps=%0d scans_B=%0d scans_A=%0d previous_ms=%0d",cycle,draws,uis,writes,terms,swaps,samples_b,samples_a,interval_ms);
 $finish;
end
endmodule
