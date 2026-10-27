`timescale 1ns/1ps
// Real Group/cache boundary with a fixed16-beat memory peer. Not MC timing.
module tb;
 parameter NATIVE=1;
 reg clk=0,reset=1;always #5 clk=~clk;
 GSR GSR(.GSRI(1'b1));
 integer cycle=0,sent=0,got=0,misses=0,beat=0,thrash=0,first=-1,last=-1;
 reg filling=0,ack=0;reg[1:0]launched=0;reg[7:0]id0=0,id1=0;
 wire source_ready,head_valid,consume,next_valid,fault_q,fault_c,output_valid,request;
 wire[34:0]packet;wire[15:0]next_key;wire[71:0]result;wire[15:0]t0,t1,t2,t3;
 wire[31:0]addr;
 wire[6:0]weight_address=cycle-8;
 wire issue=!reset&&cycle>=140&&sent<96&&source_ready;
 function[34:0]item;input[7:0]id;reg[15:0]sector;begin
  sector=16'h80+(thrash?id*16:0);item={4'hf,1'b0,id,3'd0,3'd0,sector};
 end endfunction
 gpu_v2_packet_store_unique #(.NATIVE(NATIVE)) queue(
  .clk(clk),.reset(reset),.ce(1'b1),.reserve_valid(issue),.reserve_owner(6'd0),
  .write_valid(launched[1]),.write_payload(item(id1)),.owner_request(1'b0),.consume(consume),
  .producer_ready(source_ready),.front_valid(head_valid),.front_payload(packet),
  .next_valid(next_valid),.next_key(next_key),.fault(fault_q));
 gpu_v2_texture_runtime_cache_unique #(.NATIVE(NATIVE)) cache(
  .clk(clk),.reset(reset),.proposal_ce(1'b1),.commit_allow(1'b1),.abort(1'b0),
  .texture_invalidate(1'b0),.in_valid(head_valid),.in_payload(packet),
  .next_valid(next_valid),.next_key(next_key),
  .weight_write(!reset&&cycle>=8&&cycle<136),.weight_address(weight_address),.weight_value({4{9'd64}}),
  .in_ready(consume),.out_ready(1'b1),.out_valid(output_valid),.out_payload(result),
  .out_tex0(t0),.out_tex1(t1),.out_tex2(t2),.out_tex3(t3),
  .mem_req_valid(request),.mem_req_addr(addr),.mem_req_ready(!filling&&!ack),
  .mem_resp_valid(filling),.mem_resp_index(beat[3:0]),.mem_resp_data(64'hffffffffffffffff),
  .mem_resp_complete(ack),.mem_resp_ok(1'b1),.fault(fault_c));
 always @(posedge clk)begin
  cycle<=cycle+1;if(cycle>10000)$fatal(1,"bounded Group lookahead timeout");
  if(!reset)begin
   if(fault_q||fault_c)$fatal(1,"Group lookahead protocol");
   launched<={launched[0],issue};if(issue)begin id0<=sent;sent<=sent+1;end
   if(launched[0])id1<=id0;
   ack<=0;
   if(request&&!filling&&!ack)begin
    if(addr!==((16'h80+(thrash?misses*16:0))*128))$fatal(1,"refill address/order");
    filling<=1;beat<=0;misses<=misses+1;
   end
   if(filling)begin if(beat==15)begin filling<=0;ack<=1;end else beat<=beat+1;end
   if(output_valid)begin
    if(result[71:64]!==got[7:0]||{t3,t2,t1,t0}!==64'hffffffffffffffff)
     $fatal(1,"Group lookahead owner/texel mismatch");
    if(first<0)first<=cycle;last<=cycle;got<=got+1;
   end
   if(got==96)begin
    if(sent!=96||misses!=(thrash?96:1)||last-first!=95*(thrash?20:2))
     $fatal(1,"Group lookahead conservation/cadence");
    $display("PASS LOOKAHEAD native=%0d thrash=%0d groups=%0d misses=%0d intervals=%0d",NATIVE,thrash,got,misses,last-first);
    $finish;
   end
  end
 end
 initial begin
  if(!$value$plusargs("THRASH=%d",thrash))$fatal(1,"explicit THRASH required");
  repeat(4)@(negedge clk);reset=0;
 end
endmodule
