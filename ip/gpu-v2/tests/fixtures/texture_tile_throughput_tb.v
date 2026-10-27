`timescale 1ns/1ps
// Saturated production coordinate -> coefficient -> membership -> cache -> Color.
// The upstream R/I calendar and ROP are deliberately outside this local probe.
module tb;
 reg clk=0,reset=1;always #5 clk=~clk;
 GSR GSR(.GSRI(1'b1));
 integer profile=0,pace=0,filter_arg=1,mip_arg=0,ii_x2=0;
 integer groups=0,members=0,coarse=0,backlog_peak=0,group_peak=0,warm_group_peak=0,offered;
 integer first_in=-1,last_in=-1,first_out=-1,last_out=-1;
 integer qlat,qlat_min=999999,qlat_max=0,lambda_min=999999,lambda_max=0;
 integer cycle=0,sent=0,got=0,beat=0,misses=0,mode,stress,expected_ii;
 integer first=-1,last=-1,intervals=0,bad_intervals=0,previous=-1;
 reg filling=0,ack=0;reg[31:0]fill_address;
 wire enabled=!stress||cycle%17<12;
 wire ready,cv,cr,public_free,valid,packet_ready,cache_valid,color_ready,result;
 wire cf,bf,kf,ofault;wire[37:0]owner;wire[35:0]coordinates;
 wire[34:0]packet;wire next_valid;wire[15:0]next_key;
 wire ww;wire[6:0]wa;wire[35:0]wd;wire[71:0]payload;
 wire[15:0]t0,t1,t2,t3;wire[5:0]key;wire[23:0]rgb;
 wire request;wire[31:0]address;wire[3:0]quad=(sent/4)%16;
 wire[1:0]lane=sent%4;
 wire admit=lane!=0||public_free;
 wire source_valid=sent<512&&admit&&cycle>=200+(pace==16?16*(sent/4):pace*sent);
 wire two_mips=mip_arg!=0;
 reg[15:0]uv_u=0,uv_v=0;
 function[15:0]uvalue;input integer i;begin
  uvalue=profile==5?32768:(profile==1||profile==4||((profile==2||profile==3)&&i%4==3))?16384:4352;
 end endfunction
 function[15:0]vvalue;input integer i;begin
  vvalue=profile==5?32768:(profile==4||(profile==3&&i%4==3))?16384:4864;
 end endfunction
 // Emulate the producer RAM Q: reload only before U; keep it through V.
 always @(negedge clk)if(!reset&&ready)begin uv_u<=uvalue(sent);uv_v<=vvalue(sent);end
 wire[7:0]lod=two_mips?8'd4:8'd0;
 wire[1:0]filter=filter_arg;
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
  .mem_resp_valid(filling),.mem_resp_index(beat[3:0]),.mem_resp_data({4{tile_code(fill_address/128)}}),
  .mem_resp_complete(ack),.mem_resp_ok(1'b1),.fault(kf));
 gpu_v2_texture_runtime_color color(
  .clk(clk),.reset(reset),.proposal_ce(enabled),.commit_allow(1'b1),
  .check_quad(quad),.public_free(public_free),
  .publish_valid(enabled&&source_valid&&ready&&lane==0),.publish_quad(quad),.publish_mask(4'hf),
  .in_valid(cache_valid),.in_payload(payload),.in_texels({t3,t2,t1,t0}),.in_ready(color_ready),
  .out_ready(1'b1),.out_valid(result),.out_key(key),.out_rgb(rgb),.fault(ofault));
 function[15:0]tile_code;input integer sector;begin tile_code=(sector*32'h17e9)^(sector>>1)^16'ha35c;end endfunction
 function integer channel;input[15:0]code;input integer c;begin
  case(c)0:channel=(code[15:11]<<3)|(code[15:11]>>2);1:channel=(code[10:5]<<2)|(code[10:5]>>4);default:channel=(code[4:0]<<3)|(code[4:0]>>2);endcase
 end endfunction
 // Independent integer texture addresses, mip selection and F9 tap rounding.
 function[23:0]gold_value;input integer i;
 integer u,v,fu,fv,bottom,top,right,br,c,k,layer,parent,sector,total,w;reg[23:0]answer;
 begin
  answer=0;
  for(c=0;c<3;c=c+1)begin
   total=0;
   for(layer=0;layer<(two_mips?2:1);layer=layer+1)begin
    u=uvalue(i)*32/256-128;v=vvalue(i)*32/256-128;
    if(layer)begin u=(u-128)/2;v=(v-128)/2;end
    fu=u%256;fv=v%256;parent=two_mips?(layer?256:255):511;
    bottom=parent*fv/256;top=parent-bottom;right=top*fu/256;br=bottom*fu/256;
    for(k=0;k<4;k=k+1)begin
     sector=4096+(layer?4:8)+((v/256+k/2)/8)*(layer?2:4)+(u/256+k%2)/8;
     case(k)0:w=top-right;1:w=right;2:w=bottom-br;default:w=br;endcase
     total=total+(w*channel(tile_code(sector),c)+256)/512;
    end
   end
   if(total>255)total=255;answer=answer|(total<<(8*c));
  end
  gold_value=answer;
 end endfunction
 wire[23:0]gold=gold_value(got);
 initial begin
  mode=3;stress=0;expected_ii=0;
  if(!$value$plusargs("PROFILE=%d",profile)||!$value$plusargs("PACE=%d",pace)||
     !$value$plusargs("FILTER=%d",filter_arg)||!$value$plusargs("MIPS=%d",mip_arg)||
     !$value$plusargs("II_X2=%d",ii_x2))$fatal(1,"explicit scenario required");
  repeat(4)@(negedge clk);reset=0;
 end
 always@(posedge clk)begin
  cycle<=cycle+1;if(cycle>30000)$fatal(1,"bounded packed pipeline timeout");
  if(!reset)begin
   if(cf||bf||kf||ofault)$fatal(1,"packed pipeline fault");
   if(enabled&&cv&&cr)begin
    if(owner[17:9]!=(two_mips?256:0)||owner[8:0]!=(two_mips?255:511))$fatal(1,"actual mip weights");
    if(owner[17:9]<lambda_min)lambda_min=owner[17:9];if(owner[17:9]>lambda_max)lambda_max=owner[17:9];
   end
   if(cache.read_issue)groups<=groups+1;
   if(coefficients.row_count>group_peak)group_peak=coefficients.row_count;
   if(got>=256&&got<=508&&coefficients.row_count>warm_group_peak)warm_group_peak=coefficients.row_count;
   if(coefficients.member_issue)begin members<=members+1;if(coefficients.selected_coarse)coarse<=coarse+1;end
   offered=cycle<200?0:pace==16?4*((cycle-200)/16+1):pace?(cycle-200)/pace+1:sent;
   if(offered>512)offered=512;
   if(offered-sent>backlog_peak)backlog_peak<=offered-sent;
   ack<=0;
   if(request&&!filling&&!ack)begin filling<=1;fill_address<=address;beat<=0;misses<=misses+1;end
   if(filling)begin if(beat==15)begin filling<=0;ack<=1;end else beat<=beat+1;end
   if(enabled&&source_valid&&ready)begin sent<=sent+1;if(first_in<0)first_in<=cycle;last_in<=cycle;end
   if(enabled&&result)begin
    if(first_out<0)first_out<=cycle;last_out<=cycle;
    if(pace==16&&got>=256&&got%4==3)begin qlat=cycle-(200+16*(got/4));if(qlat<qlat_min)qlat_min=qlat;if(qlat>qlat_max)qlat_max=qlat;end
    if(key!==got%64||rgb!==gold)$fatal(1,"packed result owner/color %0d %h %h gold%0d",got,key,rgb,gold);
    if(got>=256&&got<=508)begin
     if(first<0)first<=cycle;last<=cycle;
     if(previous>=0)begin intervals<=intervals+1;if(cycle-previous!=expected_ii)bad_intervals<=bad_intervals+1;end
     previous<=cycle;
    end
    got<=got+1;
   end
   if(got==512)begin
    if(sent!=512||coarse!=(two_mips?512:0)||members!=(two_mips?1024:512)||
       (last-first)*2!=intervals*ii_x2)$fatal(1,"tile throughput conservation/cadence");
    $display("PASS TILE profile=%0d filter=%0d mips=%0d pace=%0d pixels=%0d groups=%0d members=%0d coarse=%0d lambda_min=%0d lambda_max=%0d misses=%0d warm_intervals=%0d warm_cycles=%0d backlog_peak=%0d group_peak=%0d warm_group_peak=%0d first_latency=%0d drain=%0d total=%0d quad_latency_min=%0d quad_latency_max=%0d",profile,filter_arg,mip_arg,pace,got,groups,members,coarse,lambda_min,lambda_max,misses,intervals,last-first,backlog_peak,group_peak,warm_group_peak,first_out-first_in,last_out-last_in,last_out-first_in+1,qlat_min,qlat_max);
    $finish;
   end
  end
 end
endmodule
