//! UV payload controller matching `emu::uv_store`, including unclock-enabled
//! capture of an already reserved synchronous return. One native SDP36 block.
pub fn source(native: bool) -> String {
    let memory = if native {
        r#"  SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),
    .BLK_SEL_0(3'b000),.BLK_SEL_1(3'b000),.RESET_MODE("SYNC")) payload(
    .DO(ram_do),.DI(write_data),.BLKSELA(3'b000),.BLKSELB(3'b000),
    .ADA({4'b0000,write_address,1'b0,4'b1111}),
    .ADB({4'b0000,read_address,5'b00000}),
    .CLKA(clk),.CLKB(clk),.CEA(write_enable),.CEB(read_enable),
    .OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
"#
    } else {
        r#"  reg [35:0] ram[0:31];
  reg [35:0] data_return;
  assign ram_do=data_return;
  always @(posedge clk) begin
    if(write_enable) ram[write_address]<=write_data;
    if(read_enable) data_return<=ram[read_address];
  end
"#
    };
    BODY.replace("// MEMORY", memory)
}
const BODY: &str = r#"module gpu_v2_uv_store(
 input wire clk,reset,ce,
 input wire [2:0] phase,
 input wire in_valid,
 input wire [2:0] in_slot,
 input wire [143:0] in_uv,
 output wire in_ready,
 input wire d_take,
 output wire d_valid,
 output wire [2:0] d_slot,
 output wire [143:0] d_uv,
 input wire c_request,
 input wire [2:0] c_slot,
 input wire [1:0] c_lane,
 input wire c_take,
 output wire c_empty,c_valid,
 output wire [2:0] c_owner_slot,
 output wire [1:0] c_owner_lane,
 output wire [31:0] c_uv,
 output wire write_enable,read_enable,
 output wire [4:0] write_address,read_address,
 output reg [35:0] write_data);
 reg raw_valid,raw_taken;
 reg [2:0] raw_slot,raw_next;
 reg [143:0] raw_data;
 reg c_reserved,c_ready,pending;
 reg [2:0] owner_slot;
 reg [1:0] owner_lane;
 reg [31:0] c_hold;
 wire [35:0] ram_do;
 assign in_ready=!reset && !raw_valid;
 wire accepted=ce && in_valid && in_ready;
 assign d_valid=!reset && raw_valid && !raw_taken;
 assign d_slot=raw_slot;
 assign d_uv=raw_data;
 assign c_empty=!c_reserved;
 assign c_valid=!reset && c_reserved && (c_ready || pending);
 assign c_uv=pending ? {ram_do[33:18],ram_do[15:0]} : c_hold;
 assign c_owner_slot=owner_slot;
 assign c_owner_lane=owner_lane;
 assign write_enable=!reset && ce && ((raw_valid && raw_next<4) || accepted);
 assign write_address=raw_valid ? {raw_slot,raw_next[1:0]} : {in_slot,2'b00};
 always @* begin
   if(!raw_valid) write_data=in_uv[35:0];
   else case(raw_next[1:0])
     0: write_data=raw_data[35:0];
     1: write_data=raw_data[71:36];
     2: write_data=raw_data[107:72];
     default: write_data=raw_data[143:108];
   endcase
 end
 assign read_enable=!reset && ce && phase[0] && c_request && !c_reserved;
 assign read_address={c_slot,c_lane};
 // MEMORY
 always @(posedge clk) begin
   if(reset) begin
     raw_valid<=0;raw_taken<=0;raw_next<=0;raw_slot<=0;
     c_reserved<=0;c_ready<=0;pending<=0;owner_slot<=0;owner_lane<=0;
   end else begin
     // The previous hard DO belongs to a reserved C destination, even on CE0.
     pending<=read_enable;
     if(pending && !(ce && c_take)) begin
       c_hold<={ram_do[33:18],ram_do[15:0]};c_ready<=1;
     end
     if(ce) begin
       if(accepted) begin
         raw_data<=in_uv;raw_slot<=in_slot;raw_next<=1;
         raw_valid<=1;raw_taken<=0;
       end
       if(raw_valid && raw_next<4) raw_next<=raw_next+1'b1;
       if(d_take) raw_taken<=1;
       if(raw_valid && (raw_next==4 || (raw_next==3 && write_enable)) && (raw_taken || d_take)) raw_valid<=0;
       if(c_take) begin c_reserved<=0;c_ready<=0;end
       if(read_enable) begin
         owner_slot<=c_slot;owner_lane<=c_lane;c_reserved<=1;c_ready<=0;
       end
     end
   end
 end
endmodule
"#;
