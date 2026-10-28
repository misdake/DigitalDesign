`timescale 1ns/1fs
// Cold functional replay: production GPU/arbiter/stream MC, no CPU/Display load.
module tb;
reg clk=0,controller_clk=0,sdram_clk=0,reset=1;
wire ce=1'b1;
// Independent related clocks. No NBA-generated logic clock samples a new Q.
initial begin #4.629630;forever #9.259260 clk=~clk;end
always #4.629630 controller_clk=~controller_clk;
initial begin #4.050926;forever #4.629630 sdram_clk=~sdram_clk;end
integer cycle=0,start_cycle=0;
always @(negedge clk)cycle=cycle+1;
GSR GSR(.GSRI(1'b1));
reg wen=0;reg[3:0]channel=0;reg[15:0]control=0;
wire busy,fault,roq,ror,rov,rol,roe,frq,frr,frv,frl,fre,fwq,fwr,fwdready,fwv,fwl,fwe;
wire[21:0]roa,fra,fwa;wire[1:0]roc,frc,fwc;
wire[63:0]rod,frd,fwd;
wire mclk,cke,cs,ras,cas,we;wire[3:0]dqm;wire[10:0]addr;wire[1:0]bank;wire[31:0]dq;
gpu_v2_vliw_command gpu(.clk(clk),.reset(reset),.ce(ce),.abort(1'b0),
 .device_index(3'd4),.device_channel(channel),.device_read_enable(1'b0),
 .device_write_enable(wen),.device_write_data(control),.busy(busy),.fault(fault),.output_ready(1'b1),
 .ro_request_valid(roq),.ro_request_ready(ror),.ro_address(roa),.ro_count(roc),
 .ro_response_valid(rov),.ro_response_last(rol),.ro_response_error(roe),.ro_data(rod),
 .fb_r_request_valid(frq),.fb_r_request_ready(frr),.fb_r_address(fra),.fb_r_count(frc),
 .fb_r_response_valid(frv),.fb_r_response_last(frl),.fb_r_response_error(fre),.fb_r_data(frd),
 .fb_w_request_valid(fwq),.fb_w_request_ready(fwr),.fb_w_address(fwa),.fb_w_count(fwc),.fb_w_data(fwd),
 .fb_w_data_ready(fwdready),.fb_w_response_valid(fwv),.fb_w_response_last(fwl),.fb_w_response_error(fwe));
GowinSdramCombination mc(.logic_clk(clk),.controller_clk(controller_clk),.sdram_clk(sdram_clk),.reset(reset),
 .instruction_request_valid(1'b0),.instruction_address(22'd0),.instruction_response_ready(1'b1),
 .data_request_valid(1'b0),.data_write(1'b0),.data_line(1'b0),.data_address(22'd0),
 .data_write_data(64'd0),.data_response_ready(1'b1),
 .dma_request_valid(1'b0),.dma_write(1'b0),.dma_address(22'd0),.dma_write_data(16'd0),.dma_response_ready(1'b1),
 .display_request_valid(1'b0),.display_address(22'd0),.display_response_ready(1'b1),
 .gpu_ro_request_valid(roq),.gpu_ro_write(1'b0),.gpu_ro_address(roa),.gpu_ro_line_count_minus_1(roc),
 .gpu_ro_write_data(64'd0),.gpu_ro_request_ready(ror),.gpu_ro_response_valid(rov),
 .gpu_ro_read_data(rod),.gpu_ro_response_last(rol),.gpu_ro_error(roe),
 .gpu_fb_r_request_valid(frq),.gpu_fb_r_write(1'b0),.gpu_fb_r_address(fra),.gpu_fb_r_line_count_minus_1(frc),
 .gpu_fb_r_write_data(64'd0),.gpu_fb_r_request_ready(frr),.gpu_fb_r_response_valid(frv),
 .gpu_fb_r_read_data(frd),.gpu_fb_r_response_last(frl),.gpu_fb_r_error(fre),
 .gpu_fb_w_request_valid(fwq),.gpu_fb_w_write(1'b1),.gpu_fb_w_address(fwa),.gpu_fb_w_line_count_minus_1(fwc),
 .gpu_fb_w_write_data(fwd),.gpu_fb_w_request_ready(fwr),.gpu_fb_w_write_data_ready(fwdready),
 .gpu_fb_w_response_valid(fwv),.gpu_fb_w_response_last(fwl),.gpu_fb_w_error(fwe),
 .O_sdram_clk(mclk),.O_sdram_cke(cke),.O_sdram_cs_n(cs),.O_sdram_cas_n(cas),
 .O_sdram_ras_n(ras),.O_sdram_wen_n(we),.O_sdram_dqm(dqm),.O_sdram_addr(addr),.O_sdram_ba(bank),.IO_sdram_dq(dq));
// Published pin-model tAC/tOH plus an explicit fixed functional return delay.
// 4ns is a replay model assumption, not a measured PCB/PVT timing bound.
LabPinModel #(.CAS_LATENCY(3),.TAC(6),.TOH(2),.RETURN_MIN(4),.RETURN_MAX(4),.RCD_MIN(3),.RP_MIN(3)) pins(
 .sclk(mclk),.reset(reset),.cke(cke),.cs(cs),.ras(ras),.cas(cas),.we(we),
 .dqm(dqm),.a(addr),.ba(bank),.dq(dq));
reg[3:0]valid_bytes[0:2097151];
reg[3:0]writable_bytes[0:2097151];
function integer physical;
 input integer logical_word;
 integer remaining;
 begin
  remaining=(logical_word&31)|((logical_word>>7)<<5);
  physical=(((logical_word>>5)&3)<<19)|remaining;
 end
endfunction
integer guard_word,guard_index;
always @(posedge controller_clk)if(!reset&&(mc.bridge.take||mc.bridge.read_chain_accept)&&!mc.bridge.offer_write)begin
 for(guard_word=0;guard_word<mc.bridge.offer_words;guard_word=guard_word+1)begin
  guard_index=physical(mc.bridge.offer_address+guard_word);
  if(valid_bytes[guard_index]!==4'b1111)
   $fatal(1,"Uncaptured/uninitialized read: logical byte=%h mask=%h take=%b read_chain=%b",(mc.bridge.offer_address+guard_word)*4,valid_bytes[guard_index],mc.bridge.take,mc.bridge.read_chain_accept);
 end
end
integer write_location;
reg[7:0]last_col;
always @(posedge mclk)begin
 #0;
 if(!reset&&cke&&!cs&&pins.writing_bus)begin
  last_col=pins.col-1'b1;
  write_location={pins.burst_bank,pins.rows[pins.burst_bank],last_col};
  if((writable_bytes[write_location]&(~dqm)) !== (~dqm))
   $fatal(1,"Write outside the captured writable extent: physical=%h mask=%h",write_location,~dqm);
  if(valid_bytes[write_location]===4'bxxxx)valid_bytes[write_location]=0;
  valid_bytes[write_location]=valid_bytes[write_location]|(~dqm);
 end
end
task tick;begin @(negedge clk);#1;end endtask
task send;input[3:0]ch;input[15:0]value;begin channel=ch;control=value;wen=1;tick;wen=0;end endtask
task dump_plane;input integer base,length;input[1023:0]name;
 integer fd,offset,index;reg[31:0]word;
 begin
  fd=$fopen(name,"wb");if(!fd)$fatal(1,"Plane output open");
  for(offset=0;offset<length;offset=offset+4)begin
   index=physical((base+offset)/4);
   if(valid_bytes[index]!==4'b1111)$fatal(1,"Output contains uninitialized bytes");
   word=pins.mem[index];
   $fwrite(fd,"%c%c%c%c",word[7:0],word[15:8],word[23:16],word[31:24]);
  end
  $fclose(fd);
 end
endtask
initial begin
 $readmemh("memory.hex",pins.mem);
 $readmemh("valid.hex",valid_bytes);
 $readmemh("marked.hex",pins.marked);
 $readmemh("writable.hex",writable_bytes);
 tick;tick;reset=0;
 while(!mc.initialized)tick;
 tick;start_cycle=cycle;
 send(0,16'd@COUNT@);send(1,16'd@BASE_LINE@);
 while(busy)begin tick;if(fault)$fatal(1,"Production GPU fault");end
 repeat(3)tick;
 dump_plane(@COLOR_BASE@,@PLANE_BYTES@,"color.bin");
 dump_plane(@DEPTH_BASE@,@PLANE_BYTES@,"depth.bin");
 dump_plane(@COLOR_BASE@,@ALLOCATION_BYTES@,"color-allocation.bin");
 dump_plane(@DEPTH_BASE@,@ALLOCATION_BYTES@,"depth-allocation.bin");
 $display("REPLAY_COMPLETE cycles=%0d initialized_reads=checked",cycle-start_cycle);
 $finish;
end
// GPU_PROFILER_OBSERVER
endmodule
