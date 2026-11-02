`timescale 1ns/1ps
module tb;
reg write_clock=0,read_clock=0,write_enable=0;
reg [8:0] write_address=0,read_address_a=0,read_address_b=0;
reg [63:0] write_data=0;
wire [31:0] read_data_a,read_data_b;
DisplayLineBuffer dut(.*);
reg [2047:0] vectors;
integer fd,result,index,logic_edge,pixel_edge,enable,known_a,known_b,count=0;
reg [31:0] expected_a,expected_b;
initial begin
 if(!$value$plusargs("vectors=%s",vectors)) $fatal(1,"missing vectors");
 fd=$fopen(vectors,"r");
 if(!fd) $fatal(1,"cannot open vectors");
 while(!$feof(fd)) begin
  result=$fscanf(fd,"%d %d %d %d %h %h %h %h %h %d %h %d\n",
   index,logic_edge,pixel_edge,enable,write_address,write_data,
   read_address_a,read_address_b,expected_a,known_a,expected_b,known_b);
  if(result!=12) $fatal(1,"malformed vector %0d %0d",count,result);
  write_enable=enable;
  #1; write_clock=logic_edge; read_clock=pixel_edge;
  #1;
  if(known_a ? read_data_a!==expected_a : !$isunknown(read_data_a))
   $fatal(1,"RAM_Q_A edge=%0d actual=%h expected=%h known=%0d",index,read_data_a,expected_a,known_a);
  if(known_b ? read_data_b!==expected_b : !$isunknown(read_data_b))
   $fatal(1,"RAM_Q_B edge=%0d actual=%h expected=%h known=%0d",index,read_data_b,expected_b,known_b);
  $display("Q %0d %h %h",index,read_data_a,read_data_b);
  write_clock=0; read_clock=0; #1;
  count=count+1;
  if(count>4096) $fatal(1,"RAM edge cap");
 end
 $fclose(fd);
 $display("PASS RAM %0d",count);
 $finish;
end
initial begin #20000; $fatal(1,"RAM time cap"); end
endmodule
