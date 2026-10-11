//! Four-row ingress into one UV RAM and the single D operand bank.
//! Raw metadata writes straight through to its canonical bank on row0. The
//! parent owns Context8 reservation/publication and must not read unowned slots.
//! All command consumption is CE-enabled; reserved RAM returns are wall-clocked.
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
const BODY: &str = r#"module gpu_v2_uv_rows(
 input wire clk,reset,ce,
 input wire [2:0] phase,
 input wire in_valid,
 input wire [2:0] in_slot,
 input wire [35:0] in_metadata,in_uv,
 output wire in_ready,
 output wire [1:0] in_row,
 output wire raw_write,
 output wire [2:0] raw_slot,
 output wire [35:0] raw_metadata,
 output wire committed,
 output wire [2:0] committed_slot,
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
 output wire [35:0] write_data);
 reg raw_active,raw_full;
 reg [2:0] owner;
 reg [1:0] next;
 reg [143:0] operands;
 reg c_reserved,c_ready,pending;
 reg [2:0] owner_slot;
 reg [1:0] owner_lane;
 reg [31:0] c_hold;
 wire [35:0] ram_do;
 assign in_ready=!reset && !raw_full;
 wire accepted=ce && in_valid && in_ready;
 assign in_row=next;
 assign raw_write=accepted && !raw_active;
 assign raw_slot=in_slot;
 assign raw_metadata=in_metadata;
 assign committed=accepted && raw_active && next==3;
 assign committed_slot=owner;
 assign d_valid=!reset && raw_full;
 assign d_slot=owner;
 assign d_uv=operands;
 assign c_empty=!c_reserved;
 assign c_valid=!reset && c_reserved && (c_ready || pending);
 assign c_uv=pending ? {ram_do[33:18],ram_do[15:0]} : c_hold;
 assign c_owner_slot=owner_slot;
 assign c_owner_lane=owner_lane;
 assign write_enable=!reset && accepted;
 assign write_address=raw_active ? {owner,next} : {in_slot,2'b00};
 assign write_data=in_uv;
 wire c_committed=(!raw_active || raw_full || owner!=c_slot) &&
   (!accepted || (raw_active ? owner : in_slot)!=c_slot);
 assign read_enable=!reset && ce && phase[0] && c_request && !c_reserved && c_committed;
 assign read_address={c_slot,c_lane};
 // MEMORY
 always @(posedge clk) begin
   if(reset) begin
     raw_active<=0;raw_full<=0;next<=0;owner<=0;
     c_reserved<=0;c_ready<=0;pending<=0;owner_slot<=0;owner_lane<=0;
   end else begin
     pending<=read_enable;
     if(pending && !(ce && c_take)) begin
       c_hold<={ram_do[33:18],ram_do[15:0]};c_ready<=1;
     end
     if(ce) begin
       if(d_take) begin raw_active<=0;raw_full<=0;next<=0;end
       if(accepted) begin
         if(!raw_active) begin owner<=in_slot;raw_active<=1;end
         case(next)
           0: operands[35:0]<=in_uv;
           1: operands[71:36]<=in_uv;
           2: operands[107:72]<=in_uv;
           3: operands[143:108]<=in_uv;
         endcase
         next<=next+1'b1;
         if(next==3) raw_full<=1;
       end
       if(c_take) begin c_reserved<=0;c_ready<=0;end
       if(read_enable) begin
         owner_slot<=c_slot;owner_lane<=c_lane;c_reserved<=1;c_ready<=0;
       end
     end
   end
 end
endmodule
"#;
