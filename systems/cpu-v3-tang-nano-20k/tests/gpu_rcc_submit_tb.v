`timescale 1ns/1ps
// Real endpoints, controlled 64-bit DRAM service. No Display runs in this fixture.
module tb;
localparam FAULT=__FAULT__;
reg clk=0,reset=1;always #5 clk=~clk;
GSR GSR(.GSRI(1'b1));
integer cycle=0;reg[15:0]rom[0:65535];reg[63:0]mem[0:1048575];
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
assign rdata=di==0?sdata:di==4?gdata:16'd0;
__SYSCTL__ sysctl(.clk(clk),.reset(reset),.device_index(di),.device_channel(ch),.device_read_enable(ren),.device_write_enable(wen),.device_write_data(wdata),.device_read_data(sdata),
 .dcache_maintenance_ready(ready),.dcache_maintenance_done(done),.dcache_maintenance_error(error),.watch_read_data(16'd0),
 .cpu_hold(hold),.dcache_clean(clean),.dcache_invalidate(invalidate));
__CACHE__ cache(.clk(clk),.reset(reset),.clean_all(clean),.invalidate_all(invalidate),.line_copy_start(lc),.line_copy_source(ls),.line_copy_destination_page(lp),.line_clean_start(cl),.line_clean_address(ca),.line_copy_ready(lc_ready),
 .cpu_request_valid(dq),.cpu_write(dw),.cpu_address(da),.cpu_write_data(wd),.cpu_response_ready(rr),.cpu_request_ready(dr),.cpu_response_valid(rv),.cpu_read_data(rd),.cpu_error(de),
 .memory_request_ready(mready),.memory_write_data_ready(mwready),.memory_response_valid(mrv),.memory_read_data(mrd),.memory_error(me),.memory_request_valid(mq),.memory_write(mw),.memory_line(ml),.memory_address(ma),.memory_write_data(mwd),.memory_response_ready(mrr),
 .maintenance_ready(ready),.maintenance_busy(busy),.maintenance_done(done),.maintenance_error(error),.valid_sweep(sweep));

wire roq,ror,rov,rol,frq,fwq,fwr,fwready,fwv;
wire[21:0]roa,fra,fwa;wire[1:0]roc,frc,fwc;wire[63:0]rod,fwd;
wire ce=cycle%7!=2;
gpu_v2_vliw_command gpu(.clk(clk),.reset(reset),.ce(ce),.abort(1'b0),.output_ready(1'b1),
 .device_index(di),.device_channel(ch),.device_read_enable(ren),.device_write_enable(wen),.device_write_data(wdata),.device_read_data(gdata),
 .ro_request_valid(roq),.ro_request_ready(ror),.ro_address(roa),.ro_count(roc),.ro_response_valid(rov),.ro_response_last(rol),.ro_response_error(1'b0),.ro_data(rod),
 .fb_r_request_valid(frq),.fb_r_request_ready(1'b0),.fb_r_address(fra),.fb_r_count(frc),.fb_r_response_valid(1'b0),.fb_r_response_last(1'b0),.fb_r_response_error(1'b0),.fb_r_data(64'd0),
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
 if(frq)$fatal(1,"Clear-only pass unexpectedly reads framebuffer");
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
task check_surface;input[15:0]color,depth;integer q;reg[63:0]expected;begin
 for(q='h400000/8-16;q<'h4b0000/8+16;q=q+1)begin
   expected=64'h55aa55aa55aa55aa;
   if(q>='h430000/8&&q<('h430000+192000)/8)expected={4{color}};
   else if(q>='h460000/8&&q<('h460000+192000)/8)expected={4{depth}};
   // CPU owns only the first command sector and the 32-byte object in each slot.
   else if((q>='h490000/8&&q<'h490080/8)||(q>='h4a0000/8&&q<'h4a0020/8))expected=mem[q];
   if(mem[q]!==expected)$fatal(1,"Framebuffer/padding/Front mismatch q=%h got=%h expected=%h",q,mem[q],expected);
 end
end endtask
always @(posedge clk)begin
 cycle<=cycle+1;
 if(cycle%20000==0)begin $display("RCC_TRACE cycle=%0d pc=%h cpu=%0d gpu=%0d writes=%0d",cycle,pc,cpu.state,gpu.processor.state,writes);$fflush();end
 if(cycle>1000000)$fatal(1,"RCC GPU watchdog pc=%h cpu=%0d gpu=%0d status=%h writes=%0d",pc,cpu.state,gpu.processor.state,gdata,writes);
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
     for(idx=0;idx<8;idx=idx+1)
       if(mem[(fst_address+idx)>>2][((fst_address+idx)%4)*16+:16]!==((idx%2)?idx/2+1:0))$fatal(1,"Old FST not visible before launch");
   end
   if(wen&&di==7&&ch==1)fst_address<=wdata;
   if(wen&&di==7&&ch==0)begin
     if(wdata==1)check_surface('h1357,'h2468);
     else if(wdata==2)check_surface('habcd,'h5678);
     else $fatal(1,"Bad completion marker");
     if(wa||ro_active||terms!=wdata*3000)$fatal(1,"Completion precedes actual memory drain");
     completions<=completions+1;
   end
 end
end
initial begin
 for(idx=0;idx<65536;idx=idx+1)rom[idx]=16'h6c00;
 for(idx=0;idx<32768;idx=idx+1)mem[idx]=0;
 for(idx='h400000/8-16;idx<'h4b0000/8+16;idx=idx+1)mem[idx]=64'h55aa55aa55aa55aa;
 $readmemh("__PROGRAM__",rom);
 repeat(3)@(posedge clk);@(negedge clk);reset=0;
 wait(halted||fault);#1;
 if(fault||signal!==16'h600d)$fatal(1,"RCC failure pc=%h code=%0d signal=%h",pc,fault_code,signal);
 if(FAULT)begin if(!injected||starts!=0||writes!=0)$fatal(1,"Maintenance fault launched GPU");end
 else if(starts!=2||completions!=2||writes!=6000||terms!=writes||blocked==0||ce0==0||fst_cycles==0||held_response<5)
   $fatal(1,"Coverage starts=%0d done=%0d writes=%0d terms=%0d blocked=%0d ce0=%0d FST=%0d response=%0d",starts,completions,writes,terms,blocked,ce0,fst_cycles,held_response);
 $display("RCC_GPU_PASS fault_case=%0d cycles=%0d starts=%0d completions=%0d writes=%0d true_terminals=%0d maintenance=%0d CPU_high_writebacks=%0d",FAULT,cycle,starts,completions,writes,terms,maintenance,cpu_high_writes);
 $finish;
end
endmodule
