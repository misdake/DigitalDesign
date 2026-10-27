// One ordered 512x36 source-record authority. Layout of words belongs to caller.
module gpu_v2_source_record(
 input wire clk,reset,ce,abort,
 input wire write_valid,input wire [35:0] write_data,
 output wire write_ready,output wire [3:0] producer_slot,
 output wire [4:0] write_row,used,
 input wire read_valid,read_prepare,input wire [4:0] read_row,
 output wire read_ready,
 output reg return_valid,input wire return_ready,
 output reg [3:0] return_slot,output reg [4:0] return_row,
 output wire [35:0] return_data,
 input wire release_valid,output wire release_ready,
 output wire published,released,fault
);
reg [4:0] allocate,retire,write_cursor;
reg stopped;
assign producer_slot=allocate[3:0];
assign write_row=write_cursor;
assign used=allocate-retire;
assign fault=abort||stopped;
wire available=!reset&&!fault;
assign write_ready=available&&(used<5'd16);
wire readable=read_prepare?(read_row<write_cursor):(used!=0);
assign read_ready=available&&(!return_valid||return_ready)&&readable;
assign release_ready=available&&(used!=0)
 &&!(return_valid&&return_slot==retire[3:0])&&!(read_valid&&!read_prepare);
wire write_fire=ce&&write_valid&&write_ready;
wire read_fire=ce&&read_valid&&read_ready;
assign published=write_fire&&write_cursor==5'd31;
assign released=ce&&release_valid&&release_ready;
wire [3:0] read_slot=read_prepare?allocate[3:0]:retire[3:0];
wire [8:0] read_address={read_slot,read_row};
wire [8:0] write_address={allocate[3:0],write_cursor};
`ifdef GPU_SOURCE_RECORD_NATIVE
SDPX9B #(.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.READ_MODE(1'b0),
 .BLK_SEL_0(3'b000),.BLK_SEL_1(3'b000),.RESET_MODE("SYNC")) record_bank(
 .DO(return_data),.DI(write_data),
 .ADA({write_address,1'b0,4'b1111}),.ADB({read_address,5'b00000}),
 .BLKSELA(3'b000),.BLKSELB(3'b000),.CLKA(clk),.CLKB(clk),
 .CEA(write_fire),.CEB(read_fire),.OCE(1'b1),.RESETA(reset),.RESETB(reset));
`else
reg [35:0] record_bank[0:511];
reg [35:0] ram_do;
assign return_data=ram_do;
always @(posedge clk)begin
 if(write_fire)record_bank[write_address]<=write_data;
 if(reset)ram_do<=0;else if(read_fire)ram_do<=record_bank[read_address];
end
`endif
always @(posedge clk)begin
 if(reset)begin
  allocate<=0;retire<=0;write_cursor<=0;return_valid<=0;
  return_slot<=0;return_row<=0;stopped<=0;
 end else begin
  if(abort)stopped<=1;
  if(ce&&!fault)begin
   if(read_fire)begin
    return_valid<=1;return_slot<=read_slot;return_row<=read_row;
   end else if(return_ready)return_valid<=0;
   if(write_fire)begin
    write_cursor<=write_cursor+5'd1;
    if(published)allocate<=allocate+5'd1;
   end
   if(released)retire<=retire+5'd1;
  end
 end
end
endmodule
