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
integer n=9,opcode=8'h42,arg=0,id=8'h73;
reg checking=0;
function [7:0] reply(input integer b);begin
 case(b) 0:reply=255;1:reply=id;2:reply=8'h5a;default:reply=(b*73)^8'ha5;endcase
end endfunction
function [7:0] tx(input integer b);begin
 tx=0;
 if(b==0)tx=1;
 if(b==1)tx=opcode;
 if(b==3)tx=arg;
 if(opcode==8'h44 && b==4)tx=3;
 if(opcode==8'h43 && !arg && b>=4)tx=8'h5a;
end endfunction
reg [7:0] expected_tx,expected_rx;
always @(posedge clk)begin cycles=cycles+1;if(cycles>12000000)$fatal(1,"bounded packet engine");end
always @(negedge att)begin selected_at=cycles;falls=0;rises=0;end
always @(negedge sclk)if(checking)begin
 if(att || cycles-selected_at<432)$fatal(1,"select setup");
 if(falls!=rises)$fatal(1,"duplicate falling edge");
 if(falls%8!=0 && cycles-last_rise!=216)$fatal(1,"high half period");
 if(falls!=0 && falls%8==0 && cycles-last_rise!=7128)$fatal(1,"exact autonomous guard");
 last_fall=cycles;expected_rx=reply(falls/8);
 #17 dat=expected_rx[falls%8]; falls=falls+1;
end
always @(posedge sclk)if(checking && !reset)begin
 if(cycles-last_fall!=216)$fatal(1,"low half period");
 expected_tx=tx(rises/8);
 if(cmd!==expected_tx[rises%8])$fatal(1,"CMD packet/order/hold byte %0d bit%0d",rises/8,rises%8);
 rises=rises+1;last_rise=cycles;
end
always @(posedge att)if(checking && !reset)begin
 if(rises!=n*8 || falls!=n*8)$fatal(1,"packet length %0d wanted %0d",rises/8,n);
 if(cycles-last_rise!=7128)$fatal(1,"last guard/release");
end
task send(input [3:0] c,input [15:0] v);begin
 @(negedge clk);channel=c;value=v;wr=1;
 @(negedge clk);wr=0;
end endtask
task read(input [3:0] c);begin @(negedge clk);channel=c;rd=1;#1;end endtask
task idle;begin read(0);while(result[0])begin @(negedge clk);#1;end end endtask
task reset_dut;begin
 checking=0;@(negedge clk);reset=1;repeat(2)@(negedge clk);reset=0;
 read(0);if(result[3:0]!==0 || {sclk,cmd,att}!==3'b111)$fatal(1,"reset idle");
end endtask
integer i,k,j,offset;
reg [15:0] saved,expected;
initial begin
 reset_dut();
 send(0,1);read(0);if(!result[3])$fatal(1,"ACK without result");
 send(0,2);send(12,1);read(0);if(!result[3])$fatal(1,"unknown write");
 send(0,3);
 // Autonomous packets: digital, analog, pressure, configuration and unknown ID.
 for(k=0;k<6;k=k+1)begin
  opcode=8'h42;arg=0;id=8'h73;n=9;
  case(k)0:begin id=8'h41;n=5;opcode=8'h43;arg=1;end
   1:begin id=8'hf3;opcode=8'h44;arg=1;end
   2:begin id=8'hf3;opcode=8'h43;end
   3:begin id=8'h79;n=21;end
   4:begin id=8'h55;n=3;end
  endcase
  checking=1;send(1,((opcode==8'h42)?0:((opcode==8'h43)?1:2))|(arg<<8));
  send(1,0);send(0,1); // Busy writes cannot restart or ACK.
  // No CPU service throughout the entire packet and beyond completion.
  repeat(230000)@(negedge clk);
  read(0);if(result!==26 || !att)$fatal(1,"packet publication/status %h",result);
  for(i=0;i<(n-1)/2;i=i+1)begin
   expected={reply(i*2+1),reply(i*2+2)};
   read(1);saved=result;
   if(result!==expected)$fatal(1,"packed result %0d %h %h",i,result,expected);
   read(1);if(result!==saved)$fatal(1,"read/watch consumed result");
   send(1,0);read(1);if(result!==saved)$fatal(1,"unread overwritten");
   send(0,2);read(0);if(result[3:0]!==2)$fatal(1,"clear rejected");
   send(0,1);
  end
  read(0);if(result[3:0]!==0)$fatal(1,"last word releases snapshot");
  device=0;read(1);if(result!==0)$fatal(1,"device decode");device=5;
  read(2);if(result!==0)$fatal(1,"unused read");
  checking=0;
 end
 // Reset and abort across setup, each bit phase, guard, packet and valid hold.
 for(k=0;k<2;k=k+1)begin
  for(j=0;j<23;j=j+1)begin
   reset_dut();send(1,0);
   offset=j*216+30;
   if(j==20)offset=12000;
   if(j==21)offset=60000;
   if(j==22)offset=100000;
   repeat(offset)@(negedge clk);
   if(k==0)reset_dut();else begin
    send(0,3);read(0);
    if(result[3:0]!==0 || {sclk,cmd,att}!==3'b111)$fatal(1,"abort phase%0d",j);
   end
  end
 end
 // Fresh recovery and explicit two-stage DAT sampling.
 reset_dut();send(1,0);dat=0;
 wait(dut.state==3 && dut.divider==1);@(negedge clk);dat=1;
 @(negedge clk);@(negedge clk);
 if(dut.shift[7]!==0)$fatal(1,"DAT bypassed synchronizer");
 send(0,3);opcode=8'h42;arg=0;id=8'h73;n=9;checking=1;send(1,0);idle();
 read(0);if(result[3:0]!==2)$fatal(1,"recovery");
 $display("DS2_PASS autonomous5/9/21-byte packets, configuration, stable snapshot, guards, reset/abort, sync");$finish;
end
endmodule
