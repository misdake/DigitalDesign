// Cold-only CB06 interpreter. One word and one real memory response at a time.
module SingleBootLoader #(parameter TIMEOUT_BITS=24) (
 input wire clk, input wire ready_to_start,
 input wire flash_ready, input wire flash_data_valid,
 input wire [7:0] flash_data, input wire flash_done, input wire flash_error,
 input wire memory_request_ready, input wire memory_response_valid, input wire memory_error,
 input wire display_started, input wire display_error, input wire cache_ready,
 output wire flash_start, output wire [23:0] flash_address, output wire [23:0] flash_length,
 output wire flash_data_ready,
 output wire memory_request_valid, output wire [21:0] memory_address,
 output wire [15:0] memory_write_data, output wire memory_response_ready,
 output reg load_done=0, output wire done, output wire error, output wire [3:0] error_code
);
localparam WAIT=0,SIGN_START=1,SIGN=2,SIGN_DONE=3,CMD_START=4,CMD=5,CMD_DONE=6,
 COPY_START=7,LOW_BYTE=8,HIGH_BYTE=9,REQUEST=10,RESPONSE=11,COPY_DONE=12,
 DISPLAY=13,FINISHED=14,FAILED=15;
reg [3:0] state=WAIT;
reg [23:0] cursor=24'h100000;
reg [2:0] index=0;
reg [21:0] target=0;
reg [21:0] remaining=0;
reg zeroing=0,bad_command=0,flash_finished=0,in_flight=0;
reg [15:0] write_data=0;
reg [3:0] failure=0;
reg [TIMEOUT_BITS-1:0] timeout_count=0;
wire flash_fire=flash_data_valid && flash_data_ready;
wire request_fire=memory_request_valid && memory_request_ready;
wire response_fire=memory_response_valid && memory_response_ready;
wire [22:0] target_end={1'b0,target}+{1'b0,remaining};
wire command_bad_range=target_end>23'h400000 ||
 (target<22'h248000 && target_end>23'h230000);
assign flash_start=(state==SIGN_START || state==CMD_START || state==COPY_START) && flash_ready;
assign flash_address=cursor;
assign flash_length=state==SIGN_START ? 24'd4 : state==CMD_START ? 24'd8 : {1'b0,remaining,1'b0};
assign flash_data_ready=state==SIGN || state==CMD || state==LOW_BYTE || state==HIGH_BYTE;
assign memory_request_valid=state==REQUEST;
assign memory_address=target;
assign memory_write_data=zeroing ? 16'b0 : write_data;
assign memory_response_ready=in_flight;
assign done=state==FINISHED;
assign error=state==FAILED;
assign error_code=failure;
function [7:0] signature_byte;
 input [1:0] i;
 begin case(i)
  0:signature_byte=8'h43;1:signature_byte=8'h42;
  2:signature_byte=8'h30;default:signature_byte=8'h36;
 endcase end
endfunction
always @(posedge clk) begin
 if(flash_fire)cursor<=cursor+1'b1;
 if(flash_done)flash_finished<=1;
 if(request_fire)in_flight<=1;
 if(response_fire)in_flight<=0;
 if(state!=WAIT && state!=FINISHED && state!=FAILED) begin
  if(flash_fire || request_fire || response_fire || flash_start || flash_done)
   timeout_count<=0;
  else timeout_count<=timeout_count+1'b1;
 end
 case(state)
 WAIT:if(ready_to_start)state<=SIGN_START;
 SIGN_START:if(flash_ready)begin index<=0;flash_finished<=0;state<=SIGN;end
 SIGN:if(flash_fire)begin
  index<=index+1'b1;
  if(index==3)state<=SIGN_DONE;
  if(flash_data!=signature_byte(index[1:0]))begin state<=FAILED;failure<=1;end
 end
 SIGN_DONE:if(flash_finished || flash_done)state<=CMD_START;
 CMD_START:if(flash_ready)begin
  index<=0;bad_command<=0;flash_finished<=0;state<=CMD;
 end
 CMD:if(flash_fire)begin
  index<=index+1'b1;
  case(index)
   0:begin target[6:0]<=flash_data[7:1];if(flash_data[0])bad_command<=1;end
   1:target[14:7]<=flash_data;
   2:begin target[21:15]<=flash_data[6:0];if(flash_data[7])bad_command<=1;end
   3:if(flash_data!=0)bad_command<=1;
   4:begin remaining[6:0]<=flash_data[7:1];if(flash_data[0])bad_command<=1;end
   5:remaining[14:7]<=flash_data;
   6:begin remaining[21:15]<=flash_data[6:0];if(flash_data[7])bad_command<=1;end
   7:begin zeroing<=flash_data[7];if(flash_data[6:0]!=0)bad_command<=1;state<=CMD_DONE;end
  endcase
 end
 CMD_DONE:if(flash_finished || flash_done)begin
  if(bad_command)begin state<=FAILED;failure<=2;end
  else if(remaining==0)begin
   if(target!=0 || zeroing)begin state<=FAILED;failure<=1;end
   else begin load_done<=!flash_error;state<=DISPLAY;end
  end else if(command_bad_range)begin state<=FAILED;failure<=2;end
  else if(zeroing)state<=REQUEST;
  else state<=COPY_START;
 end
 COPY_START:if(flash_ready)begin flash_finished<=0;state<=LOW_BYTE;end
 LOW_BYTE:if(flash_fire)begin write_data[7:0]<=flash_data;state<=HIGH_BYTE;end
 HIGH_BYTE:if(flash_fire)begin write_data[15:8]<=flash_data;state<=REQUEST;end
 REQUEST:if(request_fire)state<=RESPONSE;
 RESPONSE:if(response_fire)begin
  target<=target+1'b1;remaining<=remaining-1'b1;
  if(remaining==1)begin
   if(zeroing)state<=CMD_START;else state<=COPY_DONE;
  end else if(zeroing)state<=REQUEST;
  else state<=LOW_BYTE;
 end
 COPY_DONE:if(flash_finished || flash_done)state<=CMD_START;
 DISPLAY:if(display_started && cache_ready)state<=FINISHED;
 default:begin end
 endcase
 // Errors win over the last write, END and Display acknowledgement.
 if(state!=WAIT && state!=FINISHED && state!=FAILED)begin
  if(flash_error)begin state<=FAILED;failure<=3;end
  else if(response_fire && memory_error)begin state<=FAILED;failure<=4;end
  else if(display_error && load_done)begin state<=FAILED;failure<=4;end
  else if(&timeout_count && !(flash_fire || request_fire || response_fire || flash_start || flash_done))begin
   state<=FAILED;failure<=5;
  end
 end
end
endmodule
