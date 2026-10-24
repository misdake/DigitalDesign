// Independent byte-array peer; accelerated serial divider only in this fixture.
localparam HALF=16,FRAME=HALF*64*32;
wire sclk,cmd,att; reg dat=1;
__DS2__ #(.DIV_BITS(4)) ds2(.clk(clk),.reset(reset),.device_index(query_index),.device_channel(query_channel),
 .device_read_enable(device_read_enable||watch_enable),.device_write_enable(device_write_enable&&!watch_enable),
 .device_write_data(device_write_data),.device_read_data(ds2_read_data),
 .dat(dat),.ds2_clk(sclk),.cmd(cmd),.att(att));
integer test_mode=0,axis_case=-1,buttons=16'h1234,config_mode=0,analog_mode=0;
integer byte_index=0,bit_index=0,command=0,arg=0,packets=0,completed=0,transfers=0,tail_bytes=0;
integer select_cycle=0,last_rise=0,last_fall=0,pause_triggered=0,paused_bytes=0;
integer frame_mode=0,frame_config=0,frame_analog=0,frame_axis=-1,frame_buttons=0;
reg[7:0]received=0,response=255;
assign test_data=(query_index==7&&query_channel==3)?completed[15:0]:16'd0;
always @(posedge clk)begin
 if(pause_left>0)pause_left=pause_left-1;
 if(test_mode==6&&!pause_triggered&&!att)begin pause_triggered=1;pause_left=FRAME*3;end
 if(device_write_enable&&device_index==7)begin
  if(device_channel==0)begin
   test_mode=device_write_data;pause_triggered=0;
   if(test_mode==8)begin analog_mode=0;config_mode=0;end
   if(test_mode==11)begin config_mode=1;analog_mode=1;end
  end
  if(device_channel==1)axis_case=device_write_data;
  if(device_channel==2)buttons=device_write_data;
 end
end
function[7:0]axis;input integer a;integer raw;begin
 if(frame_axis<0)case(a)0:axis=0;1:axis=255;2:axis=127;default:axis=128;endcase
 else begin
  case(frame_axis)0:raw=0;1:raw=127;2:raw=128;default:raw=255;endcase
  case(a)0:axis=raw;1:axis=255-raw;2:axis=raw^128;default:axis=raw^127;endcase
 end
end endfunction
function[7:0]reply;input integer n;begin
 case(n)
  0:reply=255;
  1:if(frame_mode==2)reply=8'h55;
    else if(frame_mode==5)reply=255;
    else if(frame_config)reply=8'hf3;
    else if(!frame_analog)reply=8'h41;
    else if(frame_mode==1)reply=8'h79;else reply=8'h73;
  2:if(frame_mode==3)reply=0;else if(frame_mode==4||frame_mode==5)reply=255;else reply=8'h5a;
  3:reply=~frame_buttons;
  4:reply=~(frame_buttons>>8);
  5,6,7,8:reply=axis(n-5);
  default:reply=8'h80+n;
 endcase
end endfunction
always @(negedge att)if(!reset)begin
 byte_index=0;bit_index=0;command=0;arg=0;select_cycle=cycles;packets=packets+1;
 frame_mode=test_mode;frame_config=config_mode;frame_analog=analog_mode;
 frame_axis=axis_case;frame_buttons=buttons;
end
always @(negedge sclk)if(!reset)begin
 if(att||cycles-select_cycle<HALF*64)$fatal(1,"CPU select setup");
 if(bit_index!=0&&cycles-last_rise!=HALF)$fatal(1,"CPU high half");
 if(byte_index!=0&&bit_index==0&&cycles-last_rise!=49*HALF)$fatal(1,"CPU byte guard");
 last_fall=cycles;#17 dat=reply(byte_index)>>bit_index;
end
always @(posedge sclk)if(!att&&!reset)begin
 if(cycles-last_fall!=HALF)$fatal(1,"CPU low half");
 received[bit_index]=cmd;last_rise=cycles;
 if(bit_index==7)begin
  transfers=transfers+1;if(pause_left>0)paused_bytes=paused_bytes+1;
  if(byte_index==0&&received!=1)$fatal(1,"CPU packet address");
  if(byte_index==1)begin
   command=received;
   if(command!=8'h42&&command!=8'h43&&command!=8'h44)$fatal(1,"CPU command");
   if(command==8'h44&&!frame_config)$fatal(1,"mode before enter");
  end
  if(byte_index==2&&received!=0)$fatal(1,"CPU packet third byte");
  if(byte_index==3)arg=received;
  if(command==8'h44&&byte_index==4&&received!=3)$fatal(1,"analog lock");
  if(command==8'h43&&arg==0&&byte_index>=4&&received!=8'h5a)$fatal(1,"exit fill");
  if(command==8'h42&&byte_index>=3&&received!=0)$fatal(1,"poll fill");
  if(byte_index>=9)tail_bytes=tail_bytes+1;
  byte_index=byte_index+1;bit_index=0;
 end else bit_index=bit_index+1;
end
always @(posedge att)if(!reset&&packets>0)begin
 completed=completed+1;
 if(command==8'h43&&arg==1)config_mode=1;
 if(command==8'h44&&frame_mode!=8)analog_mode=1;
 if(command==8'h43&&arg==0)config_mode=0;
end
integer index;
initial begin
 for(index=0;index<65536;index=index+1)begin rom[index]=16'h6c00;memory[index]=0;end
 __PROGRAM__
 repeat(3)@(posedge clk);@(negedge clk);reset=0;
 wait(halted||fault);#1;
 if(fault||halt_signal!==16'h600d)$fatal(1,"CPU protocol halt=%h fault=%b PC=%h",halt_signal,fault,pc);
 if(tail_bytes<12||paused_bytes<9||packets<30)$fatal(1,"CPU coverage packets=%0d tail=%0d paused=%0d",packets,tail_bytes,paused_bytes);
 $display("DS2_PASS CPU/cache latest snapshots packets=%0d bytes=%0d tail=%0d paused=%0d cycles=%0d",packets,transfers,tail_bytes,paused_bytes,cycles);$finish;
end
endmodule
