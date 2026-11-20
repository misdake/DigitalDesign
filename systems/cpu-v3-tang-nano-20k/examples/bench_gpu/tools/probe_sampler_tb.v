`timescale 1ns/1ps
// Controlled warm-hit diagnostic, not a full GPU or SDRAM throughput claim.
module tb;
 reg clk=0,reset=1;
 always #5 clk=~clk;
 GSR GSR(.GSRI(1'b1));
 integer cycle=0,groups=0,pixels=0,misses=0,beat=0,mode,previous=-1;
 integer expected_interval=2,settled=0;
 integer warm_first=-1,warm_last=-1,delta2=0,delta3=0,delta_other=0;
 reg filling=0;
 wire req,ready,valid,color_ready,result,color_fault,cache_fault;
 wire[31:0]address;
 wire[71:0]payload;
 wire[15:0]t0,t1,t2,t3;
 wire[7:0]red,green,blue;
 wire[35:0]weights=mode==1?{9'd127,9'd128,9'd128,9'd128}:{9'd63,9'd64,9'd64,9'd64};
 // Each mip's four taps remain in a distinct resident sector. The producer
 // always has another group ready; no ingress, membership or R/I gaps exist.
 wire coarse=(mode==2)&&groups[0];
 wire first=(mode==1)||!coarse,last=(mode==1)||coarse;
 wire[34:0]head={4'b1111,1'b0,2'b00,4'd0,last,first,3'd2,3'd2,coarse?16'h1011:16'h1000};
 gpu_v2_texture_runtime_cache_unique cache(
  .clk(clk),.reset(reset),.proposal_ce(1'b1),.commit_allow(1'b1),.abort(1'b0),
  .texture_invalidate(1'b0),.in_valid(groups<512),.in_payload(head),
  .next_valid(groups<511),.next_key(mode==1||coarse?16'h1000:16'h1011),
  .weight_write(reset),.weight_address(7'd0),.weight_value(weights),
  .in_ready(ready),.out_ready(color_ready),.out_valid(valid),.out_payload(payload),
  .out_tex0(t0),.out_tex1(t1),.out_tex2(t2),.out_tex3(t3),
  .mem_req_valid(req),.mem_req_addr(address),.mem_req_ready(!filling),
  .mem_resp_valid(filling),.mem_resp_index(beat[3:0]),.mem_resp_data(64'hffffffffffffffff),
  .mem_resp_complete(filling&&beat==15),.mem_resp_ok(1'b1),.fault(cache_fault));
 gpu_v2_texture_runtime_color_checked color(
  .clk(clk),.reset(reset),.ce(1'b1),.proposal_ce(1'b1),
  .in_valid(valid),.in_payload(payload),.in_tex0(t0),.in_tex1(t1),.in_tex2(t2),.in_tex3(t3),
  .in_ready(color_ready),.out_ready(1'b1),.out_valid(result),
  .out_r(red),.out_g(green),.out_b(blue),.fault(color_fault));
 always@(posedge clk)begin
  cycle<=cycle+1;
  if(cycle>10000)$fatal(1,"sampler probe cycle limit");
  if(!reset)begin
   if(cache_fault||color_fault)$fatal(1,"sampler probe fault");
   if(req&&!filling)begin filling<=1;beat<=0;misses<=misses+1;end
   if(filling)begin if(beat==15)filling<=0;else beat<=beat+1;end
   if(ready)begin
    groups<=groups+1;
    if(groups>=64)begin
     if(warm_first<0)warm_first<=cycle;
     warm_last<=cycle;
     if(previous>=0)begin
      case(cycle-previous)
       2:delta2<=delta2+1;
       3:delta3<=delta3+1;
       default:delta_other<=delta_other+1;
      endcase
     end
     previous<=cycle;
    end
   end
   if(result)begin
    pixels<=pixels+1;
    if(red!=(mode==1?255:254)||green!=(mode==1?255:254)||blue!=(mode==1?255:254))
     $fatal(1,"sampler probe color data mismatch");
   end
   // Drain through public valid/result ports instead of retired internal names.
   if(groups==512 && !valid && pixels==(mode==2?256:512)) settled<=settled+1;
   else settled<=0;
   if(settled==8)begin
    if(misses!=(mode==2?2:1))$fatal(1,"unexpected steady-state miss");
    if(pixels!=(mode==2?256:512))$fatal(1,"lost color results");
    if(expected_interval==2 && (delta2!=447 || delta3!=0 || delta_other!=0))
     $fatal(1,"resident groups did not sustain the current II2 contract");
    if(expected_interval==3 && (delta3!=447 || delta2!=0 || delta_other!=0))
     $fatal(1,"resident groups did not sustain the requested legacy II3 contract");
    $display("PASS mode=%0d groups=%0d pixels=%0d misses=%0d warm_intervals=447 warm_cycles=%0d delta2=%0d delta3=%0d other=%0d",mode,groups,pixels,misses,warm_last-warm_first,delta2,delta3,delta_other);
    $finish;
   end
  end
 end
 initial begin
  if(!$value$plusargs("MODE=%d",mode)||(mode!=1&&mode!=2))$fatal(1,"explicit MODE=1 or 2 required");
  if($value$plusargs("EXPECTED_INTERVAL=%d",expected_interval))begin end
  if(expected_interval!=0 && expected_interval!=2 && expected_interval!=3)
   $fatal(1,"EXPECTED_INTERVAL must be 0 (observe), 2 or 3");
  repeat(4)@(negedge clk);reset=0;
 end
endmodule
