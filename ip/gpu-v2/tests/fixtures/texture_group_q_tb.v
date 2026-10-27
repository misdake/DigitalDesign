`timescale 1ns/1ps
module tb;
 parameter NATIVE=1;
 reg clk=0,reset=1,ce=0,reserve_valid=0,consume=0;always #5 clk=~clk;
 GSR GSR(.GSRI(1'b1));
 reg[1:0]launched=0;reg[8:0]id0=0,id1=0;
 wire ready,valid,bad,fault;wire[34:0]payload;
 wire[4:0]pc,uw;wire[5:0]gc,rows,strobes;
 integer cycle=0,sent=0,got=0,peakp=0,peakg=0,holds=0,parallel_rw=0;
 reg[31:0]rng=32'h73a91dc7;
 function[34:0]record;input integer id;begin record=35'h123456789^(id*35'h026c4b17d);end endfunction
 gpu_v2_packet_store_unique #(.NATIVE(NATIVE)) dut(
  .clk(clk),.reset(reset),.ce(ce),.reserve_valid(reserve_valid),.reserve_owner(6'd0),
  .write_valid(launched[1]),.write_payload(record(id1)),.owner_request(1'b0),.consume(consume),
  .producer_ready(ready),.front_valid(valid),.front_payload(payload),.bad(bad),.fault(fault),
  .producer_count(pc),.unwritten_count(uw),.group_count(gc),.row_count(rows),.strobes(strobes));
 always @(negedge clk)begin
  rng=(rng<<1)^(rng[31]?32'h04c11db7:0);
  ce=!reset&&rng[3:0]!=0&&cycle%19!=7;
  reserve_valid=sent<256&&ready;
  // Hold the only native head while both independent capacity limits fill.
  consume=valid&&cycle>=400&&cycle%7!=2&&rng[7:5]!=0;
 end
 always @(posedge clk)begin
  cycle<=cycle+1;if(cycle>10000)$fatal(1,"bounded Group-Q timeout");
  if(!reset)begin
   if(bad||fault)$fatal(1,"Group-Q protocol");
   // CEB low does not make writes to the held native read address safe.
   if(dut.pw&&dut.head_valid&&dut.wptr==dut.reclaim_row)
    $fatal(1,"producer overwrote the held native Group Q row");
   if(pc>peakp)peakp<=pc;if(gc>peakg)peakg<=gc;
   if(valid&&!(ce&&consume))holds<=holds+1;
   if(ce)begin
    if(consume)begin
     if(payload!==record(got))$fatal(1,"Group-Q word order/data %0d %h",got,payload);
     got<=got+1;
    end
    if(strobes[3]&&strobes[1])parallel_rw<=parallel_rw+1;
    launched<={launched[0],reserve_valid};
    if(reserve_valid)begin id0<=sent;sent<=sent+1;end
    if(launched[0])id1<=id0;
   end
   if(got==256)begin
    if(sent!=256||launched||pc||uw||gc||rows||valid||peakp!=16||peakg!=32||holds<100||parallel_rw<50)
     $fatal(1,"Group-Q coverage/conservation P%0d G%0d hold%0d rw%0d",peakp,peakg,holds,parallel_rw);
    $display("PASS GROUP_Q native=%0d words=%0d cycles=%0d peakP=%0d peakG=%0d holds=%0d concurrentRW=%0d",NATIVE,got,cycle,peakp,peakg,holds,parallel_rw);
    $finish;
   end
  end
 end
 initial begin repeat(4)@(negedge clk);reset=0;end
endmodule
