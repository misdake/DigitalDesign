// Independent byte-stream model; no production packer or hand-copied code array.
module tb;
parameter CASE=0;
reg clk=0;
always #5 clk=~clk;
reg ready_to_start=0,flash_ready=1,flash_data_valid=0,flash_done=0,flash_error=0;
reg [7:0] flash_data=0;
reg memory_request_ready=0,memory_response_valid=0,memory_error=0;
reg display_started=0,display_error=0,cache_ready=1;
wire flash_start,flash_data_ready,memory_request_valid,memory_response_ready,load_done,done,error;
wire [23:0] flash_address,flash_length;
wire [21:0] memory_address;
wire [15:0] memory_write_data;
wire [3:0] error_code;
SingleBootLoader #(.TIMEOUT_BITS(8)) dut(.*);
localparam integer COPY_BYTES=CASE==19 ? 2097152 : CASE==20 ? 2 : 8;
localparam [31:0] COPY_DEST=CASE==19 ? 32'h100000 : CASE==20 ? 32'h7ffffe : 32'he0000;
integer cycle=0,received=0,write_count=0,completed=0,load_cycle=0;
integer position=0,bytes_left=0,done_delay=-1,mem_delay=-1;
reg [21:0] saved_address=0;
reg [15:0] saved_data=0;
reg saved_error=0;
reg held=0;
reg [21:0] held_address=0;
reg [15:0] held_data=0;
integer terminal_cycles=0;
reg terminal_done=0,terminal_error=0;
wire expect_failure=(CASE>=2 && CASE<=15) || CASE==17 || CASE==22 || CASE==23 || CASE==24 || CASE==26 || CASE==27;
function [7:0] byte_at;
 input integer pos;
 reg [31:0] value;
 integer i;
 begin
  if(pos<4)begin
   case(pos)0:byte_at="C";1:byte_at="B";2:byte_at="0";default:byte_at="6";endcase
   if(CASE==10 && pos==1)byte_at=0;
   if(CASE==23)byte_at=8'hff;
  end else if(pos<12)begin
   value=pos<8 ? 0 : 32'h80000020;
   byte_at=value>>((pos%4)*8);
  end else if(pos<20)begin
   if(pos<16)begin
    value=COPY_DEST;
    case(CASE)
     2:value=COPY_DEST+1;4:value=32'h80_0e0000;
     6:value=32'h7ffffe;7:value=32'h460000;8:value=32'h45fffe;27:value=32'h7ffffe;
     default:begin end
    endcase
   end else begin
    value=COPY_BYTES;
    case(CASE)
     3:value=3;5:value=32'h01000008;9:value=32'h80000000;27:value=32'h80c00002;
     default:begin end
    endcase
   end
   byte_at=value>>((pos%4)*8);
  end else if(pos<20+COPY_BYTES) begin
   i=pos-20;byte_at=i^(i>>8)^8'h53;
  end else begin
   byte_at=CASE==22 && pos==20+COPY_BYTES ? 2 : 0;
  end
 end
endfunction
always @(posedge clk) begin
 cycle=cycle+1;
 if(cycle==3)ready_to_start<=1;
 flash_done<=0;flash_error<=0;
 if(flash_start)begin
  if(!flash_ready || bytes_left!=0 || done_delay>=0)$fatal(1,"overlapping SPI command");
  if(flash_length!=4 && flash_length!=8 && flash_length!=COPY_BYTES)$fatal(1,"SPI length");
  position=flash_address-24'h100000;
  bytes_left=flash_length;flash_ready<=0;
 end
 if(flash_data_valid && flash_data_ready)begin
  flash_data_valid<=0;position=position+1;bytes_left=bytes_left-1;received=received+1;
  if(CASE==11 && position==20+COPY_BYTES)flash_error<=1;
  if(CASE==26 && position==28+COPY_BYTES)flash_error<=1;
  if(bytes_left==0)done_delay=(CASE==1 || CASE==25) ? 100 : 0;
 end else if(!flash_data_valid && !flash_ready && bytes_left>0 && cycle%3!=0 &&
             !(CASE==14 && position>=20) && !(CASE==24 && position==2))begin
  flash_data<=byte_at(position);flash_data_valid<=1;
 end
 if(done_delay==0)begin flash_done<=1;flash_ready<=1;done_delay=-1;end
 else if(done_delay>0)done_delay=done_delay-1;
 memory_request_ready<=cycle%7>=2;
 if(held && (!memory_request_valid || memory_address!==held_address || memory_write_data!==held_data))
  $fatal(1,"request changed under backpressure");
 held<=memory_request_valid && !memory_request_ready;
 held_address<=memory_address;held_data<=memory_write_data;
 if(memory_request_valid && memory_request_ready)begin
  if(mem_delay>=0 || memory_response_valid)$fatal(1,"more than one boot request");
  saved_address=memory_address;saved_data=memory_write_data;
  if(memory_address<16)begin
   if(memory_write_data!==0)$fatal(1,"ZERO not materialized");
  end else begin
   if(memory_address!==COPY_DEST/2+(write_count-16))$fatal(1,"COPY target");
   if(memory_write_data!=={byte_at(20+(write_count-16)*2+1),byte_at(20+(write_count-16)*2)})
    $fatal(1,"COPY data");
  end
  write_count=write_count+1;mem_delay=3;
  saved_error=CASE==12 && write_count==16+COPY_BYTES/2;
 end
 if(mem_delay==0 && CASE!=13)begin
  memory_response_valid<=1;memory_error<=saved_error;mem_delay=-1;
 end else if(mem_delay>0)mem_delay=mem_delay-1;
 if(memory_response_valid && memory_response_ready)begin
  memory_response_valid<=0;memory_error<=0;
  if(!memory_error)completed=completed+1;
 end
 if(load_done && load_cycle==0)begin
  load_cycle=cycle;
  if(completed!=16+COPY_BYTES/2 || dut.in_flight || mem_delay>=0 || memory_response_valid)
   $fatal(1,"load_done before real final completion %0d",completed);
 end
 if(load_cycle!=0)begin
  if(CASE!=15 && cycle>=load_cycle+20)display_started<=1;
  if(CASE==16)cache_ready<=cycle>=load_cycle+100;
  if(CASE==17 && cycle>=load_cycle+20)display_error<=1;
 end
 if(done && (!load_done || !display_started || !cache_ready || display_error))$fatal(1,"early CPU release");
 if(done || error)begin
  if(terminal_cycles==0)begin
   terminal_done=done;terminal_error=error;
   if(error!==expect_failure)$fatal(1,"unexpected result case%0d error%0d code%0d",CASE,error,error_code);
   if(CASE==12 && error_code!=4)$fatal(1,"last memory error lost");
   if((CASE==10 || CASE==23 || CASE==24) && write_count!=0)$fatal(1,"bad signature wrote DRAM");
   if(CASE==17 && done)$fatal(1,"error lost to Display ack");
   if(CASE==26 && load_done)$fatal(1,"failed END published load_done");
   if(CASE==27 && write_count!=16)$fatal(1,"overflowing ZERO emitted a write");
  end else if(done!==terminal_done || error!==terminal_error)$fatal(1,"terminal restarted");
  terminal_cycles=terminal_cycles+1;ready_to_start<=~ready_to_start;
  if(terminal_cycles==20)begin
   $display("PASS single boot case%0d cycles%0d writes%0d completed%0d error%0d",CASE,cycle,write_count,completed,error_code);
   $finish;
  end
 end
 if(cycle>(CASE==19 ? 20000000 : 10000))$fatal(1,"bounded CB06 scenario case%0d state%0d",CASE,dut.state);
end
endmodule
