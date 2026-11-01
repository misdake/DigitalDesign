`timescale 1ns/1ps
module tb;
 parameter NATIVE=0;
 reg clk=0,reset=1;always #5 clk=~clk;GSR GSR(.GSRI(1'b1));
 reg mvalid=0;reg[3:0]level=0;reg[35:0]weights=1;
 reg pvalid=0,fb=0;reg[15:0]base=0;reg[75:0]pmember=76'h1000000000;
 reg reserve=0,write_valid=0,consume=0;
 membership_leaf #(.SHORT_ALIGNMENT(1)) membership(.clk(clk),.reset(reset),.ce(1'b1),.in_valid(mvalid),
  .in_w0(weights[8:0]),.in_w1(weights[17:9]),.in_w2(weights[26:18]),.in_w3(weights[35:27]),
  .in_c0(10'd0),.in_c1(10'd0),.in_c2(10'd0),.in_c3(10'd0),.in_slot(4'd0),.in_level(level),.in_key(6'd0),.in_fine(1'b1),.in_last_fine(1'b1));
 gpu_v2_texture_work_unique work(.clk(clk),.reset(reset),.ce(1'b1),.in_valid(1'b0),.in_member(76'd0),.out_ready(1'b0));
 packet_leaf_unique packet(.clk(clk),.reset(reset),.ce(1'b1),.in_valid(pvalid),.in_member(pmember),.in_tap(2'd0),
  .texture_base(base),.texture_mip(1'b0),.texture_fb400240(fb));
 gpu_v2_packet_store_unique #(.NATIVE(0)) group_dut(.clk(clk),.reset(reset),.ce(1'b1),.reserve_valid(reserve),.reserve_owner(6'd0),
  .write_valid(write_valid),.write_payload(35'd0),.owner_request(1'b0),.consume(consume));
 gpu_v2_runtime_packet_backend_unique packets(.clk(clk),.reset(reset),.proposal_ce(1'b1),.commit_allow(1'b0),.abort(1'b0),
  .issue_enable(1'b1),.in_valid(1'b0),.in_member(76'd0),.out_ready(1'b0),.texture_base(16'd0),.texture_mip(1'b0),.texture_fb400240(1'b0));
 defparam packets.u_pool.NATIVE=0;
 gpu_v2_texture_runtime_backend_unique runtime_dut(.clk(clk),.reset(reset),.ce(1'b1),.commit_allow(1'b0),.abort(1'b0),
  .in_valid(1'b0),.in_owner(38'd0),.in_result(36'd0),.packet_enable(1'b0),.out_ready(1'b0),
  .texture_base(16'd0),.texture_mip(1'b0),.texture_fb400240(1'b0));
 defparam runtime_dut.packets.u_pool.NATIVE=0;
 initial begin
  repeat(3)@(negedge clk);reset=0;
  @INJECT@
  @(posedge clk);#1;
  if({membership.fault,work.fault,packet.fault,group_dut.fault,packets.terminal,runtime_dut.stopped}!==6'd0)$fatal(1,"product diagnostic lifetime changed");
  $display("PASS RELEASE case=@CASE@ time=%0t states=%h",$time,{work.count,work.fetch_ptr,group_dut.used,group_dut.wptr,packets.owner_e1,runtime_dut.reserved});$finish;
 end
 initial begin #1000;$fatal(1,"negative watchdog");end
endmodule
