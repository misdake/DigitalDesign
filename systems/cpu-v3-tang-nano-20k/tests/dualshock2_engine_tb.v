module tb;
reg clk=0,reset=1,rd=0,wr=0,dat=1;
always #5 clk=~clk;
reg [2:0] device=5;
reg [3:0] channel=0;
reg [15:0] value=0;
wire [15:0] result;
wire sclk,cmd,att;
__DS2__ dut(.clk(clk),.reset(reset),.device_index(device),.device_channel(channel),
 .device_read_enable(rd),.device_write_enable(wr),.device_write_data(value),
 .dat(dat),.device_read_data(result),.ds2_clk(sclk),.cmd(cmd),.att(att));
integer cycles=0,falls=0,rises=0,last_fall=0,last_rise=0,selected_at=0;
reg checking=0;
reg [7:0] tx=0,reply=0;
always @(posedge clk)begin cycles=cycles+1;if(cycles>8000000)$fatal(1,"bounded engine");end
always @(negedge att)begin selected_at=cycles;falls=0;rises=0;end
always @(negedge sclk)if(checking)begin
 if(att || cycles-selected_at<216)$fatal(1,"select setup");
 if(falls!=rises)$fatal(1,"duplicate falling edge");
 if(falls%8!=0 && cycles-last_rise!=216)$fatal(1,"high half period");
 if(falls!=0 && falls%8==0 && cycles-last_rise<6912)$fatal(1,"guard");
 last_fall=cycles;
 // Independent controller output delay, then two main-clock synchronizer FFs.
 #17 dat=reply[falls%8]; falls=falls+1;
end
always @(posedge sclk)if(checking && !reset)begin
 if(cycles-last_fall!=216)$fatal(1,"low half period");
 if(cmd!==tx[rises%8])$fatal(1,"CMD LSB/order/hold bit %0d",rises%8);
 rises=rises+1;last_rise=cycles;
end
task send(input [3:0] c,input [15:0] v);begin
 @(negedge clk);channel=c;value=v;wr=1;
 @(negedge clk);wr=0;
end endtask
task read(input [3:0] c);begin @(negedge clk);channel=c;rd=1;#1;end endtask
task idle;begin read(0);while(result[0])begin @(negedge clk);#1;end end endtask
task reset_dut;begin
 checking=0;@(negedge clk);reset=1;repeat(2)@(negedge clk);reset=0;
 read(0);if(result!==0 || {sclk,cmd,att}!==3'b111)$fatal(1,"reset idle");
end endtask
integer n,k,offset;
reg [7:0] saved;
initial begin
 reset_dut();
 // Illegal start without select and unknown controls/channels.
 send(1,1);read(0);if(result!==8)$fatal(1,"start without ATT");
 send(3,1);send(0,7);read(0);if(result!==8)$fatal(1,"bad command");
 send(0,3);send(0,1);idle();
 for(n=0;n<256;n=n+1)begin
  tx=n;reply=(n*73)^8'ha5;checking=1;
  send(1,tx);
  // Busy writes must not restart or consume; rejected is sticky.
  send(1,~tx);send(0,2);send(2,1);
  idle();if(result!==14)$fatal(1,"busy completion/rejection status %h",result);
  if(cycles-last_rise!=6912)$fatal(1,"guard completion must be exactly 6912 clocks");
  read(2);if(result!==reply)$fatal(1,"DAT shift/sync %h %h",result,reply);
  saved=result;
  // CPU is stopped for longer than a complete byte; result must persist.
  repeat(11000)@(negedge clk);
  read(2);if(result!==saved)$fatal(1,"result backpressure");
  send(1,0);read(2);if(result!==saved)$fatal(1,"overwrite unread result");
  device=0;read(2);if(result!==0)$fatal(1,"device decode");device=5;
  send(3,1);read(0);if(result!==6)$fatal(1,"read/watch consumed result");
  send(2,0);read(0);if(result!==4)$fatal(1,"explicit consume");
 end
 checking=0;send(0,2);idle();if(!att || !sclk || !cmd)$fatal(1,"release");
 // Reset and abort during setup, each half-bit, guard, pending result, release.
 for(k=0;k<2;k=k+1)begin
  for(n=0;n<22;n=n+1)begin
   reset_dut();send(0,1);
   if(n!=0)begin
    idle();send(1,8'h96);
    if(n<18) offset=(n-1)*216+50;
    else if(n==18)offset=4000;
    else offset=10400;
    repeat(offset)@(negedge clk);
    if(n==20)begin send(2,0);send(0,2);end
    if(n==21)send(0,1);
   end
   if(k==0) reset_dut();else begin
    send(0,3);read(0);
    if(result!==0 || {sclk,cmd,att}!==3'b111)$fatal(1,"abort phase %0d",n);
   end
   // Recovery after interrupted traffic has a fresh complete byte.
   send(0,1);idle();falls=0;rises=0;tx=8'h81;reply=8'h3c;checking=1;
   send(1,tx);idle();read(2);if(result!==reply)$fatal(1,"abort/reset recovery");
   send(2,0);checking=0;send(0,2);idle();
  end
 end
 // DAT sample is the second FF value, not the asynchronous pin at the edge.
 reset_dut();send(0,1);idle();send(1,0);
 dat=0;wait(dut.state==3 && dut.divider==1);@(negedge clk);dat=1;
 @(negedge clk);@(negedge clk);
 if(dut.shift[7]!==0)$fatal(1,"DAT bypassed synchronizer");
 send(0,3);
 $display("DS2_PASS engine 256 duplex bytes; edge, guard, backpressure, reset/abort phases");$finish;
end
endmodule
