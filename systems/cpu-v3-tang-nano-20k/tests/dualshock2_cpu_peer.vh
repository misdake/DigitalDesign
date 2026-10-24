integer progress;
initial progress=$fopen("target/dualshock2-validation/cpu/progress.log","w");
wire sclk,cmd,att;
reg dat=1;
__DS2__ ds2(.clk(clk),.reset(reset),.device_index(query_index),.device_channel(query_channel),
 .device_read_enable(device_read_enable||watch_enable),.device_write_enable(device_write_enable&&!watch_enable),
 .device_write_data(device_write_data),.device_read_data(ds2_read_data),
 .dat(dat),.ds2_clk(sclk),.cmd(cmd),.att(att));
integer test_mode=0,axis_case=-1,buttons=16'h1234,config_mode=0,analog_mode=0;
integer byte_index=0,bit_index=0,command=0,arg=0,packets=0,transfers=0,tail_bytes=0;
integer select_cycle=0,last_rise=0,last_fall=0,pause_triggered=0,watch_ticks=0;
integer long_pause_bytes=0;
reg [7:0] response=255,received=0;
always @(posedge clk)begin
 if(cycles%1000000==0)begin $fdisplay(progress,"cycle=%0d pc=%h packets=%0d state=%0d watch=%b",cycles,pc,packets,ds2.state,watch_enable);$fflush(progress);end
 if(watch_enable)watch_ticks=watch_ticks+1;
 if(pause_left>0)pause_left=pause_left-1;
 if((test_mode==6||test_mode==7) && !pause_triggered && ds2.state==2)begin
  pause_triggered=1;
  if(test_mode==6)pause_left=1200000;else pause_left=3600000;
 end
 if(device_write_enable && device_index==7)begin
  if(device_channel==0)begin
   test_mode=device_write_data;pause_triggered=0;
   if(test_mode==8)begin analog_mode=0;config_mode=0;end
   if(test_mode==11)begin config_mode=1;analog_mode=1;end
  end
  if(device_channel==1)axis_case=device_write_data;
  if(device_channel==2)buttons=device_write_data;
 end
end
always @(negedge att)begin
 byte_index=0;bit_index=0;command=0;arg=0;response=255;
 select_cycle=cycles;packets=packets+1;
end
function [7:0] axis(input integer a);
 integer raw;begin
  if(axis_case<0)case(a)0:axis=0;1:axis=255;2:axis=127;3:axis=128;endcase
  else begin
   case(axis_case)0:raw=0;1:raw=127;2:raw=128;default:raw=255;endcase
   case(a)0:axis=raw;1:axis=255-raw;2:axis=raw^128;3:axis=raw^127;endcase
  end
 end
endfunction
always @(negedge sclk)begin
 if(att || cycles-select_cycle<216)$fatal(1,"CPU select setup");
 if(bit_index!=0 && cycles-last_rise!=216)$fatal(1,"CPU high half");
 if(byte_index!=0 && bit_index==0 && cycles-last_rise<6912)$fatal(1,"CPU guard");
 last_fall=cycles;
 #17 dat=response[bit_index];
end
always @(posedge sclk)if(!att && !reset)begin
 if(cycles-last_fall!=216)$fatal(1,"CPU low half");
 received[bit_index]=cmd;last_rise=cycles;
 if(bit_index==7)begin
  transfers=transfers+1;if(pause_left>0)long_pause_bytes=long_pause_bytes+1;
  if(byte_index==0 && test_mode!=9 && received!=1)$fatal(1,"CPU packet address");
  if(byte_index==1)begin
   command=received;
   if(command!=8'h42 && command!=8'h43 && command!=8'h44)$fatal(1,"CPU command");
   if(command==8'h44 && !config_mode)$fatal(1,"mode before enter");
  end
  if(byte_index==2 && received!=0)$fatal(1,"CPU packet third byte");
  if(byte_index==3)arg=received;
  if(command==8'h44 && byte_index==4 && received!=3)$fatal(1,"analog lock");
  if(command==8'h43 && arg==0 && byte_index>=4 && received!=8'h5a)$fatal(1,"exit fill");
  if(command==8'h42 && byte_index>=3 && received!=0)$fatal(1,"poll fill");
  if(byte_index>=9)tail_bytes=tail_bytes+1;
  byte_index=byte_index+1;bit_index=0;
  case(byte_index)
   1: if(test_mode==2)response=8'h55;
      else if(test_mode==5)response=255;
      else if(config_mode)response=8'hf3;
      else if(!analog_mode)response=8'h41;
      else if(test_mode==1)response=8'h79;else response=8'h73;
   2: if(test_mode==3)response=0;else if(test_mode==4||test_mode==5)response=255;else response=8'h5a;
   3: response=~buttons;
   4: response=~(buttons>>8);
   5,6,7,8: response=axis(byte_index-5);
   default: response=8'h80+byte_index;
  endcase
 end else bit_index=bit_index+1;
end
always @(posedge att)if(!reset && byte_index>=5 && test_mode!=9)begin
 $fdisplay(progress,"packet=%0d mode=%0d cmd=%h bytes=%0d cycles=%0d",packets,test_mode,command,byte_index,cycles);$fflush(progress);
 if(command==8'h42 && test_mode==1 && byte_index!=21)$fatal(1,"pressure tail missing");
 if(command==8'h43 && arg==1)begin
  if(byte_index!=5 && !analog_mode)$fatal(1,"digital entry length");
  config_mode=1;
 end
 if(command==8'h44)begin
  if(byte_index!=9 || arg!=1)$fatal(1,"mode packet");
  if(test_mode!=8)analog_mode=1;
 end
 if(command==8'h43 && arg==0)begin
  if(byte_index!=9)$fatal(1,"exit packet");config_mode=0;
 end
end
integer index;
initial begin
 for(index=0;index<65536;index=index+1)begin rom[index]=16'h6c00;memory[index]=0;end
 __PROGRAM__
 repeat(3)@(posedge clk);@(negedge clk);reset=0;
 wait(halted||fault);#1;
 if(fault || halt_signal!==16'h600d)$fatal(1,"CPU protocol halt=%h fault=%b PC=%h",halt_signal,fault,pc);
 if(tail_bytes<12 || watch_ticks<6000 || long_pause_bytes<2)$fatal(1,"CPU coverage");
 $display("DS2_PASS real CPU/cache Device5: packets=%0d bytes=%0d tail=%0d watch=%0d paused=%0d cycles=%0d",packets,transfers,tail_bytes,watch_ticks,long_pause_bytes,cycles);$finish;
end
endmodule
