`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,in_valid=0,in_write=0,out_ready=0;
always #5 clk=!clk;
reg[21:0]in_address=0;reg[1:0]in_count=0;
wire in_ready,out_valid,out_write;wire[21:0]out_address;wire[1:0]out_count;
CpuV3GpuMemoryRequestSlot dut(.*);
integer cycle=0,sent=0,received=0,stalls=0;
reg held=0;reg[24:0]saved;
function[1:0]length;input integer i;begin case(i%3)0:length=0;1:length=1;default:length=3;endcase end endfunction
always @(negedge clk)if(!reset)begin
 in_valid=sent<16;in_address=22'h20000+sent*64;in_write=sent%2;in_count=length(sent);
 out_ready=cycle%7>2;
end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>256)$fatal(1,"slot watchdog");
 if(!reset)begin
  if(held&&{out_address,out_write,out_count}!==saved)$fatal(1,"held descriptor changed");
  if(out_valid)begin
   if(out_address!==22'h20000+received*64||out_write!==(received%2!=0)||out_count!==length(received))$fatal(1,"slot order/metadata");
   if(out_ready)received=received+1;else stalls=stalls+1;
  end
  held=out_valid&&!out_ready;saved={out_address,out_write,out_count};
  if(in_valid&&in_ready)begin
   if(out_valid)$fatal(1,"slot overwrote an outstanding descriptor");sent=sent+1;
  end
  if(received==16)begin
   if(sent!=16||stalls==0)$fatal(1,"slot coverage");
   $display("PASS slot descriptors=%0d stalls=%0d",received,stalls);$finish;
  end
 end
end
initial begin repeat(3)@(negedge clk);reset=0;end
endmodule
