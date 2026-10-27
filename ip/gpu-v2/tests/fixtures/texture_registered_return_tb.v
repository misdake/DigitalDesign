`timescale 1ns/1ps
module tb;
 reg clk=0,reset=1,initializing=1;
 always #5 clk=~clk;
 GSR GSR(.GSRI(1'b1));
 parameter NATIVE=1;
 integer cycle=0,groups=0,pixels=0,mode,stress,beat=0,delay=0,misses=0;
 integer previous=-1,intervals=0,bad_intervals=0,overlap=0,paused_returns=0,held=0;
 reg[6:0]init_address=0;
 reg filling=0,ack=0;
 reg[15:0]fill_sector;
 wire enabled=!stress||cycle%11<7;
 wire consumer_enabled=!stress||cycle%31<19;
 wire ready,valid,color_ready,result,cache_fault,color_fault,request;
 wire[31:0]address;
 wire[71:0]payload;
 wire[15:0]t0,t1,t2,t3;
 wire[7:0]red,green,blue;
 wire coarse=mode==2&&groups[0];
 wire[5:0]input_key=mode==2?groups/2:groups;
 wire[15:0]sector=16'h1000+(stress&&groups>=128?((groups/2)%8)*16:0)+(coarse?1:0);
 wire[34:0]head={4'b1111,1'b0,input_key[1:0],input_key[5:2],mode==1||coarse,mode==1||!coarse,3'd2,3'd2,sector};
 wire next_coarse=mode==2&&!groups[0];
 wire[15:0]next_sector=16'h1000+(stress&&groups+1>=128?(((groups+1)/2)%8)*16:0)+(next_coarse?1:0);
 wire[8:0]weight_delta={7'd0,init_address[2:1]};
 wire[8:0]special_weight=(mode==1?9'd127:9'd63)+weight_delta;
 wire[8:0]regular_weight=(mode==1?9'd128:9'd64)-weight_delta;
 wire[35:0]weights={special_weight,regular_weight,regular_weight,regular_weight};
 function[15:0]texel;input[15:0]s;begin
  texel={(s[7:4]^5'b10101),6'd41,(s[4:0]^5'b01011)};
 end endfunction
 wire response=filling&&delay==0&&(!stress||cycle%7!=0);
 wire[15:0]fill_color=texel(fill_sector);
 gpu_v2_texture_runtime_cache_unique #(.NATIVE(NATIVE)) cache(
  .clk(clk),.reset(reset),.proposal_ce(enabled),.commit_allow(1'b1),.abort(1'b0),
  .texture_invalidate(1'b0),.in_valid(!initializing&&groups<512),.in_payload(head),
  .next_valid(groups<511),.next_key(next_sector),
  .weight_write(initializing&&!reset),.weight_address(init_address),.weight_value(weights),
  .in_ready(ready),.out_ready(color_ready),.out_valid(valid),.out_payload(payload),
  .out_tex0(t0),.out_tex1(t1),.out_tex2(t2),.out_tex3(t3),
  .mem_req_valid(request),.mem_req_addr(address),.mem_req_ready(!filling&&!ack&&(!stress||cycle%5!=0)),
  .mem_resp_valid(response),.mem_resp_index(beat[3:0]),.mem_resp_data({4{fill_color}}),
  .mem_resp_complete(ack),.mem_resp_ok(1'b1),.fault(cache_fault));
 wire[5:0]result_key;
 gpu_v2_texture_runtime_color_checked color(
  .clk(clk),.reset(reset),.ce(enabled),.proposal_ce(enabled&&consumer_enabled),
  .in_valid(valid),.in_payload(payload),.in_tex0(t0),.in_tex1(t1),.in_tex2(t2),.in_tex3(t3),
  .in_ready(color_ready),.out_ready(1'b1),.out_valid(result),.out_key(result_key),
  .out_r(red),.out_g(green),.out_b(blue),.fault(color_fault));
 function integer channel;input[15:0]s;input integer c;reg[15:0]v;begin
  v=texel(s);
  case(c)
   0:channel={v[15:11],v[15:13]};
   1:channel={v[10:5],v[10:9]};
   2:channel={v[4:0],v[4:2]};
  endcase
 end endfunction
 function integer weighted;input integer c,w0,w1;begin
  weighted=((w0*c+256)>>9)+3*((w1*c+256)>>9);
 end endfunction
 function integer expected;input integer p,c;integer base;begin
  base=16'h1000+(stress&&p>=64?(p%8)*16:0);
  if(mode==1)begin
   // One group per pixel uses the exact producer group/2 sector mapping.
   base=16'h1000+(stress&&p>=128?((p/2)%8)*16:0);
   expected=weighted(channel(base,c),127+(p%4),128-(p%4));
  end else expected=weighted(channel(base,c),63+(p%4),64-(p%4))+weighted(channel(base+1,c),63+(p%4),64-(p%4));
 end endfunction
 reg hold_valid=0;
 reg[135:0]held_result;
 always@(posedge clk)begin
  cycle<=cycle+1;
  if(cycle>30000)$fatal(1,"registered-return cycle limit");
  if(!reset)begin
   if(cache_fault||color_fault)$fatal(1,"registered-return fault");
   if(initializing)begin
    if(init_address==127)initializing<=0;else init_address<=init_address+1'b1;
   end
   ack<=0;
   if(request&&cache.mem_req_ready)begin
    if(filling)$fatal(1,"two memory fills");
    filling<=1;beat<=0;delay<=stress?3:0;fill_sector<=address[22:7];misses<=misses+1;
   end
   if(filling&&delay!=0)delay<=delay-1;
   if(response)begin if(beat==15)begin filling<=0;ack<=1;end else beat<=beat+1;end
   if(response&&!enabled)paused_returns<=paused_returns+1;
   if(cache.rd_valid&&cache.res_valid&&!consumer_enabled)overlap<=overlap+1;
   if(cache.return_capture&&cache.data_write&&cache.pin_line==cache.desc_line)
    $fatal(1,"native texel-bank read/refill line collision");
   if(cache.return_capture&&cache.res_valid&&!cache.result_pop)$fatal(1,"overwrote unconsumed native Q");
   if(hold_valid&&held_result!=={payload,t3,t2,t1,t0})$fatal(1,"held native Q or metadata changed");
   hold_valid<=valid&&!cache.result_pop;
   held_result<={payload,t3,t2,t1,t0};
   if(hold_valid)held<=held+1;
   if(ready)begin
    groups<=groups+1;
    if(!stress&&groups>=64)begin
     if(previous>=0)begin intervals<=intervals+1;if(cycle-previous!=2)bad_intervals<=bad_intervals+1;end
     previous<=cycle;
    end
   end
   if(enabled&&result)begin
    if(result_key!==(pixels%64)||red!==expected(pixels,0)||green!==expected(pixels,1)||blue!==expected(pixels,2))
     $fatal(1,"result %0d key=%0d rgb=%0d/%0d/%0d expected=%0d/%0d/%0d",pixels,result_key,red,green,blue,expected(pixels,0),expected(pixels,1),expected(pixels,2));
    pixels<=pixels+1;
   end
   if(groups==512&&!valid&&!cache.rd_valid&&!color.q_done&&!color.done)begin
    if(pixels!=(mode==2?256:512))$fatal(1,"missing results");
    if(!stress&&(bad_intervals!=0||intervals!=447||misses!=(mode==2?2:1)))$fatal(1,"hit cadence or refill mismatch");
    if(stress&&(overlap==0||paused_returns==0||held==0||misses<16))$fatal(1,"stress coverage incomplete");
    $display("PASS native=%0d mode=%0d stress=%0d groups=%0d pixels=%0d misses=%0d II2=%0d overlap=%0d paused_returns=%0d held=%0d",NATIVE,mode,stress,groups,pixels,misses,intervals,overlap,paused_returns,held);
    $finish;
   end
  end
 end
 initial begin
  if(!$value$plusargs("MODE=%d",mode)||!$value$plusargs("STRESS=%d",stress))$fatal(1,"explicit MODE and STRESS required");
  repeat(4)@(negedge clk);reset=0;
 end
endmodule
