`timescale 1ns/1ps
// Saturated production coordinate -> coefficient -> membership -> cache -> Color.
// The upstream R/I calendar and ROP are deliberately outside this local probe.
module tb;
 reg clk=0,reset=1;always #5 clk=~clk;
 GSR GSR(.GSRI(1'b1));
 integer cycle=0,sent=0,got=0,beat=0,misses=0,mode,stress,expected_ii;
 integer first=-1,last=-1,intervals=0,bad_intervals=0,previous=-1;
 reg filling=0,ack=0;
 wire enabled=!stress||cycle%17<12;
 wire ready,cv,cr,public_free,valid,packet_ready,cache_valid,color_ready,result;
 wire cf,bf,kf,ofault;wire[37:0]owner;wire[35:0]coordinates;
 wire[34:0]packet;wire next_valid;wire[15:0]next_key;
 wire ww;wire[6:0]wa;wire[35:0]wd;wire[71:0]payload;
 wire[15:0]t0,t1,t2,t3;wire[5:0]key;wire[23:0]rgb;
 wire request;wire[31:0]address;wire[3:0]quad=(sent/4)%16;
 wire[1:0]lane=sent%4;
 wire admit=lane!=0||public_free;
 wire source_valid=sent<512&&admit;
 wire two_mips=mode==2||mode==5;
 wire[15:0]uv_u=mode>=4?16'd16384:16'd4352,uv_v=mode>=4?16'd16384:16'd4864;
 wire[7:0]lod=two_mips?8'd4:8'd0;
 wire[1:0]filter=mode==1||mode==4?2'd1:2'd2;
 gpu_v2_texture_direct_lod coordinate(
  .clk(clk),.reset(reset),.ce(enabled),.commit_allow(1'b1),
  .in_valid(source_valid),.in_ready(ready),.in_uv({uv_v,uv_u}),
  .in_quad(quad),.in_mask(4'hf),.in_lane(lane),.in_lod(lod),
  .in_max_n(4'd5),.in_has_mip(1'b1),.in_filter(filter),
  .out_valid(cv),.out_ready(cr),.out_owner(owner),.out_result(coordinates),.fault(cf));
 gpu_v2_texture_runtime_backend_unique coefficients(
  .texture_base(16'h1000),.texture_mip(1'b1),
  .clk(clk),.reset(reset),.ce(enabled),.commit_allow(1'b1),.abort(1'b0),
  .in_valid(cv),.in_owner(owner),.in_result(coordinates),.in_ready(cr),
  .packet_enable(1'b1),.out_ready(packet_ready),.out_valid(valid),.out_payload(packet),
  .next_valid(next_valid),.next_key(next_key),
  .weight_write(ww),.weight_address(wa),.weight_value(wd),.fault(bf));
 gpu_v2_texture_runtime_cache_unique cache(
  .clk(clk),.reset(reset),.proposal_ce(enabled),.commit_allow(1'b1),.abort(1'b0),
  .texture_invalidate(1'b0),.in_valid(valid),.in_payload(packet),
  .next_valid(next_valid),.next_key(next_key),
  .weight_write(ww),.weight_address(wa),.weight_value(wd),
  .in_ready(packet_ready),.out_ready(color_ready),.out_valid(cache_valid),.out_payload(payload),
  .out_tex0(t0),.out_tex1(t1),.out_tex2(t2),.out_tex3(t3),
  .mem_req_valid(request),.mem_req_addr(address),.mem_req_ready(!filling&&!ack),
  .mem_resp_valid(filling),.mem_resp_index(beat[3:0]),.mem_resp_data(64'hffffffffffffffff),
  .mem_resp_complete(ack),.mem_resp_ok(1'b1),.fault(kf));
 gpu_v2_texture_runtime_color color(
  .clk(clk),.reset(reset),.proposal_ce(enabled),.commit_allow(1'b1),
  .check_quad(quad),.public_free(public_free),
  .publish_valid(enabled&&source_valid&&ready&&lane==0),.publish_quad(quad),.publish_mask(4'hf),
  .in_valid(cache_valid),.in_payload(payload),.in_texels({t3,t2,t1,t0}),.in_ready(color_ready),
  .out_ready(1'b1),.out_valid(result),.out_key(key),.out_rgb(rgb),.fault(ofault));
 integer gold,parent,bottom,top,right,bottom_right,u,v,layer,p,answer;
 // Independent integer reference for the constant white texture, including
 // per-tap rounding. The coarse coordinates derive from the same UV contract.
 initial begin
  if(!$value$plusargs("MODE=%d",mode)||mode<1||mode>5)$fatal(1,"MODE=1..5 required");
  if(!$value$plusargs("STRESS=%d",stress))$fatal(1,"explicit STRESS required");
  if(!$value$plusargs("II=%d",expected_ii))$fatal(1,"explicit expected II required");
  answer=0;
  for(p=0;p<2;p=p+1)begin
   parent=(mode==2||mode==5)?(p==0?255:256):(p==0?511:0);
   u=(mode>=4?16384:4352)*32/256-128;v=(mode>=4?16384:4864)*32/256-128;
   if(p==1)begin u=(u-128)/2;v=(v-128)/2;end
   bottom=parent*(v%256)/256;top=parent-bottom;
   right=top*(u%256)/256;bottom_right=bottom*(u%256)/256;
   answer=answer+((top-right)*255+256)/512+(right*255+256)/512+
    ((bottom-bottom_right)*255+256)/512+(bottom_right*255+256)/512;
  end
  gold=answer>255?255:answer;
  repeat(4)@(negedge clk);reset=0;
 end
 always@(posedge clk)begin
  cycle<=cycle+1;if(cycle>30000)$fatal(1,"bounded packed pipeline timeout");
  if(!reset)begin
   if(cf||bf||kf||ofault)$fatal(1,"packed pipeline fault");
   ack<=0;
   if(request&&!filling&&!ack)begin filling<=1;beat<=0;misses<=misses+1;end
   if(filling)begin if(beat==15)begin filling<=0;ack<=1;end else beat<=beat+1;end
   if(enabled&&source_valid&&ready)sent<=sent+1;
   if(enabled&&result)begin
    if(key!==got%64||rgb!=={3{gold[7:0]}})$fatal(1,"packed result owner/color %0d %h %h gold%0d",got,key,rgb,gold);
    if(got>=64)begin
     if(first<0)first<=cycle;last<=cycle;
     if(previous>=0)begin intervals<=intervals+1;if(cycle-previous!=expected_ii)bad_intervals<=bad_intervals+1;end
     previous<=cycle;
    end
    got<=got+1;
   end
   if(got==512)begin
    if(sent!=512||misses!=(mode==5?5:mode==4?4:mode==2?2:1)||(!stress&&bad_intervals))$fatal(1,"pipeline conservation/cadence");
    $display("PASS PACKED mode=%0d stress=%0d pixels=%0d warm_intervals=%0d warm_cycles=%0d II=%0d misses=%0d",mode,stress,got,intervals,last-first,expected_ii,misses);
    $finish;
   end
  end
 end
endmodule
