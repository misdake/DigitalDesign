`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,ce=1,allow=1,abort=0,valid=0,ordy=0,mready=0,mvalid=0,complete=0,ok=1,ww=0;
always #5 clk=~clk;
wire ready,ov,mreq,drained,fault,bad;wire[71:0]op;wire[15:0]t0,t1,t2,t3;wire[31:0]addr;
reg[3:0]index=0;reg[63:0]data=0;
reg[34:0]payload={4'hf,1'b0,8'd0,3'd0,4'd0,15'h80};
integer failed,sector,i,way,cycles=0,pubs=0,reads=0,waits;reg[135:0]held;
GSR GSR(.GSRI(1'b1));
function[15:0]value;input integer a;begin value=((a>>1)*32'h17e9)^(a>>6)^16'ha35c;end endfunction
gpu_v2_texture_runtime_cache_unique #(.NATIVE(@NATIVE@)) dut(.clk(clk),.reset(reset),.proposal_ce(ce),.commit_allow(allow),.abort(abort),
.in_valid(valid),.in_payload(payload),.texture_invalidate(1'b0),.weight_write(ww),.weight_address(7'd0),.weight_value({4{9'd64}}),
.next_valid(1'b0),.next_key(15'd0),.in_ready(ready),.out_ready(ordy),.out_valid(ov),.out_payload(op),
.out_tex0(t0),.out_tex1(t1),.out_tex2(t2),.out_tex3(t3),.mem_req_valid(mreq),.mem_req_addr(addr),.mem_req_ready(mready),
.mem_resp_valid(mvalid),.mem_resp_index(index),.mem_resp_data(data),.mem_resp_complete(complete),.mem_resp_ok(ok),
.drained(drained),.fault(fault),.preflight_bad(bad));
always @(posedge clk)begin
 cycles=cycles+1;if(cycles>300)$fatal(1,"terminal cycle bound");
 if(!reset)begin if(dut.publish)pubs=pubs+1;if(ready)reads=reads+1;end
 #1;if(!reset)$display("TRACE %0d %h %h %h %h %h %h %h %h %h %h %h %h",cycles,ce,dut.desc_valid,dut.desc_sector,dut.desc_started,dut.desc_next,drained,dut.state_rows[0],ready,ov,fault,bad,{t3,t2,t1,t0});
end
initial begin
 if(!$value$plusargs("FAIL=%d",failed))failed=0;
 repeat(3)@(negedge clk);reset=0;ww=1;
 @(negedge clk);ww=0;valid=1;
 for(sector=0;sector<2;sector=sector+1)begin
  waits=0;while(!mreq)begin @(negedge clk);waits=waits+1;if(waits>20)$fatal(1,"request wait");end
  if(addr!==(32'h8000+sector*128))$fatal(1,"request address");
  way=dut.desc_line[1:0];mready=1;
  @(negedge clk);mready=0;ce=0;
  for(i=0;i<16;i=i+1)begin
   mvalid=1;index=i;complete=i==15;ok=!(complete&&failed==sector+1);
   data={value(32'h8000+sector*128+i*8+6),value(32'h8000+sector*128+i*8+4),value(32'h8000+sector*128+i*8+2),value(32'h8000+sector*128+i*8)};
   @(negedge clk);
  end
  mvalid=0;complete=0;ok=1;
  if(failed==sector+1)begin
   if(dut.state_rows[0][way*2+:2]!==0||!drained||dut.desc_valid||pubs||reads||ov||fault||bad)$fatal(1,"failed parent invalidation/drain");
   repeat(4)@(negedge clk);
   if(pubs||reads||ov||!drained)$fatal(1,"failed parent published while paused");
   $display("PASS terminal fail=%0d cycles=%0d publications=%0d consumes=%0d drained=%b",failed,cycles,pubs,reads,drained);$finish;
  end
  if(sector==0)begin
   if(dut.state_rows[0][way*2+:2]!==1||!dut.desc_valid||!dut.desc_sector||pubs||reads||ov)$fatal(1,"child0 published prematurely");
  end else if(dut.state_rows[0][way*2+:2]!==2||!drained||pubs!=1||reads||ov)$fatal(1,"parent success boundary");
 end
 ce=1;
 waits=0;while(reads==0)begin @(negedge clk);waits=waits+1;if(waits>20)$fatal(1,"consume wait");end
 valid=0;
 waits=0;while(!ov)begin @(negedge clk);waits=waits+1;if(waits>20)$fatal(1,"result wait");end
 if({t3,t2,t1,t0}!=={value(32'h8022),value(32'h8020),value(32'h8002),value(32'h8000)})$fatal(1,"independent texel address oracle");
 held={op,t3,t2,t1,t0};ce=0;
 repeat(4)begin @(negedge clk);if(!ov||{op,t3,t2,t1,t0}!==held)$fatal(1,"held Q changed");end
 ce=1;ordy=1;
 @(negedge clk);if(ov||pubs!=1||reads!=1||fault||bad)$fatal(1,"retirement counts");
 $display("PASS terminal success cycles=%0d publications=%0d consumes=%0d",cycles,pubs,reads);$finish;
end
initial begin #4000;$fatal(1,"terminal wall bound");end
endmodule
