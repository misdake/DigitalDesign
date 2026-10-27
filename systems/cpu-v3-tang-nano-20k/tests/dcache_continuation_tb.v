`timescale 1ns/1ps
module tb;
@PORTS@
wire clk=logic_clk;
always #5.051 controller_clk=~controller_clk;
always @(posedge controller_clk)logic_clk=~logic_clk;
initial begin #13.259;forever begin sdram_clk=~sdram_clk;#5.051;end end
GSR GSR(.GSRI(1'b1));
GowinSdramCombination dut(.*);
LabPinModel #(.PERIOD_NS(10.102),.RETURN_MIN(1),.RETURN_MAX(4)) pin(
 .sclk(O_sdram_clk),.reset(reset),.cke(O_sdram_cke),.cs(O_sdram_cs_n),.ras(O_sdram_ras_n),
 .cas(O_sdram_cas_n),.we(O_sdram_wen_n),.dqm(O_sdram_dqm),.a(O_sdram_addr),.ba(O_sdram_ba),.dq(IO_sdram_dq),.cycle(),.refreshes());
reg clean=0,inval=0,q=0,wr=0;reg[31:0]addr=0;reg[15:0]value=0;
wire ready,valid,error,maint_ready,maint_busy,maint_done,maint_error,sweep;
wire[15:0]read_data;
integer inject=0;
integer gpu_contention=0,gpu_pending=0,gpu_terms=0;
integer cycle=0,phase=0,requests=0,terms=0,early=0,chains=0,reads=0,display_terms=0,mode=0,lines=128,stride=1,shuffle=0,start=0,i,j,index,w,unused,pending_display=0,baseline=0,elapsed=0;
wire cache_error=data_error || (inject && phase==1 && terms==1 && data_response_valid);
CpuV3DataCache cache(.clk(clk),.reset(reset),.clean_all(clean),.invalidate_all(inval),
 .line_copy_start(1'b0),.line_copy_source(22'd0),.line_copy_destination_page(8'd0),.line_clean_start(1'b0),.line_clean_address(22'd0),
 .cpu_request_valid(q),.cpu_write(wr),.cpu_address(addr),.cpu_write_data(value),.cpu_response_ready(1'b1),
 .cpu_request_ready(ready),.cpu_response_valid(valid),.cpu_read_data(read_data),.cpu_error(error),
 .memory_request_valid(data_request_valid),.memory_write(data_write),.memory_line(data_line),.memory_address(data_address),.memory_write_data(data_write_data),
 .memory_request_ready(data_request_ready),.memory_write_data_ready(data_write_data_ready),.memory_response_valid(data_response_valid),.memory_read_data(data_read_data),.memory_error(cache_error),.memory_response_ready(data_response_ready),
 .maintenance_ready(maint_ready),.maintenance_busy(maint_busy),.maintenance_done(maint_done),.maintenance_error(maint_error),.valid_sweep(sweep));
function[20:0]native_word;input integer bytes;integer w;begin w=bytes/4;native_word=((w>>5)&3)*524288+(w&31)+((w>>7)<<5);end endfunction
function[15:0]seed;input integer w;begin seed=16'h91a3^(w*73)^(w>>3);end endfunction
always @(posedge controller_clk)if(!reset&&dut.bridge.u_controller.chain_accept)chains=chains+1;
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>150000)$fatal(1,"D-cache native cycle bound state=%0d",cache.state);
 if(!reset)begin
  if(error)$fatal(1,"CPU cache fault");
  if(data_request_valid&&data_request_ready)begin
   if(!data_write)reads=reads+1;
   if(phase==1)begin
    if(!data_write)$fatal(1,"maintenance issued refill");
    if(requests!=terms)early=early+1;
    requests=requests+1;
   end
  end
  if(phase==1&&data_response_valid&&data_response_ready)terms=terms+1;
  if(display_request_valid&&display_request_ready)begin display_request_valid<=0;pending_display=1;end
  if(display_response_valid&&display_response_last)begin pending_display=0;display_terms=display_terms+1;end
  if(gpu_fb_r_request_valid&&gpu_fb_r_request_ready)begin gpu_fb_r_request_valid<=0;gpu_pending=1;end
  if(gpu_fb_r_response_valid)begin
   if(gpu_fb_r_read_data!==64'h1234567812345678||gpu_fb_r_error)$fatal(1,"direction-disjoint class data");
   if(gpu_fb_r_response_last)begin gpu_pending=0;gpu_terms=gpu_terms+1;end
  end
 end
end
always @(negedge clk)if(phase==1&&!inject&&!pending_display&&cycle%83==0)begin display_request_valid=1;display_address='h4000;end
always @(negedge clk)if(phase==1&&gpu_contention&&!gpu_pending&&cycle%59==0)begin gpu_fb_r_request_valid=1;gpu_fb_r_address='h4000;gpu_fb_r_line_count_minus_1=1;end
// Request stability and beat stability have different owners.
reg held_req=0,held_data=0;reg[21:0]kept_addr;reg[63:0]kept_data;
always @(posedge clk)if(!reset)begin
 if(held_req&&(!data_request_valid||data_address!==kept_addr))$fatal(1,"held descriptor changed");
 held_req=data_request_valid&&!data_request_ready;kept_addr=data_address;
 // A stalled beat is meaningful only for a granted source; the cache's
 // synchronous RAM can prime before grant without owning a payload beat.
 if(held_data&&data_write_data!==kept_data)$fatal(1,"held payload changed");
 held_data=cache.state==8&&!data_write_data_ready&&!cache_error;
 kept_data=data_write_data;
end
 task access;input write;input integer address;input[15:0]v,expected;begin
  @(negedge clk);wait(ready);wr=write;addr=address;value=v;q=1;
  @(posedge clk);@(negedge clk);q=0;wait(valid);#1;
  if(error||(!write&&read_data!==expected))$fatal(1,"CPU value mismatch addr=%h got=%h expected=%h",address,read_data,expected);
  @(posedge clk);@(negedge clk);
 end endtask
initial begin
 unused=$value$plusargs("MODE=%d",mode);unused=$value$plusargs("LINES=%d",lines);unused=$value$plusargs("STRIDE=%d",stride);unused=$value$plusargs("SHUFFLE=%d",shuffle);unused=$value$plusargs("ERROR=%d",inject);
 unused=$value$plusargs("BASELINE=%d",baseline);
 unused=$value$plusargs("GPUCONTEND=%d",gpu_contention);
 reset=1;display_response_ready=1;instruction_response_ready=1;dma_response_ready=1;
 for(i=0;i<4096;i=i+2)pin.seed_word(native_word(i*2),{seed(i+1),seed(i)});
 for(i='h8000;i<'h8100;i=i+4)pin.seed_word(native_word(i),32'h12345678);
 repeat(3)@(posedge clk);@(negedge clk);reset=0;wait(maint_ready);
 for(i=0;i<lines;i=i+1)begin
  index=shuffle?(i*37)%128:i*stride;access(1,index*16+index%16,16'h4000+index,0);
 end
 wait(maint_ready);@(negedge clk);phase=1;start=cycle;if(mode)inval=1;else clean=1;
 @(posedge clk);@(negedge clk);inval=0;clean=0;wait(maint_done);@(negedge clk);
 elapsed=cycle-start;
 if(inject)begin
  if(!maint_error||ready||maint_ready||!maint_busy)$fatal(1,"maintenance error did not fail-stop");
  i=requests;repeat(32)@(posedge clk);if(requests!=i||ready||maint_done)$fatal(1,"fault released ownership");
  $display("DCACHE_NATIVE_FAILSTOP_PASS requests=%0d terminals=%0d",requests,terms);$finish;
 end
 if(maint_error||requests!=lines||terms!=lines||(!baseline&&early==0))$fatal(1,"maintenance completion coverage req=%0d terms=%0d",requests,terms);
 // Cover every line, sparse holes and the entire equally-sized guard region.
 for(w=0;w<4096;w=w+1)begin
  index=w/16;j=w%16;
  if((j%2?(pin.mem[native_word(w*2)]>>16):(pin.mem[native_word(w*2)]&65535)) !==
    (index<128&&(shuffle||(index%stride==0&&index/stride<lines))&&j==index%16?32'h4000+index:{16'd0,seed(w)}))
   $fatal(1,"native clean data/guard mismatch index=%0d word=%0d got=%h",index,j,pin.mem[native_word(w*2)]);
 end
 phase=2;i=reads;access(0,0,0,lines>0?16'h4000:seed(0));
 if(reads-i!=mode)$fatal(1,"clean/invalidate residency contract");
 $display("DCACHE_NATIVE_PASS baseline=%0d mode=%0d lines=%0d stride=%0d shuffle=%0d cycles=%0d early=%0d chains=%0d display=%0d",baseline,mode,lines,stride,shuffle,elapsed,early,chains,display_terms);
 if(!baseline&&lines==128&&chains==0)$fatal(1,"no physical write continuation");
 if(gpu_contention&&gpu_terms==0)$fatal(1,"direction-disjoint shared class not exercised");
 $finish;
end
endmodule
