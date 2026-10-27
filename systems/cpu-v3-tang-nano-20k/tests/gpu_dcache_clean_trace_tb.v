`timescale 1ns/1ps
// Observe the existing D-cache; no speculative request or RTL change is made.
module tb;
reg clk=0,reset=1;always #5 clk=~clk;
GSR GSR(.GSRI(1'b1));
reg clean=0,q=0;reg[31:0]address=0;reg[15:0]data=0;
wire ready,valid,error,mq,mw,ml,mrr,maintenance_ready,maintenance_busy,maintenance_done,maintenance_error,sweep;
wire[15:0]read_data;wire[21:0]ma;wire[63:0]mwd;
integer state=0,beat=0,delay_left=0,terminal_delay=0,lines=128,stride=1;
wire mr=state==0,mwr=state==2,mv=state==1||state==4;
__CACHE__ cache(.clk(clk),.reset(reset),.clean_all(clean),.invalidate_all(1'b0),
 .line_copy_start(1'b0),.line_copy_source(22'd0),.line_copy_destination_page(8'd0),
 .line_clean_start(1'b0),.line_clean_address(22'd0),
 .cpu_request_valid(q),.cpu_write(1'b1),.cpu_address(address),.cpu_write_data(data),.cpu_response_ready(1'b1),
 .cpu_request_ready(ready),.cpu_response_valid(valid),.cpu_read_data(read_data),.cpu_error(error),
 .memory_request_ready(mr),.memory_write_data_ready(mwr),.memory_response_valid(mv),.memory_read_data(64'd0),.memory_error(1'b0),
 .memory_request_valid(mq),.memory_write(mw),.memory_line(ml),.memory_address(ma),.memory_write_data(mwd),.memory_response_ready(mrr),
 .maintenance_ready(maintenance_ready),.maintenance_busy(maintenance_busy),.maintenance_done(maintenance_done),.maintenance_error(maintenance_error),.valid_sweep(sweep));
integer cycle=0,phase=0,start_cycle=0,requests=0,terminals=0;
integer last_terminal=-1,last_data=-1,early_offers=0,min_gap=99999,max_gap=0;
always @(posedge clk)begin
 cycle<=cycle+1;
 if(cycle>200000)$fatal(1,"cache trace watchdog");
 if(!reset)begin
  if(error||maintenance_error)$fatal(1,"cache trace error");
  if(phase==1&&mq&&state!=0)early_offers=early_offers+1;
  if(mq&&mr)begin
   beat<=0;state<=mw?2:1;
   if(phase==1)begin
    if(!mw)$fatal(1,"clean issued a read");
    if(last_terminal>=0)begin
     if(cycle-last_terminal<min_gap)min_gap=cycle-last_terminal;
     if(cycle-last_terminal>max_gap)max_gap=cycle-last_terminal;
    end
    $display("CLEAN_REQUEST index=%0d cycle=%0d address=%h since_terminal=%0d since_data=%0d",requests,cycle,ma,last_terminal<0?-1:cycle-last_terminal,last_data<0?-1:cycle-last_data);
    requests=requests+1;
   end
  end
  case(state)
   1:if(mrr)begin if(beat==3)state<=0;else beat<=beat+1;end
   2:if(mwr)begin
    if(beat==3)begin delay_left<=terminal_delay;state<=3;if(phase==1)last_data=cycle;end
    else beat<=beat+1;
   end
   3:if(delay_left==0)state<=4;else delay_left<=delay_left-1;
   4:if(mrr)begin state<=0;if(phase==1)begin last_terminal=cycle;terminals=terminals+1;$display("CLEAN_TERMINAL cycle=%0d",cycle);end end
  endcase
 end
end
integer i,ignored;
initial begin
 ignored=$value$plusargs("DELAY=%d",terminal_delay);
 ignored=$value$plusargs("LINES=%d",lines);
 ignored=$value$plusargs("STRIDE=%d",stride);
 if(lines<1||lines>128||stride<1||stride>8)$fatal(1,"trace configuration");
 repeat(3)@(posedge clk);@(negedge clk);reset=0;
 wait(maintenance_ready);
 for(i=0;i<lines;i=i+1)begin
  @(negedge clk);wait(ready);address=i*stride*16;data=i+1;q=1;
  @(posedge clk);@(negedge clk);q=0;
  wait(valid);@(posedge clk);@(negedge clk);
 end
 wait(maintenance_ready);@(negedge clk);phase=1;start_cycle=cycle;clean=1;
 @(posedge clk);@(negedge clk);clean=0;
 wait(maintenance_done);@(negedge clk);
 if(requests!=lines||terminals!=lines||early_offers!=0)$fatal(1,"trace coverage");
 $display("CACHE_TRACE_PASS lines=%0d stride=%0d terminal_delay=%0d cycles=%0d request_gap_min=%0d request_gap_max=%0d early_offers=%0d",lines,stride,terminal_delay,cycle-start_cycle,min_gap,max_gap,early_offers);
 $finish;
end
endmodule
