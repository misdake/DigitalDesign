`timescale 1ns/1ps
module tb;
parameter DIV_BITS=2;
localparam HALF=1<<DIV_BITS,FRAME=HALF*64*32;
reg clk=0;always #5 clk=~clk;
reg reset=1,dat=1;
reg[2:0]device_index=5;reg[3:0]device_channel=8;
reg device_read_enable=0,device_write_enable=0;reg[15:0]device_write_data=0;
wire[15:0]device_read_data;wire ds2_clk,cmd,att;
`ifndef PORTABLE
GSR GSR(.GSRI(1'b1));
`endif
__DS2__ #(.DIV_BITS(DIV_BITS))dut(.*);
integer cycle=0,transactions=0,pubs=0,bytes=0,wire_bit=0,wire_byte=0;
integer fall_cycle=0,rise_cycle=0,att_cycle=0,previous_att=-1,last_byte_rise=0,cmd_change=0;
integer mode_id=8'h41,frame_id=0,operation=0,test_kind=0,connected=1;
integer bad_marker=0,bad_prefix=0,unknown=0,configure_enabled=1;
integer frame_connected=1,frame_bad_marker=0,frame_bad_prefix=0,frame_unknown=0;
integer last_transaction_bytes=0,last_transaction_command=0,changed_checks=0;
reg[7:0]sent[0:20];reg[7:0]observed=0;reg[7:0]reply=8'hff;
reg[7:0]sample_id[0:1];reg[7:0]sample_payload[0:15];
reg[7:0]read_bytes[0:7];reg[15:0]status;reg old_front=0;
function[7:0] response;input integer n;begin
 if(!frame_connected)response=8'hff;
 else case(n)
  0:response=frame_bad_prefix?8'hfe:8'hff;
  1:response=frame_unknown?8'h12:frame_id;
  2:response=frame_bad_marker?8'ha5:8'h5a;
  3:response=(transactions*17)^8'h9c;
  4:response=(transactions*13)^8'hc3;
  default:if(frame_id==8'h41&&n>=5)response=8'hff;
          else response=(transactions*29+n*37)^8'h58;
 endcase
end endfunction
function[7:0] expected_command;input integer n;begin
 case(n)
  0:expected_command=1;
  1:case(operation)0:expected_command=8'h42;1:expected_command=8'h43;2:expected_command=8'h44;default:expected_command=8'h43;endcase
  3:expected_command=(operation==1||operation==2)?1:0;
  default:if(operation==3&&n>=4)expected_command=8'h5a;
          else if(operation==2&&n==4)expected_command=3;
          else expected_command=0;
 endcase
end endfunction
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>FRAME*45)$fatal(1,"autonomous DS2 watchdog");
 #1;
 if(!reset&&dut.front!=old_front)begin
  pubs=pubs+1;old_front=dut.front;
  if(operation!=0||!frame_connected||frame_bad_marker||frame_bad_prefix||frame_unknown||frame_id==8'hf3)$fatal(1,"invalid snapshot publication");
  for(integer n=1;n<=8;n=n+1)begin
   sample_payload[(dut.front?8:0)+((n+1)&7)]=response(n);
  end
  sample_id[dut.front]=frame_id;
 end
end
always @(negedge att)if(!reset)begin
 if(previous_att>=0&&cycle-previous_att!=FRAME)$fatal(1,"CPU-independent polling period");
 previous_att=cycle;att_cycle=cycle;transactions=transactions+1;
 frame_id=mode_id;frame_connected=connected;frame_bad_marker=bad_marker;frame_bad_prefix=bad_prefix;frame_unknown=unknown;operation=dut.operation;wire_bit=0;wire_byte=0;observed=0;
 fall_cycle=0;rise_cycle=0;last_byte_rise=0;reply=response(0);dat=1;
end
always @(cmd)cmd_change=cycle;
always @(negedge ds2_clk)if(!reset)begin
 if(cycle-cmd_change<HALF-1)$fatal(1,"CMD setup shorter than a half-period");
 if(att)$fatal(1,"CLK outside ATT");
 if(wire_byte==0&&wire_bit==0&&cycle-att_cycle<HALF*64)$fatal(1,"ATT setup shortened");
 if(wire_bit==0&&wire_byte>0&&cycle-last_byte_rise!=49*HALF)$fatal(1,"byte guard");
 if(wire_bit!=0&&cycle-rise_cycle!=HALF)$fatal(1,"CLK high width");
 if(cycle-fall_cycle==0)$fatal(1,"duplicate CLK fall");
 fall_cycle=cycle;observed[wire_bit]=cmd;reply=response(wire_byte);dat=reply[wire_bit];
end
always @(posedge ds2_clk)if(!reset&&!att&&fall_cycle>0)begin
 if(cycle-fall_cycle!=HALF)$fatal(1,"CLK low width");
 if(cmd!==observed[wire_bit])$fatal(1,"CMD changed before sampling edge");
 rise_cycle=cycle;
 if(wire_bit==7)begin
  if(observed!==expected_command(wire_byte))$fatal(1,"command op%0d byte%0d got%h want%h",operation,wire_byte,observed,expected_command(wire_byte));
  sent[wire_byte]=observed;wire_byte=wire_byte+1;wire_bit=0;last_byte_rise=cycle;bytes=bytes+1;
 end else wire_bit=wire_bit+1;
end
always @(posedge att)if(!reset&&transactions>0)begin
 if(wire_bit!=0)$fatal(1,"truncated serial byte");
 if(!frame_connected||frame_bad_prefix||frame_bad_marker||frame_unknown)begin
  if(wire_byte!=3)$fatal(1,"invalid reply must release after3 bytes");
 end else if(operation==1)begin if(wire_byte!=5)$fatal(1,"enter length");end
 else if(operation==0&&frame_id==8'h79)begin if(wire_byte!=21)$fatal(1,"pressure tail must be completely clocked");end
 else if(wire_byte!=9)$fatal(1,"normal/config length");
 last_transaction_bytes=wire_byte;last_transaction_command=sent[1];
 if(frame_connected&&!frame_bad_marker&&!frame_bad_prefix&&!frame_unknown&&configure_enabled)begin
  if(operation==1)mode_id=8'hf3;
  if(operation==3)mode_id=8'h73;
 end
 dat=1;
end
task tick;begin @(posedge clk);#2;end endtask
task end_transaction;input integer n;begin while(transactions<n||!att)tick();end endtask
task begin_snapshot;begin
 @(negedge clk);device_index=5;device_channel=8;device_write_enable=1;
 tick();@(negedge clk);device_write_enable=0;
end endtask
task read_status;begin @(negedge clk);device_index=5;device_channel=8;device_read_enable=1;#1;status=device_read_data;tick();end endtask
task coherent_snapshot;begin
 begin_snapshot();read_status();if(!status[0])$fatal(1,"expected valid sample");
 for(integer i=0;i<8;i=i+1)begin
  @(negedge clk);device_channel=i;#1;read_bytes[i]=device_read_data[7:0];
  if(device_read_data[15:8]!=0)$fatal(1,"sample upper bits");tick();
 end
 read_status();if(status[1])$fatal(1,"unexpected retry in short coherent read");
 for(integer j=0;j<8;j=j+1)if(read_bytes[j]!==sample_payload[{dut.front,j[2:0]}])$fatal(1,"snapshot raw byte%0d got%h want%h",j,read_bytes[j],sample_payload[{dut.front,j[2:0]}]);
end endtask
initial begin
 if(DIV_BITS==8)mode_id=8'h79;
 tick();tick();@(negedge clk);reset=0;
 if(DIV_BITS==8)begin
  end_transaction(2);coherent_snapshot();
  if(last_transaction_bytes!=21)$fatal(1,"default-rate pressure tail");
  $display("DS2_PASS default timing half=%0d frame=%0d completeBytes=%0d",HALF,FRAME,bytes);$finish;
 end
 // CPU absent: configure digital->analog by hardware, then keep polling.
 end_transaction(6);if(mode_id!=8'h73||pubs<2)$fatal(1,"autonomous configuration/publication");
 coherent_snapshot();
 // Two publications cannot cancel a sticky coherence marker (no toggle ABA).
 begin_snapshot();end_transaction(8);read_status();if(!status[1]||!status[0])$fatal(1,"multiple updates lost");changed_checks=changed_checks+1;
 coherent_snapshot();
 // A CPU begin on exactly the publish edge must lose to publication.
 while(!dut.publish)begin @(negedge clk);#1;end
 device_channel=8;device_write_enable=1;tick();@(negedge clk);device_write_enable=0;
 read_status();if(!status[1])$fatal(1,"publish/begin priority");changed_checks=changed_checks+1;
 // Pressure receives all21 bytes but publishes only the eight useful fields.
 mode_id=8'h79;end_transaction(transactions+2);if(last_transaction_bytes!=21)$fatal(1,"pressure receive");coherent_snapshot();
 // Bad marker and bad prefix invalidate, never publish partially overwritten pages.
 begin_snapshot();bad_marker=1;end_transaction(transactions+2);read_status();if(status[0]||!status[1])$fatal(1,"bad marker invalidation");
 begin_snapshot();end_transaction(transactions+2);read_status();if(status!=0)$fatal(1,"stable invalid state falsely changed");
 bad_marker=0;end_transaction(transactions+2);if(!dut.valid)$fatal(1,"recovery before invalidate race");
 bad_marker=1;while(!(dut.invalidate&&dut.valid))begin @(negedge clk);#1;end
 device_channel=8;device_write_enable=1;tick();@(negedge clk);device_write_enable=0;
 read_status();if(status[0]||!status[1])$fatal(1,"invalidate/begin priority");changed_checks=changed_checks+1;
 bad_marker=0;bad_prefix=1;end_transaction(transactions+2);read_status();if(status[0])$fatal(1,"bad prefix valid");
 bad_prefix=0;unknown=1;end_transaction(transactions+2);read_status();if(status[0])$fatal(1,"unknown mode valid");
 unknown=0;connected=0;end_transaction(transactions+2);read_status();if(status[0])$fatal(1,"disconnect valid");
 // Reconnect while already in config mode: hardware exits and polls again.
 connected=1;mode_id=8'hf3;end_transaction(transactions+4);if(!dut.valid||mode_id!=8'h73)$fatal(1,"F3 recovery");coherent_snapshot();
 // A different device's SEND cannot clear read_changed or disturb acquisition.
 begin_snapshot();end_transaction(transactions+2);
 @(negedge clk);device_index=4;device_channel=8;device_write_enable=1;tick();
 @(negedge clk);device_write_enable=0;#1;if(device_read_data!==0)$fatal(1,"unselected output");
 read_status();if(!status[1])$fatal(1,"unselected write changed state");
 // Reset can interrupt traffic, but autonomous polling must resume.
 while(att)tick();@(negedge clk);reset=1;previous_att=-1;old_front=0;
 tick();if(!att||!cmd||!ds2_clk||dut.valid||dut.read_changed)$fatal(1,"reset outputs");
 @(negedge clk);reset=0;end_transaction(transactions+2);coherent_snapshot();
 $display("DS2_PASS autonomous frames=%0d bytes=%0d publications=%0d coherence=%0d divider=%0d",transactions,bytes,pubs,changed_checks,DIV_BITS);$finish;
end
endmodule
