// One cold-start owner. Declaration initial values are FPGA configuration state.
// No runtime reset, programmable destination, section table or CPU/MMIO loader.
module SingleBootLoader #(
    parameter TIMEOUT_BITS = 20
) (
    input wire clk, input wire ready_to_start,
    input wire flash_ready, input wire flash_data_valid,
    input wire [7:0] flash_data, input wire flash_done, input wire flash_error,
    input wire memory_request_ready, input wire memory_response_valid, input wire memory_error,
    output wire flash_start, output wire [23:0] flash_address, output wire [23:0] flash_length,
    output wire flash_data_ready,
    output wire memory_request_valid, output wire [21:0] memory_address,
    output wire [15:0] memory_write_data, output wire memory_response_ready,
    output wire done, output wire error, output wire [3:0] error_code
);
localparam WAIT=0, HEADER_START=1, HEADER=2, HEADER_DONE=3, ZERO_REQUEST=4,
 ZERO_RESPONSE=5, CODE_START=6, LOW_BYTE=7, HIGH_BYTE=8, CODE_REQUEST=9,
 CODE_RESPONSE=10, CODE_DONE=11, FINISHED=12, FAILED=13;
reg [3:0] state=WAIT;
reg [4:0] header_index=0;
reg [17:0] code_bytes=0;
reg [16:0] word_index=0;
reg [7:0] low_byte=0;
reg [15:0] write_data=0;
reg [3:0] failure=0;
reg [TIMEOUT_BITS-1:0] timeout_count=0;
reg flash_finished=0;
wire flash_fire=flash_data_valid && flash_data_ready;
wire request_fire=memory_request_valid && memory_request_ready;
wire response_fire=memory_response_valid && memory_response_ready;
wire zeroing=state==ZERO_REQUEST || state==ZERO_RESPONSE;
assign flash_start=(state==HEADER_START || state==CODE_START) && flash_ready;
assign flash_address=state==HEADER_START ? 24'h100000 : 24'h100080;
assign flash_length=state==HEADER_START ? 24'd32 : {6'b0,code_bytes};
assign flash_data_ready=state==HEADER || state==LOW_BYTE || state==HIGH_BYTE;
assign memory_request_valid=state==ZERO_REQUEST || state==CODE_REQUEST;
assign memory_address=zeroing ? {5'b0,word_index} : {6'b000111,word_index[15:0]};
assign memory_write_data=zeroing ? 16'b0 : write_data;
assign memory_response_ready=state==ZERO_RESPONSE || state==CODE_RESPONSE;
assign done=state==FINISHED;
assign error=state==FAILED;
assign error_code=failure;
function [7:0] expected_header;
 input [4:0] index;
 begin
  case(index)
   0:expected_header=8'h43;1:expected_header=8'h50;2:expected_header=8'h55;3:expected_header=8'h33;
   4:expected_header=8'h42;5:expected_header=8'h4f;6:expected_header=8'h4f;7:expected_header=8'h54;
   8:expected_header=8'd5;10:expected_header=8'd32;
   default:expected_header=0;
  endcase
 end
endfunction
always @(posedge clk) begin
 if (flash_done) flash_finished<=1;
 if(state!=WAIT && state!=FINISHED && state!=FAILED) begin
  if(flash_fire || request_fire || response_fire || flash_start || flash_done) timeout_count<=0;
  else timeout_count<=timeout_count+1'b1;
 end
 case(state)
 WAIT:if(ready_to_start) state<=HEADER_START;
 HEADER_START:if(flash_ready)begin state<=HEADER;flash_finished<=0;end
 HEADER:if(flash_fire)begin
  header_index<=header_index+1'b1;
  if(header_index==12)code_bytes[7:0]<=flash_data;
  else if(header_index==13)code_bytes[15:8]<=flash_data;
  else if(header_index==14)begin
   code_bytes[17:16]<=flash_data[1:0];
   if(flash_data[7:2]!=0)begin state<=FAILED;failure<=1;end
  end else if(flash_data!=expected_header(header_index))begin state<=FAILED;failure<=1;end
  if(header_index==31 && flash_data==0)state<=HEADER_DONE;
 end
 HEADER_DONE:if(flash_finished || flash_done)begin
  if(code_bytes==0 || code_bytes[0] || code_bytes>18'h20000)begin state<=FAILED;failure<=2;end
  else begin word_index<=0;state<=ZERO_REQUEST;end
 end
 ZERO_REQUEST:if(request_fire)state<=ZERO_RESPONSE;
 ZERO_RESPONSE:if(response_fire)begin
  if(word_index==8191)begin word_index<=0;state<=CODE_START;end
  else begin word_index<=word_index+1'b1;state<=ZERO_REQUEST;end
 end
 CODE_START:if(flash_ready)begin state<=LOW_BYTE;flash_finished<=0;end
 LOW_BYTE:if(flash_fire)begin low_byte<=flash_data;state<=HIGH_BYTE;end
 HIGH_BYTE:if(flash_fire)begin write_data<={flash_data,low_byte};state<=CODE_REQUEST;end
 CODE_REQUEST:if(request_fire)state<=CODE_RESPONSE;
 CODE_RESPONSE:if(response_fire)begin
  word_index<=word_index+1'b1;
  if(word_index+1'b1==code_bytes[17:1])state<=CODE_DONE;
  else state<=LOW_BYTE;
 end
 CODE_DONE:if(flash_finished || flash_done)state<=FINISHED;
 default:begin end
 endcase
 // Fail has priority over every normal transition, including the last write.
 if(state!=WAIT && state!=FINISHED && state!=FAILED)begin
  if(flash_error)begin state<=FAILED;failure<=3;end
  else if(response_fire && memory_error)begin state<=FAILED;failure<=4;end
  else if(&timeout_count)begin state<=FAILED;failure<=5;end
 end
end
endmodule
