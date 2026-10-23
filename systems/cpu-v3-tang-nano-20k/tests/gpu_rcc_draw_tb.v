`timescale 1ns/1ps
// Real endpoints, controlled 64-bit DRAM service. No Display runs in this fixture.
module tb;
localparam FAULT=0;
reg clk=0,reset=1;always #5 clk=~clk;
GSR GSR(.GSRI(1'b1));
integer cycle=0;reg[15:0]rom[0:65535];reg[15:0]gold1[0:95999],gold2[0:95999];reg[63:0]asset_initial[0:527];reg[63:0]mem[0:1048575];
wire ireq,iready;wire[31:0]ia;
wire dq,dw,dr,rv,rr,de;wire[31:0]da;wire[15:0]wd,rd;
wire lc,lc_ready,cl;wire[21:0]ls,ca;wire[7:0]lp;
wire[2:0]di;wire[3:0]ch;wire ren,wen;wire[15:0]wdata,rdata,sdata,gdata;
wire hold,clean,invalidate,ready,busy,done,error,sweep;
wire halted,fault;wire[15:0]signal,pc;wire[7:0]fault_code;
wire mq,mw,ml,mready,mwready,mrv,mrr,me;wire[21:0]ma;wire[63:0]mwd,mrd;
integer held_response=0;wire pause=rv&&held_response<5;
__CORE__ cpu(.clk(clk),.reset(reset),.hold(hold||sweep||pause),
 .instruction_request_ready(1'b1),.instruction_response_valid(ireq),.instruction_data(rom[ia[15:0]]),.instruction_error(1'b0),
 .instruction_request_valid(ireq),.instruction_address(ia),.instruction_response_ready(iready),
 .data_request_ready(dr),.data_response_valid(rv),.data_read_data(rd),.data_error(de),
 .data_line_copy_ready(lc_ready),.data_cache_command_error(error),.data_request_valid(dq),.data_write(dw),.data_address(da),.data_write_data(wd),.data_response_ready(rr),
 .data_line_copy_valid(lc),.data_line_copy_source(ls),.data_line_copy_destination_page(lp),.data_line_clean_valid(cl),.data_line_clean_address(ca),
 .device_index(di),.device_channel(ch),.device_read_enable(ren),.device_write_enable(wen),.device_write_data(wdata),.device_read_data(rdata),
 .halted(halted),.halt_signal(signal),.fault(fault),.fault_code(fault_code),.pc(pc));
assign rdata=di==0?sdata:di==4?gdata:di==3&&ch==2?16'h0020:16'd0;
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

// Cache burst memory: commit a write only at its successful terminal response.
integer ms=0,mi=0,mb=0,md=0,cpu_high_writes=0;reg mwrite=0;reg injected=0;
reg[63:0]mwbuf[0:3];
assign mready=ms==0&&cycle%5!=1;
assign mwready=ms==3&&cycle%4!=1;
assign mrv=(ms==2&&cycle%3!=1)||ms==5;
assign mrd=mem[mb+mi];
assign me=FAULT&&ms==5&&mb>=('h490000/8)&&!injected;
always @(posedge clk)begin
 if(reset)begin ms<=0;mi<=0;end else case(ms)
 0:if(mq&&mready)begin
   if(!ml||ma[3:0]!=0)$fatal(1,"CPU cache burst framing");
   mb<=ma>>2;mwrite<=mw;md<=15;mi<=0;ms<=1;
 end
 1:if(md!=0)md<=md-1;else ms<=mwrite?3:2;
 2:if(mrv&&mrr)begin if(mi==3)begin mi<=0;ms<=0;end else mi<=mi+1;end
 3:if(mwready)begin mwbuf[mi]<=mwd;if(mi==3)begin mi<=0;md<=4;ms<=4;end else mi<=mi+1;end
 4:if(md!=0)md<=md-1;else ms<=5;
 5:if(mrr)begin
   if(me)injected<=1;
   else begin
     mem[mb]<=mwbuf[0];mem[mb+1]<=mwbuf[1];mem[mb+2]<=mwbuf[2];mem[mb+3]<=mwbuf[3];
     if(mb>=('h490000/8)&&mb<('h4b0000/8))cpu_high_writes<=cpu_high_writes+1;
   end
   ms<=0;
 end
 endcase
end
integer ro_active=0,ro_base=0,ro_i=0,ro_n=0,ro_reads=0;
assign ror=!ro_active&&cycle%5!=3;
assign rov=ro_active&&cycle%3!=2;
assign rol=ro_i==ro_n-1;
assign rod=mem[ro_base+ro_i];
always @(posedge clk)if(!reset)begin
 if(roq&&ror)begin
   if(roa[3:0]!=0||!(roc==0||roc==1||roc==3))$fatal(1,"GPU RO framing");
   ro_base<=roa>>2;ro_i<=0;ro_n<=(roc+1)*4;ro_active<=1;ro_reads<=ro_reads+1;
 end else if(rov)begin if(rol)ro_active<=0;else ro_i<=ro_i+1;end
end
integer fr_active=0,fr_base=0,fr_i=0,fr_n=0,fb_reads=0;
assign frr=!fr_active&&cycle%5!=2;
assign frv=fr_active&&cycle%3!=1;
assign frl=fr_i==fr_n-1;
assign frd=mem[fr_base+fr_i];
always @(posedge clk)if(!reset)begin
 if(frq&&frr)begin
  if(fra[3:0]!=0||!(frc==0||frc==1||frc==3))$fatal(1,"FB read framing");
  if(!((fra>=('h430000/2)&&fra<('h430000+192000)/2)||(fra>=('h460000/2)&&fra<('h460000+192000)/2)))$fatal(1,"FB read bounds");
  fr_base<=fra>>2;fr_i<=0;fr_n<=(frc+1)*4;fr_active<=1;fb_reads<=fb_reads+1;
 end else if(frv)begin if(frl)fr_active<=0;else fr_i<=fr_i+1;end
end
integer draws=0,slot0=0,slot1=0,dma_overlap=0;
always @(posedge clk)if(!reset)begin
 if(ce&&gpu.start_fire)begin draws<=draws+1;if(gpu.slot)slot1<=slot1+1;else slot0<=slot0+1;end
 if(gpu.input_active&&gpu.mesh_loading)dma_overlap<=dma_overlap+1;
end
integer wa=0,wb=0,wi=0,delay_w=0,writes=0,terms=0,blocked=0,ce0=0;
reg[63:0]buf_w[0:15];reg offered=0;reg[23:0]held_offer=0;
assign fwr=!wa&&cycle%5!=1;
assign fwready=wa&&wi<16&&cycle%4!=2;
assign fwv=wa&&wi==16&&delay_w==0;
integer k;
always @(posedge clk)if(!reset)begin
 if(!ce)ce0<=ce0+1;
 if(offered&&(!fwq||{fwc,fwa}!==held_offer))$fatal(1,"GPU request changed under backpressure");
 offered<=fwq&&!fwr;held_offer<={fwc,fwa};if(fwq&&!fwr)blocked<=blocked+1;

 if(fwq&&fwr)begin
   if(fwc!=3||fwa[5:0]!=0)$fatal(1,"GPU write framing");
   if(!((fwa>=('h430000/2)&&fwa<('h430000/2)+96000)||
        (fwa>=('h460000/2)&&fwa<('h460000/2)+96000)))$fatal(1,"GPU wrote Front/padding/neighbour");
   wa<=1;wb<=fwa>>2;wi<=0;delay_w<=5;writes<=writes+1;
 end else if(wa)begin
   if(fwready)begin buf_w[wi]<=fwd;wi<=wi+1;end
   if(wi==16&&delay_w!=0)delay_w<=delay_w-1;
   if(fwv)begin
     for(k=0;k<16;k=k+1)mem[wb+k]<=buf_w[k];
     terms<=terms+1;wa<=0;
   end
 end
end

integer starts=0,completions=0,maintenance=0,fst_address=0,fst_cycles=0,idx;
task check_surface;input[15:0]frame;integer q,x,y,off,half,diff;reg[63:0]expected;reg[15:0]actual,want;begin
 for(q='h400000/8-16;q<'h4b0000/8+16;q=q+1)begin
   expected=64'h55aa55aa55aa55aa;
   if(q>='h430000/8&&q<('h430000+192000)/8)expected=mem[q];
   else if(q>='h460000/8&&q<('h460000+192000)/8)expected=mem[q];
   // CPU owns only the first command sector and the 32-byte object in each slot.
   else if((q>='h490000/8&&q<'h490180/8)||(q>='h4a0000/8&&q<'h4b0000/8))expected=mem[q];
   if(q>='h4b0000/8)expected=asset_initial[q-'h4b0000/8];
   if(mem[q]!==expected)$fatal(1,"Framebuffer/padding/Front mismatch q=%h got=%h expected=%h",q,mem[q],expected);
 end
 for(q=0;q<528;q=q+1)if(mem['h4b0000/8+q]!==asset_initial[q])$fatal(1,"Immutable asset changed");
 for(q=260;q<384;q=q+2)if(mem[('h490000+q)/8][(q%8)*8+:16]!==0)$fatal(1,"Command padding changed");
 for(y=0;y<240;y=y+1)for(x=0;x<400;x=x+1)begin
   off=((y/16)*25+x/16)*512+(y%16)*32+(x%16)*2;
   half=off%8*8;
   actual=mem[('h430000+off)/8]>>half;want=frame==1?gold1[y*400+x]:gold2[y*400+x];
   if(actual!==want)$fatal(1,"Color mismatch frame=%0d x=%0d y=%0d actual=%h expected=%h",frame,x,y,actual,want);
   actual=mem[('h460000+off)/8]>>half;
   if(want==frame)begin if(actual!==16'hffff)$fatal(1,"Depth touched outside coverage");end
   else begin diff=actual>256?actual-256:256-actual;if(diff>1)$fatal(1,"Depth mismatch x=%0d y=%0d value=%h",x,y,actual);end
 end
end endtask
always @(posedge clk)begin
 cycle<=cycle+1;
 if(cycle%20000==0)begin $display("RCC_TRACE cycle=%0d pc=%h cpu=%0d gpu=%0d writes=%0d",cycle,pc,cpu.state,gpu.processor.state,writes);$fflush();end
 if(cycle>2000000)$fatal(1,"RCC GPU watchdog pc=%h cpu=%0d gpu=%0d status=%h writes=%0d",pc,cpu.state,gpu.processor.state,gdata,writes);
 if(!reset)begin
   if(pause)held_response<=held_response+1;
   if(cpu.fpu2_pending!=0)fst_cycles<=fst_cycles+1;
   if(wen&&di==0&&(ch==1||ch==4))begin
     if(cpu.async_store_valid||cpu.fpu2_pending!=0||cpu.fpu2_busy)$fatal(1,"Maintenance overtook old work");
     maintenance<=maintenance+1;
   end
   if(wen&&di==4&&ch==1)begin
     if(wdata!='h9200||FAULT||maintenance==0||cpu_high_writes==0)$fatal(1,"Unexpected GPU launch/visibility");
     starts<=starts+1;

   end
   if(wen&&di==7&&ch==1)fst_address<=wdata;
   if(wen&&di==7&&ch==0)begin
     if(wdata==1||wdata==2)check_surface(wdata);
     else $fatal(1,"Bad completion marker");
     if(wa||ro_active||fr_active||terms!=writes||draws!=wdata*9)$fatal(1,"Completion precedes actual memory drain");
     completions<=completions+1;
   end
 end
end
initial begin
 for(idx=0;idx<65536;idx=idx+1)rom[idx]=16'h6c00;
 for(idx=0;idx<32768;idx=idx+1)mem[idx]=0;
 for(idx='h400000/8-16;idx<'h4b1080/8+16;idx=idx+1)mem[idx]=64'h55aa55aa55aa55aa;
 $readmemh("__ASSETS__",mem);
 for(idx=0;idx<528;idx=idx+1)asset_initial[idx]=mem['h4b0000/8+idx];
 $readmemh("__GOLD1__",gold1);$readmemh("__GOLD2__",gold2);
 $readmemh("__PROGRAM__",rom);
 repeat(3)@(posedge clk);@(negedge clk);reset=0;
 wait(halted||fault);#1;
 if(fault||signal!==16'h600d)$fatal(1,"RCC failure pc=%h code=%0d signal=%h",pc,fault_code,signal);
 if(FAULT)begin if(!injected||starts!=0||writes!=0)$fatal(1,"Maintenance fault launched GPU");end
 else if(starts!=2||completions!=2||writes<6000||terms!=writes||blocked==0||ce0==0||held_response<5||draws!=18||slot0==0||slot1==0||dma_overlap==0||fb_reads==0)
   $fatal(1,"Coverage starts=%0d done=%0d writes=%0d terms=%0d blocked=%0d ce0=%0d FST=%0d response=%0d",starts,completions,writes,terms,blocked,ce0,fst_cycles,held_response);
 $display("RCC_DRAW_PASS fault_case=%0d cycles=%0d starts=%0d completions=%0d writes=%0d true_terminals=%0d maintenance=%0d CPU_high_writebacks=%0d",FAULT,cycle,starts,completions,writes,terms,maintenance,cpu_high_writes);
 $display("D2_COVERAGE draws=%0d slot0=%0d slot1=%0d overlap=%0d FBreads=%0d",draws,slot0,slot1,dma_overlap,fb_reads);
 $finish;
end
endmodule
